use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use tempfile::{TempDir, tempdir};

const DEFAULT_IMAGE: &str = "openclaw:engulfed";
const DEFAULT_SERVICE: &str = "openclaw-gateway";
const DEFAULT_BRANCH_PREFIX: &str = "local/release-";
const DEFAULT_AUTHOR_NAME: &str = "clawyer";
const DEFAULT_AUTHOR_EMAIL: &str = "clawyer@local";

#[derive(Parser, Debug)]
#[command(
    name = "clawyer",
    about = "Docker helpers for OpenClaw - lawyer for your claw!"
)]
struct Cli {
    #[arg(short = 'D', long = "dir")]
    dir: Option<PathBuf>,
    #[arg(short, long)]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Start,
    Stop,
    Restart,
    Logs,
    Status,
    Health,
    Shell,
    Exec(ExecArgs),
    Run(RunArgs),
    Rebuild,
    Upgrade(UpgradeArgs),
    RepairConflicts,
    Clean,
    Token,
    FixToken,
    Dashboard,
    Devices,
    Approve(ApproveArgs),
    Cd,
    Config,
    Workspace,
    Init,
    Tui,
}

#[derive(Args, Debug)]
struct ExecArgs {
    #[arg(last = true)]
    cmd: Vec<String>,
}

#[derive(Args, Debug)]
struct RunArgs {
    #[arg(last = true)]
    args: Vec<String>,
}

#[derive(Args, Debug)]
struct UpgradeArgs {
    #[arg(long = "dry-run")]
    dry_run: bool,
    #[arg(long = "ref")]
    ref_override: Option<String>,
    #[arg(long = "drop-nonpreserved")]
    drop_nonpreserved: bool,
    #[arg(long = "no-build")]
    no_build: bool,
    #[arg(long = "no-restart")]
    no_restart: bool,
}

#[derive(Args, Debug)]
struct ApproveArgs {
    request_id: String,
}

#[derive(Debug)]
struct App {
    root: PathBuf,
    verbose: bool,
    image: String,
    notion_api_key: Option<String>,
    compose_override_dir: TempDir,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StableTag {
    tag: String,
    year: u32,
    major: u32,
    minor: u32,
    legacy_patch: u32,
}

impl Ord for StableTag {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.year, self.major, self.minor, self.legacy_patch).cmp(&(
            other.year,
            other.major,
            other.minor,
            other.legacy_patch,
        ))
    }
}

impl PartialOrd for StableTag {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let app = App::new(cli.dir, cli.verbose)?;
    dispatch(&app, cli.command)
}

fn dispatch(app: &App, command: Commands) -> Result<()> {
    match command {
        Commands::Start => {
            app.compose_status(["up", "-d", DEFAULT_SERVICE])?;
            Ok(())
        }
        Commands::Stop => {
            app.compose_status(["down"])?;
            Ok(())
        }
        Commands::Restart => restart(app),
        Commands::Logs => {
            app.compose_status(["logs", "-f", DEFAULT_SERVICE])?;
            Ok(())
        }
        Commands::Status => {
            app.compose_status(["ps"])?;
            Ok(())
        }
        Commands::Health => health(app),
        Commands::Shell => {
            app.compose_status(["exec", DEFAULT_SERVICE, "bash"])?;
            Ok(())
        }
        Commands::Exec(args) => exec_gateway(app, &args.cmd),
        Commands::Run(args) => run_openclaw(app, &args.args),
        Commands::Rebuild => rebuild(app),
        Commands::Upgrade(args) => upgrade(app, args),
        Commands::RepairConflicts => repair_conflicts(app),
        Commands::Clean => {
            app.compose_status(["down", "-v", "--remove-orphans"])?;
            Ok(())
        }
        Commands::Token => {
            println!(
                "{}",
                read_dotenv_value(&app.root.join(".env"), "OPENCLAW_GATEWAY_TOKEN")?
            );
            Ok(())
        }
        Commands::FixToken => fix_token(app),
        Commands::Dashboard => run_openclaw(app, &["dashboard".into(), "--no-open".into()]),
        Commands::Devices => run_openclaw(app, &["devices".into(), "list".into()]),
        Commands::Approve(args) => {
            run_openclaw(app, &["devices".into(), "approve".into(), args.request_id])
        }
        Commands::Cd | Commands::Init => {
            println!("{}", app.root.display());
            Ok(())
        }
        Commands::Config => {
            println!("{}", home_dir()?.join(".openclaw").display());
            Ok(())
        }
        Commands::Workspace => {
            println!("{}", home_dir()?.join(".openclaw/workspace").display());
            Ok(())
        }
        Commands::Tui => {
            println!(
                "TUI is not implemented yet. Use `clawyer status`, `clawyer health`, or `clawyer logs`."
            );
            Ok(())
        }
    }
}

