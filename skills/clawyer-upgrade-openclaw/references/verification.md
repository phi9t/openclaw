# Verification Matrix

Use these checks after `clawyer upgrade`, and also as a baseline before the upgrade when you need to compare pre- and post-upgrade behavior.

Keep every check bounded with `timeout`. A command is only green when it exits `0` and the output matches the expected runtime signal.

## 1. Baseline Container Health

Run these first:

```bash
clawyer status
clawyer health
timeout 30s clawyer run -- --version
timeout 30s clawyer run -- update status --json
```

Expected signals:

- `clawyer status` shows `openclaw-gateway` running
- `clawyer health` reports the gateway as live
- `clawyer run -- --version` prints the active OpenClaw version
- `update status --json` returns valid JSON with the expected channel and registry state

Failure classification:

- command shape or container lifecycle is wrong: `clawyer` wrapper bug
- gateway container is missing or unhealthy: container/runtime issue
- version is not the intended release after upgrade: rebuild/restart or image selection issue

## 2. Dry-Run Validation

Before the real upgrade, run:

```bash
clawyer upgrade --dry-run
```

Expected signals:

- `targetTag=` is the latest stable upstream release
- `branch=` is a `local/release-*` branch
- `preserve:` contains only `crates/clawyer` and custom skill directories not present upstream
- `drop:` contains only paths that should intentionally follow upstream

Stop conditions:

- the tag is not the latest stable release
- the preserve list includes upstream-owned paths
- the drop list contains unexpected local work

## 3. Post-Upgrade Runtime Checks

After `clawyer upgrade`, rerun:

```bash
clawyer status
clawyer health
timeout 30s clawyer run -- --version
timeout 30s clawyer run -- update status --json
timeout 30s clawyer run -- agents list --json
```

Expected signals:

- the gateway is healthy
- the reported version matches the target release
- `update status --json` is coherent for the new version
- `agents list --json` returns valid JSON and the expected agents

Failure classification:

- command exits non-zero before reaching the gateway: `clawyer` wrapper or container-launch issue
- malformed JSON or missing agents: runtime config drift

## 4. Discord Validation

Run:

```bash
timeout 60s clawyer run -- channels status --probe
```

Expected signals:

- exit `0`
- each intended Discord account reports `works`

Interpretation:

- `disconnected, works` is a probe success with reconnect churn, not a hard auth failure
- missing `works`, auth errors, or probe exit failure means the Discord runtime is not healthy

If the probe is not green, collect logs:

```bash
clawyer exec -- sh -lc 'tail -n 200 /tmp/openclaw/*.log'
```

Look for:

- repeated reconnect loops
- token/auth failures
- permission or guild/channel allowlist problems

## 5. Notion Validation

Keep this read-only. First verify env injection:

```bash
clawyer exec -- node -e 'console.log(process.env.NOTION_API_KEY ? "notion-env-present" : "notion-env-missing")'
```

Then verify `users/me`:

```bash
clawyer exec -- node -e '(async()=>{const key=process.env.NOTION_API_KEY;if(!key){console.error("NOTION_API_KEY missing");process.exit(1)}const r=await fetch("https://api.notion.com/v1/users/me",{headers:{Authorization:`Bearer ${key}`,"Notion-Version":"2022-06-28"}});const data=await r.json();console.log(JSON.stringify({status:r.status,object:data.object,type:data.type??null},null,2));if(!r.ok)process.exit(1)})().catch(err=>{console.error(err);process.exit(1)})'
```

Then verify one read-only search:

```bash
clawyer exec -- node -e '(async()=>{const key=process.env.NOTION_API_KEY;if(!key){console.error("NOTION_API_KEY missing");process.exit(1)}const r=await fetch("https://api.notion.com/v1/search",{method:"POST",headers:{Authorization:`Bearer ${key}`,"Notion-Version":"2022-06-28","Content-Type":"application/json"},body:JSON.stringify({page_size:1})});const data=await r.json();console.log(JSON.stringify({status:r.status,object:data.object,resultCount:Array.isArray(data.results)?data.results.length:null},null,2));if(!r.ok)process.exit(1)})().catch(err=>{console.error(err);process.exit(1)})'
```

Failure classification:

- `notion-env-missing`: env injection or restart issue
- HTTP `401` or `403`: Notion auth/config issue
- network or TLS failure: container/network issue

## 6. Agent Validation

This is the final gate because it exercises the deepest runtime path.

Run:

```bash
timeout 120s clawyer run -- agent --agent main --message 'Reply with exactly OK.'
```

Expected signals:

- exit `0`
- a normal assistant reply appears

If the command hangs, times out, or exits non-zero, inspect logs:

```bash
clawyer exec -- sh -lc 'tail -n 200 /tmp/openclaw/*.log'
```

Classify the first decisive blocker:

- `refresh_token_reused` or other OAuth refresh errors: provider auth issue
- missing `skills/.../SKILL.md`: workspace drift or stale skill reference
- model fallback loops or provider refusal: runtime model/profile issue
- command/container launch failure before agent execution: `clawyer` wrapper issue

Do not mark agents healthy unless this check completes successfully.

## 7. Evidence Capture

For any failed validation step, record:

- the exact command
- the exit code
- one decisive output or log snippet
- the failure class: wrapper bug, repo-state issue, runtime config issue, or external/provider issue

Use that classification before making any follow-up change.
