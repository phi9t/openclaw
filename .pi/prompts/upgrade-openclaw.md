---
description: Upgrade containerized OpenClaw through clawyer only and validate the full runtime afterward
---

Input

- Ref: $1 <optional tag|branch|sha>
  - If missing: use the latest stable upstream release.

Do (end-to-end)
Goal: upgrade this containerized OpenClaw checkout through `clawyer` only, preserve only `crates/clawyer/**` plus custom skill directories, and run the full post-upgrade validation matrix. Do NOT use host `openclaw`. Do NOT use direct `docker compose`.

0. Load the `clawyer-upgrade-openclaw` skill and read `skills/clawyer-upgrade-openclaw/references/verification.md` before changing anything.

1. Preflight the current runtime:

   ```sh
   clawyer status
   clawyer health
   ```

2. Run the upgrade preview:

   ```sh
   clawyer upgrade --dry-run ${1:+--ref "$1"}
   ```

   - Verify `targetTag=` resolves to the intended release.
   - Verify `preserve:` contains only `crates/clawyer` and custom skills that are not upstream-owned.
   - If `drop:` contains unexpected local work, stop and report it. Do not continue unless the operator explicitly wants upstream to replace those paths.

3. If the repo is already in a merge or rebase conflict:
   - Run `clawyer repair-conflicts`
   - Then rerun the same dry-run and verify the preserve/drop report again before continuing.

4. Execute the upgrade:

   ```sh
   clawyer upgrade ${1:+--ref "$1"}
   ```

   - Only use `--drop-nonpreserved` when the operator explicitly requested destructive upstream replacement of unrelated local work.

5. Run the full validation matrix from `skills/clawyer-upgrade-openclaw/references/verification.md`.
   - Include baseline container health
   - Include post-upgrade version and update status
   - Include `agents list --json`
   - Include Discord probe validation
   - Include read-only Notion validation
   - Include the live agent check as the final gate

6. If any validation step fails:
   - capture the exact command
   - capture the exit code
   - capture one decisive output or log snippet
   - classify the failure as one of:
     - `clawyer` wrapper bug
     - repo-state issue
     - runtime config issue
     - external/provider issue
   - stop after identifying the first decisive blocker

7. Output a concise structured report with:
   - target ref used
   - dry-run preserve/drop summary
   - whether the upgrade ran
   - validation results for health, Discord, Notion, and agents
   - the first blocker, if any

Rules / Guardrails

- Use `clawyer` only.
- Prefer bounded commands with `timeout` for live validation.
- Do not mark the upgrade healthy unless the full validation matrix passes.
- Do not infer agent health from Discord or Notion success.