impl App {
    fn new(dir: Option<PathBuf>, verbose: bool) -> Result<Self> {
        let root = resolve_repo_root(dir)?;
        let image = resolve_image(&root)?;
        let notion_api_key = resolve_notion_api_key()?;
        let compose_override_dir = tempdir().context("create compose override temp dir")?;
        let app = Self {
            root,
            verbose,
            image,
            notion_api_key,
            compose_override_dir,
        };
        app.write_compose_override()?;
        Ok(app)
    }

    fn compose_files(&self) -> Vec<PathBuf> {
        let mut files = vec![self.root.join("docker-compose.yml")];
        let extra = self.root.join("docker-compose.extra.yml");
        if extra.exists() {
            files.push(extra);
        }
        files.push(
            self.compose_override_dir
                .path()
                .join("docker-compose.clawyer.override.yml"),
        );
        files
    }

    fn base_env(&self) -> Vec<(String, String)> {
        let mut vars = vec![("OPENCLAW_IMAGE".to_string(), self.image.clone())];
        if let Some(key) = &self.notion_api_key {
            vars.push(("NOTION_API_KEY".to_string(), key.clone()));
        }
        vars
    }

    fn compose_command(&self) -> Command {
        let mut cmd = Command::new("docker");
        cmd.arg("compose");
        for file in self.compose_files() {
            cmd.arg("-f").arg(file);
        }
        cmd.current_dir(&self.root);
        for (key, value) in self.base_env() {
            cmd.env(key, value);
        }
        cmd
    }

    fn compose_output<I, S>(&self, args: I) -> Result<std::process::Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.compose_command();
        cmd.args(args);
        if self.verbose {
            eprintln!("+ {}", format_command(&cmd));
        }
        cmd.output()
            .with_context(|| "run docker compose".to_string())
    }

    fn compose_status<I, S>(&self, args: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let status = self.compose_status_code(args)?;
        if !status.success() {
            bail!(
                "docker compose failed with exit code {}",
                exit_code(status.code())
            );
        }
        Ok(())
    }

    fn compose_status_code<I, S>(&self, args: I) -> Result<std::process::ExitStatus>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.compose_command();
        cmd.args(args);
        if self.verbose {
            eprintln!("+ {}", format_command(&cmd));
        }
        cmd.status()
            .with_context(|| "run docker compose".to_string())
    }

    fn write_compose_override(&self) -> Result<()> {
        let override_path = self
            .compose_override_dir
            .path()
            .join("docker-compose.clawyer.override.yml");
        let mut file = fs::File::create(&override_path)
            .with_context(|| format!("create {}", override_path.display()))?;
        file.write_all(
            br#"services:
  openclaw-gateway:
    environment:
      NOTION_API_KEY: ${NOTION_API_KEY:-}
  openclaw-cli:
    environment:
      NOTION_API_KEY: ${NOTION_API_KEY:-}
"#,
        )
        .with_context(|| format!("write {}", override_path.display()))?;
        Ok(())
    }
}

fn resolve_repo_root(dir: Option<PathBuf>) -> Result<PathBuf> {
    let root = match dir {
        Some(path) => path,
        None => env::current_dir().context("read current directory")?,
    };
    let root = fs::canonicalize(&root).with_context(|| format!("resolve {}", root.display()))?;
    if !root.join("docker-compose.yml").exists() {
        bail!(
            "{} does not look like an OpenClaw Docker checkout (missing docker-compose.yml)",
            root.display()
        );
    }
    Ok(root)
}

