---
name: clawyer-upgrade-openclaw
description: Upgrade containerized OpenClaw through clawyer only. Use when OpenClaw runs only in Docker Compose, you must avoid direct host openclaw/docker compose commands, you need to sync to the latest upstream stable release, you need upstream-first conflict repair while preserving only crates/clawyer plus custom skills, or you need post-upgrade validation for agents, Discord, and Notion.
---

# Clawyer Upgrade OpenClaw

Use this skill only for the containerized OpenClaw deployment in this repo.

## Rules

- Use `clawyer` as the only operator-facing interface.
- Do not run host `openclaw` directly.
- Do not tell the operator to use `docker compose` directly.
- Keep local divergence minimal:
  - preserve `crates/clawyer/**`
  - preserve only custom skill directories under `skills/`
  - upstream wins for everything else

## Preflight

1. Run `clawyer status`.
2. Run `clawyer health`.
3. Run `clawyer upgrade --dry-run`.
4. Review the target tag, preserve list, and drop list before changing anything.
5. If the drop list contains anything unexpected, stop until the operator confirms that those paths should be replaced by upstream.

## Upgrade Flow

1. Confirm the dry-run resolves the latest stable upstream release and creates the expected `local/release-*` branch.
2. Confirm the preserve list contains only `crates/clawyer/**` and custom skill directories that are not upstream-owned.
3. Run `clawyer upgrade`.
4. Add `--drop-nonpreserved` only when you explicitly want upstream to replace unrelated local repo changes.
5. After the upgrade completes, run the validation sequence in `references/verification.md`.

## Conflict Repair

If the repo is already in a merge or rebase conflict:

1. Run `clawyer repair-conflicts`.
2. Let clawyer prefer upstream for non-preserved paths.
3. Let clawyer prefer local for `crates/clawyer/**` and custom skill directories.
4. If a preserved path still needs semantic editing, stop and fix only that path.
5. Rerun `clawyer upgrade --dry-run` before resuming the upgrade.

## Verification

- Use clawyer only.
- Use bounded commands with `timeout` for live checks so validation cannot hang indefinitely.
- Treat a check as successful only when both the exit code and the runtime output match expectations.
- Use the detailed validation matrix in `references/verification.md`.

## Notes

- `clawyer run` is for OpenClaw CLI commands in the one-shot CLI container.
- `clawyer exec` is for arbitrary commands in the running gateway container.
- `clawyer rebuild` and `clawyer restart` must be truthful; if they fail, fix clawyer instead of bypassing it with direct compose commands.
- If validation fails, collect one decisive log or output snippet and classify the failure before changing anything else.
- For a repo-local end-to-end launcher, use `.pi/prompts/upgrade-openclaw.md`.
