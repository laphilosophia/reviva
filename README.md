# Reviva

Reviva is a local-first review terminal for deterministic, inspectable, and constrained repository analysis with local LLM backends.

It is intentionally narrow:

- scan repository files
- select an explicit review target
- build a visible prompt for a specific review mode
- send to a local completion backend
- preserve raw response
- persist normalized findings and session artifacts

It is not a chatbot, IDE copilot, or autonomous coding agent.

## Why Reviva

Many local review flows become opaque once hidden prompt templates, chat abstractions, and agent loops are introduced. Reviva keeps the review path explicit and auditable.

Core principles:

- local-first behavior
- explicit prompt preview
- explicit backend URL/model
- deterministic target selection
- plain error reporting
- human-inspectable storage

## What You Can Do

- `scan` a repo with conservative filtering
- `review` a file, file set, or boundary pair
- use built-in or file-based review profiles
- inspect prompt before execution
- reuse exact prompt/backend hits via local review cache
- list and inspect sessions/findings
- export session output to Markdown or JSON

Supported review modes:

```text
contract
boundary
boundedness
failure-semantics
performance-risk
memory-risk
operator-correctness
launch-readiness
maintainability
```

## Mode vs Profile

Reviva separates two concepts:

- `mode`: coarse review lens (contract, boundary, launch-readiness, etc.)
- `profile`: review policy (rules, vocabulary, confidence/severity scales, risk classes)

`mode` is optional. Resolution order:

1. `--mode` (explicit)
2. profile name if it matches a mode
3. first profile focus token that matches a mode
4. fallback: `contract`

This keeps fast presets available while allowing profile-driven control.

## Requirements

- Rust toolchain (stable)
- local inference backend reachable via HTTP completion endpoint
- optional: `llama-server` in `PATH` for local auto-start flow

## Build

```bash
cargo build
```

Run directly from workspace:

```bash
cargo run -p reviva-cli -- --help
```

If you install the binary, command name is `reviva`.

## Quick Start

1. Scan a repo:

```bash
reviva scan --repo /path/to/repo
```

1. Run a focused review:

```bash
reviva review \
  --repo /path/to/repo \
  --mode launch-readiness \
  --file src/main.rs
```

1. Inspect results:

```bash
reviva session list --repo /path/to/repo
reviva session show --repo /path/to/repo --id <SESSION_ID>
reviva findings list --repo /path/to/repo --session <SESSION_ID>
```

1. Export:

```bash
reviva export --repo /path/to/repo --session <SESSION_ID> --format md
reviva export --repo /path/to/repo --session <SESSION_ID> --format json
```

## CLI Surface

```text
reviva scan [--repo PATH]
reviva review --repo PATH [--mode MODE] [--profile NAME] [--profile-file PATH] [--max-findings N] [--max-output-tokens N] [--file PATH]... [--boundary-left PATH --boundary-right PATH] [--incremental-from GIT_REF] [--note TEXT] [--prompt-wrapper plain|qwen-chatml] [--kv-cache on|off] [--kv-slot SLOT_ID] [--llama-lifecycle manual|ensure-running|ensure-running-and-stop] [--preview-only] [--llama-model-path PATH_OR_DIR] [--llama-server-path PATH]
reviva set save --repo PATH --name NAME --file PATH...
reviva set load --repo PATH --name NAME
reviva set list --repo PATH
reviva session list --repo PATH
reviva session show --repo PATH --id SESSION_ID
reviva findings list --repo PATH [--session SESSION_ID] [--triage]
reviva export --repo PATH --session SESSION_ID [--format md|json] [--output PATH]
```

## Configuration

Reviva reads `.reviva/config.toml` from the target repository root.

Example:

```toml
backend_url = "http://127.0.0.1:8080"
model = "local-model-name"
prompt_wrapper = "plain"
llama_lifecycle_policy = "ensure-running-and-stop"
llama_kv_cache = true
llama_slot_id = 0
llama_model_path = "path/to/models/my-model/model.gguf" # Windows -> "path\\to\\models\\my-model\\model.gguf"
llama_server_path = "llama-server"
timeout_ms = 60000
max_tokens = 2048
temperature = 0.1
stop_sequences = []
max_file_bytes = 262144
estimated_prompt_tokens = 16000
```

Notes:

- `prompt_wrapper` defaults to `plain` if omitted.
- Use `qwen-chatml` only for backends/models that expect ChatML-style prompting.
- `mode` is optional; if omitted, Reviva derives it from profile name/focus then falls back to `contract`.
- `--max-findings` and `--max-output-tokens` override profile limits for the current run only.
- `llama_lifecycle_policy` defaults to `ensure-running-and-stop` if omitted.
- `llama_kv_cache` defaults to `false` if omitted.
- `llama_slot_id` is optional; set it to pin repeated reviews to a stable llama-server slot.
- `max_file_bytes` and `estimated_prompt_tokens` are conservative defaults, not hard domain invariants.

## Target Selection Behavior

`review` target resolution order:

1. `--boundary-left` + `--boundary-right`
2. one or more `--file`
3. `--incremental-from <GIT_REF>` (changed files from `git diff`)
4. interactive selection (TTY only)

In non-interactive environments, Reviva fails fast if no explicit target is provided.

Boundary mode enforces deterministic ordering: `left -> right`.

`--incremental-from` cannot be combined with explicit `--file` or boundary flags.
In incremental mode, Reviva sends diff hunks (`git diff --unified=3`) instead of full-file content.
If a file has no diff payload, Reviva falls back to full-file content and records explicit warnings.

## llama-server Integration

When backend is `http://127.0.0.1:8080` or `http://localhost:8080`, Reviva manages `llama-server` explicitly:

- if server is active, Reviva reuses it
- if server is inactive, Reviva starts it when lifecycle policy is `ensure-running` or `ensure-running-and-stop`
- if lifecycle policy is `ensure-running`, Reviva leaves the started server running
- if lifecycle policy is `ensure-running-and-stop`, Reviva stops the started server when command exits
- if lifecycle policy is `manual`, Reviva does not start/stop server processes
- if `llama-server` binary is missing, Reviva returns explicit install guidance
- if `llama-server` is not invokable from `PATH`, Reviva tries common local fallbacks (including WinGet install path on Windows) before failing
- if model path is missing in non-interactive mode, Reviva fails with a clear error
- KV cache can be enabled with `--kv-cache on` (or `llama_kv_cache=true`) and optionally pinned via `--kv-slot` / `llama_slot_id`.

## Output and Persistence

Reviva persists data under:

```text
.reviva/
  config.toml
  sessions/
  findings/
  sets/
  exports/
```

Session is the canonical truth source. Raw backend response is always preserved.
`findings/` and `sets/` also keep derived index surfaces for quick inspection (`index.json`), while canonical state remains in sessions.

Reviva also keeps a derived local review cache (`.reviva/cache/review-cache.json`) keyed by prompt+backend identity:

- cache hit: backend call is skipped and cached raw response is reused
- cache miss: backend is called and cache is updated after session save

Finding normalization state is explicit:

- `structured`
- `partial`
- `raw_only`

If normalization is partial/raw-only, warnings are stored with reason tags.
Markdown/JSON exports prioritize parsed response interpretation for readability; full raw backend body remains in session files.
Exports now keep prompt/body readability by default:

- Markdown export shows prompt metadata (hash/line/char counts) instead of dumping full prompt text.
- JSON export includes prompt metadata and parsed response shape, while full prompt/raw bodies stay in canonical session JSON.

Profile TOML supports optional limits:

```toml
name = "strict-launch"
goal = "Conservative launch review"
severity_scale = ["release-blocker", "pre-launch-fix", "post-launch-watch"]
confidence_scale = ["definite", "likely", "uncertain"]
risk_classes = ["correctness", "security", "operator-trust"]

[limits]
max_findings = 8
max_output_tokens = 768
```

## Troubleshooting

- `normalization_state=raw_only`: inspect `session show` warnings and raw response body.
- backend timeouts/unreachable: verify `backend_url`, server status, and timeout settings.
- prompt budget refusal: narrow target selection or shorten `--note`.
- empty findings with non-empty response: verify output contract adherence and wrapper choice.
- repeated review unexpectedly misses cache: check `review_cache_key` and backend/profile/mode/prompt changes in session warnings.

## Development

Run key tests:

```bash
cargo test -p reviva-prompts -p reviva-cli -p reviva-storage -p reviva-export
```

Format:

```bash
cargo fmt
```

## License

Apache-2.0