fn resolve_image(root: &Path) -> Result<String> {
    if let Ok(value) = env::var("OPENCLAW_IMAGE") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Some(value) = read_optional_dotenv_value(&root.join(".env"), "OPENCLAW_IMAGE")? {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Some(value) = inspect_running_image(root)? {
        return Ok(value);
    }
    Ok(DEFAULT_IMAGE.to_string())
}

fn inspect_running_image(root: &Path) -> Result<Option<String>> {
    let output = Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(root.join("docker-compose.yml"))
        .arg("ps")
        .arg("-q")
        .arg(DEFAULT_SERVICE)
        .current_dir(root)
        .output()
        .context("inspect running compose service")?;
    if !output.status.success() {
        return Ok(None);
    }
    let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if container_id.is_empty() {
        return Ok(None);
    }
    let inspect = Command::new("docker")
        .arg("inspect")
        .arg("--format")
        .arg("{{.Config.Image}}")
        .arg(&container_id)
        .output()
        .context("inspect running container image")?;
    if !inspect.status.success() {
        return Ok(None);
    }
    let image = String::from_utf8_lossy(&inspect.stdout).trim().to_string();
    if image.is_empty() {
        return Ok(None);
    }
    Ok(Some(image))
}

fn resolve_notion_api_key() -> Result<Option<String>> {
    if let Ok(value) = env::var("NOTION_API_KEY") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }
    let key_path = home_dir()?.join(".config/notion/api_key");
    if key_path.exists() {
        let value = fs::read_to_string(&key_path)
            .with_context(|| format!("read {}", key_path.display()))?;
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }
    Ok(None)
}

fn read_dotenv_value(path: &Path, key: &str) -> Result<String> {
    read_optional_dotenv_value(path, key)?
        .ok_or_else(|| anyhow!("missing {key} in {}", path.display()))
}

fn read_optional_dotenv_value(path: &Path, key: &str) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((candidate, raw_value)) = trimmed.split_once('=') {
            if candidate.trim() == key {
                let value = raw_value.trim().trim_matches('"').trim_matches('\'');
                return Ok(Some(value.to_string()));
            }
        }
    }
    Ok(None)
}

