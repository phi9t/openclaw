## <!-- 3004bfe7-ecb9-49e9-b825-ee3f3e257aaf -->

todos:

- id: "add-cli-backends"
  content: "Add built-in CliBackendConfig defaults for gemini-cli, agent-cli, and opencode-cli in src/agents/cli-backends.ts and wire them into resolveCliBackendIds/normalizeProviderId."
  status: pending
- id: "document-global-priority"
  content: "Document global CLI priority configuration (codex-cli → gemini-cli → agent-cli → opencode-cli) in docs/gateway/cli-backends.md and configuration-reference.md, including a full json5 snippet for agents.defaults.model and cliBackends."
  status: pending
- id: "implement-cli-health"
  content: "Implement lightweight in-memory health/cooldown tracking for CLI backends based on FailoverError reasons so rate-limited CLIs are temporarily skipped during model failover."
  status: pending
- id: "add-tests"
  content: "Extend cli-backends, cli-runner, and live gateway CLI backend tests to cover the new CLIs and their configuration/behavior."
  status: pending
  isProject: false

---

# Multi-CLI backends for agents

## High-level design

- **Goal**: Enable OpenClaw agents to use multiple local AI CLIs (`codex`, `gemini`, `agent`, `opencode`) as text-only backends, with a single global gateway-wide configuration and a fixed priority order: codex → gemini → agent → opencode.
- **Approach**:
  - Extend existing CLI backend machinery (`agents.defaults.cliBackends`) to ship **built-in defaults** for the new CLIs, similar to `claude-cli` and `codex-cli`.
  - Use the existing **model fallback chain** (`agents.defaults.model.{primary,fallbacks}`) to express the global priority order between CLI providers.
  - Rely on the existing **FailoverError** pipeline for timeouts / rate limits, so the agent run automatically falls back to the next configured CLI model on errors.
  - Optionally add light **runtime health hints** so obviously broken CLIs can be temporarily skipped or deprioritized.

```mermaid
flowchart TD
  agentRun[AgentRun] --> modelSelect[ModelSelection]
  modelSelect --> providerCheck{Provider
is CLI?}
  providerCheck -- "no" --> apiPath[API Provider]
  providerCheck -- "yes" --> cliBackend[CLI Backend
resolveCliBackendConfig]
  cliBackend --> cliExec[runCliAgent
(spawn CLI)]
  cliExec --> success[Text reply]
  cliExec --> fail[FailoverError]
  fail --> fallbackModel[Next fallback
model]
  fallbackModel --> modelSelect
```

## Steps

### 1. Add built-in CLI backends for new CLIs

- **Files**:
  - `src/agents/cli-backends.ts`
  - `src/agents/model-selection.ts`
- **Changes**:
  - Define new `CliBackendConfig` constants for:
    - `DEFAULT_AGENT_BACKEND` (for an `agent` CLI binary).
    - `DEFAULT_OPENCODE_BACKEND` (for an `opencode` CLI binary).
    - `DEFAULT_GEMINI_BACKEND` (for a `gemini` CLI binary).
  - Choose provider ids and normalization rules so that:
    - Model refs look like `agent-cli/<model>`, `opencode-cli/<model>`, `gemini-cli/<model>`.
    - Bare names `agent`, `opencode`, `gemini` in config normalize to the corresponding `*-cli` ids via `normalizeProviderId`.
  - For each backend, configure:
    - `command` (binary name, e.g. `agent`, `opencode`, `gemini`).
    - `args` and `resumeArgs` to match each CLI’s JSON / session interface (initially conservative, based on their documented defaults; overridable via `agents.defaults.cliBackends`).
    - `output` / `resumeOutput` (`json`, `jsonl`, or `text`) and `sessionMode` matching each CLI’s semantics.
    - Optional `imageArg` / `imageMode` if the CLI supports image file paths.
    - Reasonable `reliability.watchdog` defaults (reusing `CLI_FRESH_WATCHDOG_DEFAULTS` / `CLI_RESUME_WATCHDOG_DEFAULTS`).
  - Update `resolveCliBackendIds` to include the new normalized provider ids so they show up in status / tooling.

### 2. Wire provider normalization and CLI detection

- **Files**:
  - `src/agents/model-selection.ts`
