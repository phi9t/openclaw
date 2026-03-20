# OpenClaw Local Container Validation and Remediation Report

Date (UTC): 2026-03-01
Repo revision: `ee10bbdc7ca4d19c0d51fbe1329d9929277a3476` on `main`
Scope: Docker + `clawyer` local container launch/verification path

## Environment

- Docker: 28.3.2
- Docker Compose: v2.38.2
- Cargo: 1.92.0
- Rustc: 1.92.0
- Node: v24.13.0
- pnpm: 10.23.0

## What Was Reviewed

- `crates/clawyer/src/compose.rs`
- `crates/clawyer/src/commands/mod.rs`
- `crates/clawyer/src/cli.rs`
- `docker-compose.yml`
- `docker-compose.extra.yml`
- `scripts/docker-verify-local.sh`
- `docs/install/docker.md`

## Observations (Pre-fix)

1. Health check false-negative right after `start`

- Repro: `clawyer start` followed immediately by `clawyer health`.
- Evidence: `.artifacts/logs/clawyer-extended-20260301T060823Z.log` contains:
  - `Error: gateway closed (1006 abnormal closure (no close frame))`
  - `Error: Health check failed`
- Root cause: `health` performed a single immediate check while gateway startup/channel init was still in progress.

2. Dashboard URL parsing failed despite successful dashboard output

- Repro: `clawyer dashboard`.
- Evidence: `.artifacts/logs/clawyer-extended-postfix-20260301T061011Z.log` contains:
  - `Error: Failed to get dashboard URL`
- Root cause: parser only accepted lines starting with `http://` or `https://`, but actual CLI output format is `Dashboard URL: http://...`.

3. `clawyer` command execution did not consistently include `docker-compose.extra.yml`

- Evidence (code review): `Compose::run` / `run_streamed` ignored `Compose::args()` and `health` hardcoded only `-f docker-compose.yml`.
- Impact: behavior drift between commands; extra mounts/env from generated `docker-compose.extra.yml` could be ignored by `start/status/health/rebuild/clean` paths.

## Fixes Applied

1. Compose argument handling unified

- File: `crates/clawyer/src/compose.rs`
- Changes:
  - `Compose::run` now executes `docker compose` with merged compose args from `Compose::args()`.
  - `Compose::run_streamed` now also includes merged compose args.
  - Removed unused `run_merged` path and added coverage for extra compose file detection.

2. Health command hardened for startup timing

- File: `crates/clawyer/src/commands/mod.rs`
- Changes:
  - `health` now uses merged compose args (includes extra compose file when present).
  - Added retry loop: 12 attempts with 3s delay before final failure.
  - Preserves final stdout/stderr on failure for troubleshooting.

3. Dashboard URL extraction made robust

- File: `crates/clawyer/src/commands/mod.rs`
- Changes:
  - Added URL extraction helper that finds URL tokens anywhere in output line (stdout/stderr), including prefixed lines like `Dashboard URL: ...`.
  - Added unit tests for plain and prefixed URL formats.

4. Status command now surfaces compose execution failure

- File: `crates/clawyer/src/commands/mod.rs`
- Changes:
  - `status` now returns error when `docker compose ps` exits non-zero.

## Verification

### Unit tests

- Command: `cargo test --manifest-path crates/clawyer/Cargo.toml`
- Result: PASS (14 tests)
- Includes new tests:
  - `commands::tests::extract_first_url_accepts_plain_url_line`
  - `commands::tests::extract_first_url_accepts_prefixed_line`
  - `compose::tests::test_compose_args_with_extra_file`

### Extended `clawyer` validation (post-fix)

- Log: `.artifacts/logs/clawyer-extended-postfix2-20260301T061200Z.log`
- Sequence executed:
  - `clawyer start`
  - `clawyer status`
  - `clawyer health`
  - `clawyer logs --tail 80`
  - `clawyer dashboard`
  - `clawyer devices`
- Result: PASS
- Evidence highlights:
  - `✅ Started gateway`
  - `Discord: ok (@openclaw)` from health output
  - Dashboard URL successfully printed
  - Device table listed (`Paired (1)`)

### Repo Docker verification script

- Command: `scripts/docker-verify-local.sh`
- Log: `.artifacts/logs/docker-verify-local-20260301T061218Z.log`
- Result: PASS (`==> Docker verification passed`)

## Residual Notes

- Compose emits environment warnings for unset `CLAUDE_*` variables in this local setup; these were non-blocking.
- Gateway logs include `tailscale ... ENOENT` in this environment; also non-blocking for local container validation.