fn rebuild(app: &App) -> Result<()> {
    let mut cmd = Command::new("docker");
    cmd.arg("build")
        .arg("-t")
        .arg(&app.image)
        .arg("-f")
        .arg("Dockerfile")
        .arg(".");
    cmd.current_dir(&app.root);
    if app.verbose {
        eprintln!("+ {}", format_command(&cmd));
    }
    let status = cmd.status().context("run docker build")?;
    if !status.success() {
        bail!(
            "docker build failed with exit code {}",
            exit_code(status.code())
        );
    }
    let output = Command::new("docker")
        .arg("image")
        .arg("inspect")
        .arg("--format")
        .arg("{{.Id}}")
        .arg(&app.image)
        .output()
        .context("inspect rebuilt image")?;
    if output.status.success() {
        println!(
            "Rebuilt {} -> {}",
            app.image,
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }
    Ok(())
}

fn restart(app: &App) -> Result<()> {
    app.compose_status(["up", "-d", "--force-recreate", DEFAULT_SERVICE])?;
    Ok(())
}

fn run_openclaw(app: &App, args: &[String]) -> Result<()> {
    let status = app.compose_status_code(
        ["run", "--rm", "--no-deps", "openclaw-cli"]
            .into_iter()
            .map(str::to_string)
            .chain(args.iter().cloned()),
    )?;
    if !status.success() {
        bail!(
            "docker compose run failed with exit code {}",
            exit_code(status.code())
        );
    }
    Ok(())
}

fn exec_gateway(app: &App, cmd: &[String]) -> Result<()> {
    if cmd.is_empty() {
        bail!("Usage: clawyer exec -- <CMD...>");
    }
    let status = app.compose_status_code(
        ["exec", "-T", DEFAULT_SERVICE]
            .into_iter()
            .map(str::to_string)
            .chain(cmd.iter().cloned()),
    )?;
    if !status.success() {
        bail!(
            "docker compose exec failed with exit code {}",
            exit_code(status.code())
        );
    }
    Ok(())
}

fn health(app: &App) -> Result<()> {
    let ps = app.compose_output(["ps"])?;
    print!("{}", String::from_utf8_lossy(&ps.stdout));
    let script = r#"fetch("http://127.0.0.1:18789/healthz")
  .then(async (r) => {
    const text = await r.text();
    console.log(text);
    process.exit(r.ok ? 0 : 1);
  })
  .catch((err) => {
    console.error(err.message);
    process.exit(1);
  });"#;
    let output = app.compose_output(["exec", "-T", DEFAULT_SERVICE, "node", "-e", script])?;
    if !output.status.success() {
        bail!(
            "gateway health probe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    println!("{}", String::from_utf8_lossy(&output.stdout).trim());
    Ok(())
}

fn fix_token(app: &App) -> Result<()> {
    let token = read_dotenv_value(&app.root.join(".env"), "OPENCLAW_GATEWAY_TOKEN")?;
    run_openclaw(
        app,
        &[
            "config".into(),
            "set".into(),
            "gateway.remote.token".into(),
            token.clone(),
        ],
    )?;
    run_openclaw(
        app,
        &[
            "config".into(),
            "set".into(),
            "gateway.auth.token".into(),
            token,
        ],
    )?;
    restart(app)
}

fn upgrade(app: &App, args: UpgradeArgs) -> Result<()> {
    let tag = match args.ref_override {
        Some(raw) => raw,
        None => resolve_latest_stable_tag(&app.root)?,
    };
    let branch = format!("{DEFAULT_BRANCH_PREFIX}{tag}");
    let preserve = preserved_roots(&app.root, &tag)?;
    let dirty = collect_dirty_paths(&app.root)?;
    let dropped: Vec<_> = dirty
        .iter()
        .filter(|path| !path_is_preserved(path, &preserve))
        .cloned()
        .collect();

    if args.dry_run {
        println!("targetTag={tag}");
        println!("branch={branch}");
        println!("image={}", app.image);
        println!("preserve:");
        for path in &preserve {
            println!("- {}", path.display());
        }
        println!("drop:");
        for path in &dropped {
            println!("- {}", path.display());
        }
        return Ok(());
    }

    if !dropped.is_empty() && !args.drop_nonpreserved {
        bail!(
            "non-preserved local paths would be dropped; re-run with --drop-nonpreserved after reviewing:\n{}",
            dropped
                .iter()
                .map(|path| format!("- {}", path.display()))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    let snapshot = snapshot_preserved_paths(&app.root, &preserve)?;
    if !dirty.is_empty() {
        git_status(&app.root, ["reset", "--hard", "HEAD"])?;
        git_status(&app.root, ["clean", "-fd"])?;
    }

    git_status(&app.root, ["fetch", "--tags", "origin"])?;
    git_status(&app.root, ["switch", "-C", &branch, &tag])?;
    restore_snapshot(snapshot.path(), &app.root)?;
    stage_paths(&app.root, &preserve)?;
    commit_overlay_if_needed(&app.root, &tag)?;

    if !args.no_build {
        rebuild(app)?;
    }
    if !args.no_restart {
        restart(app)?;
    }
    health(app)?;
    Ok(())
}

fn resolve_latest_stable_tag(root: &Path) -> Result<String> {
    let output = git_output(root, ["ls-remote", "--tags", "--refs", "origin"])?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let best = stdout
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter_map(|ref_name| ref_name.strip_prefix("refs/tags/"))
        .filter_map(parse_stable_tag)
        .max()
        .ok_or_else(|| anyhow!("could not resolve a stable release tag from origin"))?;
    Ok(best.tag)
}

fn parse_stable_tag(tag: &str) -> Option<StableTag> {
    if !tag.starts_with('v') || tag.contains("beta") {
        return None;
    }
    let core = &tag[1..];
    let (version_part, legacy_patch) = if let Some((base, suffix)) = core.split_once('-') {
        let patch = suffix.parse::<u32>().ok()?;
        (base, patch)
    } else {
        (core, 0)
    };
    let mut parts = version_part.split('.');
    let year = parts.next()?.parse::<u32>().ok()?;
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(StableTag {
        tag: tag.to_string(),
        year,
        major,
        minor,
        legacy_patch,
    })
}

fn preserved_roots(root: &Path, tag: &str) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    if root.join("crates/clawyer").exists() {
        roots.push(PathBuf::from("crates/clawyer"));
    }
    let skills_dir = root.join("skills");
    if skills_dir.exists() {
        for entry in
            fs::read_dir(&skills_dir).with_context(|| format!("read {}", skills_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() || !path.join("SKILL.md").exists() {
                continue;
            }
            let skill_name = entry.file_name();
            let repo_path = format!("skills/{}/SKILL.md", skill_name.to_string_lossy());
            if !git_path_exists(root, tag, &repo_path)? {
                roots.push(PathBuf::from("skills").join(skill_name));
            }
        }
    }
    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn git_path_exists(root: &Path, rev: &str, repo_path: &str) -> Result<bool> {
    let status = Command::new("git")
        .arg("cat-file")
        .arg("-e")
        .arg(format!("{rev}:{repo_path}"))
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("git cat-file {rev}:{repo_path}"))?;
    Ok(status.success())
}

fn collect_dirty_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let output = git_output(root, ["status", "--porcelain=v1", "--untracked-files=all"])?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut paths = Vec::new();
    for line in stdout.lines() {
        if line.len() < 4 {
            continue;
        }
        let raw = &line[3..];
        let path = raw
            .split(" -> ")
            .last()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("could not parse git status line: {line}"))?;
        paths.push(PathBuf::from(path));
    }
    Ok(paths)
}

fn path_is_preserved(path: &Path, preserve: &[PathBuf]) -> bool {
    preserve
        .iter()
        .any(|root| path == root || path.starts_with(root))
}

fn snapshot_preserved_paths(root: &Path, preserve: &[PathBuf]) -> Result<TempDir> {
    let snapshot = tempdir().context("create preserved snapshot dir")?;
    for rel in preserve {
        let src = root.join(rel);
        if src.exists() {
            copy_path(&src, &snapshot.path().join(rel))?;
        }
    }
    Ok(snapshot)
}

fn restore_snapshot(snapshot_root: &Path, dest_root: &Path) -> Result<()> {
    for entry in walk_paths(snapshot_root)? {
        let rel = entry
            .strip_prefix(snapshot_root)
            .with_context(|| format!("strip prefix {}", entry.display()))?;
        let dest = dest_root.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&dest).with_context(|| format!("create {}", dest.display()))?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        fs::copy(&entry, &dest)
            .with_context(|| format!("copy {} -> {}", entry.display(), dest.display()))?;
    }
    Ok(())
}

fn copy_path(src: &Path, dest: &Path) -> Result<()> {
    if src.is_dir() {
        fs::create_dir_all(dest).with_context(|| format!("create {}", dest.display()))?;
        for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
            let entry = entry?;
            let name = entry.file_name();
            if name == OsStr::new(".git") || name == OsStr::new("target") {
                continue;
            }
            copy_path(&entry.path(), &dest.join(name))?;
        }
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::copy(src, dest).with_context(|| format!("copy {} -> {}", src.display(), dest.display()))?;
    Ok(())
}

fn walk_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        out.push(path.clone());
        if path.is_dir() {
            let mut children = fs::read_dir(&path)
                .with_context(|| format!("read {}", path.display()))?
                .map(|entry| entry.map(|item| item.path()))
                .collect::<std::result::Result<Vec<_>, _>>()
                .with_context(|| format!("read {}", path.display()))?;
            children.sort();
            for child in children.into_iter().rev() {
                stack.push(child);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn stage_paths(root: &Path, preserve: &[PathBuf]) -> Result<()> {
    if preserve.is_empty() {
        return Ok(());
    }
    let mut cmd = Command::new("git");
    cmd.arg("add").arg("-A").arg("--");
    for path in preserve {
        cmd.arg(path);
    }
    cmd.current_dir(root);
    let status = cmd.status().context("git add preserved paths")?;
    if !status.success() {
        bail!("git add failed with exit code {}", exit_code(status.code()));
    }
    Ok(())
}

fn commit_overlay_if_needed(root: &Path, tag: &str) -> Result<()> {
    let diff = Command::new("git")
        .arg("diff")
        .arg("--cached")
        .arg("--quiet")
        .current_dir(root)
        .status()
        .context("git diff --cached --quiet")?;
    if diff.success() {
        return Ok(());
    }
    let mut cmd = Command::new("git");
    cmd.arg("commit")
        .arg("-m")
        .arg(format!("Local overlay for {tag}"))
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", DEFAULT_AUTHOR_NAME)
        .env("GIT_AUTHOR_EMAIL", DEFAULT_AUTHOR_EMAIL)
        .env("GIT_COMMITTER_NAME", DEFAULT_AUTHOR_NAME)
        .env("GIT_COMMITTER_EMAIL", DEFAULT_AUTHOR_EMAIL);
    let status = cmd.status().context("git commit overlay")?;
    if !status.success() {
        bail!(
            "git commit failed with exit code {}",
            exit_code(status.code())
        );
    }
    Ok(())
}

fn repair_conflicts(app: &App) -> Result<()> {
    let preserve = preserved_roots(&app.root, "HEAD")?;
    let rebase_in_progress =
        app.root.join(".git/rebase-merge").exists() || app.root.join(".git/rebase-apply").exists();
    let merge_in_progress = app.root.join(".git/MERGE_HEAD").exists();
    if !rebase_in_progress && !merge_in_progress {
        bail!("no rebase or merge conflict is in progress");
    }
    let output = git_output(&app.root, ["diff", "--name-only", "--diff-filter=U"])?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let conflicts: Vec<PathBuf> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect();
    if conflicts.is_empty() {
        bail!("no unmerged paths found");
    }
    let prefer_upstream = if rebase_in_progress {
        "--ours"
    } else {
        "--theirs"
    };
    let prefer_local = if rebase_in_progress {
        "--theirs"
    } else {
        "--ours"
    };
    for path in &conflicts {
        let selector = if path_is_preserved(path, &preserve) {
            prefer_local
        } else {
            prefer_upstream
        };
        git_status(
            &app.root,
            ["checkout", selector, "--", &path.to_string_lossy()],
        )?;
        git_status(&app.root, ["add", "--", &path.to_string_lossy()])?;
    }
    if rebase_in_progress {
        git_status(&app.root, ["rebase", "--continue"])?;
    } else {
        git_status(&app.root, ["merge", "--continue"])?;
    }
    Ok(())
}

fn git_output<I, S>(root: &Path, args: I) -> Result<std::process::Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .context("run git")?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output)
}

fn git_status<I, S>(root: &Path, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .context("run git")?;
    if !status.success() {
        bail!(
            "git command failed with exit code {}",
            exit_code(status.code())
        );
    }
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}

fn exit_code(code: Option<i32>) -> i32 {
    code.unwrap_or(1)
}

fn format_command(cmd: &Command) -> String {
    let mut parts = Vec::new();
    parts.push(cmd.get_program().to_string_lossy().to_string());
    parts.extend(cmd.get_args().map(|arg| arg.to_string_lossy().to_string()));
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::{StableTag, parse_stable_tag};

    #[test]
    fn parses_plain_stable_tags() {
        let parsed = parse_stable_tag("v2026.3.12").expect("stable tag");
        assert_eq!(
            parsed,
            StableTag {
                tag: "v2026.3.12".into(),
                year: 2026,
                major: 3,
                minor: 12,
                legacy_patch: 0,
            }
        );
    }

    #[test]
    fn parses_legacy_patch_stable_tags() {
        let parsed = parse_stable_tag("v2026.3.13-1").expect("legacy stable tag");
        assert_eq!(parsed.legacy_patch, 1);
        assert_eq!(parsed.tag, "v2026.3.13-1");
    }

    #[test]
    fn rejects_beta_tags() {
        assert!(parse_stable_tag("v2026.3.13-beta.1").is_none());
        assert!(parse_stable_tag("v2026.3.13.beta.1").is_none());
    }

    #[test]
    fn orders_legacy_patch_after_plain_release() {
        let plain = parse_stable_tag("v2026.3.13").expect("plain");
        let patched = parse_stable_tag("v2026.3.13-1").expect("patched");
        assert!(patched > plain);
    }
}