- **Changes**:
  - Extend `normalizeProviderId` to treat any legacy / alias forms (if relevant) as:
    - `agent-cli`
    - `opencode-cli`
    - `gemini-cli`
  - Ensure `isCliProvider` returns `true` for these providers so they route through `runCliAgent` instead of the HTTP API path.

### 3. Global configuration pattern for CLI priority chain

- **Files**:
  - `docs/gateway/cli-backends.md`
  - `docs/gateway/configuration-reference.md`
  - `src/config/schema.help.ts` (keep help text aligned)
- **Changes**:
  - Document a **single global** model configuration that encodes the requested priority order using model fallbacks:
    - Example snippet under `agents.defaults`:
      ```json5
      agents: {
        defaults: {
          model: {
            primary: "codex-cli/gpt-5.4",
            fallbacks: [
              "gemini-cli/gemini-2.0-pro",
              "agent-cli/default",
              "opencode-cli/default",
            ],
          },
          models: {
            "codex-cli/gpt-5.4": {},
            "gemini-cli/gemini-2.0-pro": {},
            "agent-cli/default": {},
            "opencode-cli/default": {},
          },
          cliBackends: {
            "codex-cli": { command: "codex" },
            "gemini-cli": { command: "gemini" },
            "agent-cli": { command: "agent" },
            "opencode-cli": { command: "opencode" },
          },
        },
      }
      ```
  - Explicitly call out that:
    - This configuration is **global for all agents** unless an individual agent overrides its model.
    - The runtime will automatically fall back from `codex-cli` to `gemini-cli` → `agent-cli` → `opencode-cli` when a provider fails with a `FailoverError` (timeout, auth, rate_limit, etc.).

### 4. Light usage/limit monitoring and health hints

- **Files**:
  - `src/agents/cli-runner.ts`
  - `src/agents/failover-error.ts`
  - (Optionally) a new `src/agents/cli-health.ts`
- **Changes**:
  - Reuse existing `FailoverError` + `classifyFailoverReason` to detect rate limits / billing issues coming out of CLI runs (e.g. CLI stdout/stderr contains upstream quota messages).
  - Introduce a minimal in-memory health tracker for CLI backends:
    - Track last failure time and reason per backend id (e.g. `rate_limit`, `billing`, `timeout`).
    - For hard quota reasons (`rate_limit`, `billing`), apply a short **cooldown window** (e.g. 60–300s) where that backend is treated as “degraded”.
  - In the agent model failover path (where FailoverError is handled), consult the health tracker when deciding the next model:
    - Skip immediately retrying a CLI backend that is in cooldown for quota reasons.
    - Once the cooldown elapses (or a manual successful run occurs), mark the backend healthy again so it can resume being first in the chain.
  - Keep this logic **stateless across restarts** (no persistence), so configuration remains the primary source of truth.

### 5. Tests

- **Files**:
  - `src/agents/cli-backends.test.ts`
  - `src/agents/cli-runner.test.ts`
  - `src/gateway/gateway-cli-backend.live.test.ts`
  - `docs/help/testing.md`
- **Changes**:
  - Unit tests for `resolveCliBackendConfig` for each new backend:
    - Merges defaults with overrides.
    - Respects `cliBackends` overrides in config.
  - Unit tests in `cli-runner.test.ts` to ensure:
    - New providers use correct args / output parsing modes.
    - FailoverError is thrown on non-zero exit / watchdog timeouts.
  - Optional live tests in `gateway-cli-backend.live.test.ts`:
    - Gated by provider-specific env vars (e.g., `OPENCLAW_LIVE_CLI_BACKEND_PROVIDER=gemini-cli`) so they only run when those CLIs are installed.
  - Update testing docs (`docs/help/testing.md`) with a short section describing how to run live CLI backend smoke tests for the new CLIs.

### 6. UX and observability

- **Files**:
  - `src/commands/status-all/agents.ts`
  - `src/gateway/agent-prompt.ts` or related status surfaces (if needed)
  - `docs/gateway/configuration-reference.md`
- **Changes**:
  - Ensure agent status output shows which CLI backend was used for the last run (provider/model), so it’s clear when the run fell back from `codex-cli` to a lower-priority CLI.
  - Optionally add a brief note to configuration docs explaining how CLI backend health and cooldown behave when rate limits are hit.
