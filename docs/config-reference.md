# Config Reference

`reviva init` creates `.reviva/config.toml` with defaults.

Example:

```toml
backend_url = "http://127.0.0.1:8080"
model = "local-model-name"
prompt_wrapper = "chatml"
llama_lifecycle_policy = "ensure-running-and-stop"
llama_kv_cache = false
llama_slot_id = 0
review_profile = "default"
review_profile_file = "path/to/review-profile.toml"
llama_server_path = "path/to/llama-server"
llama_model_path = "path/to/model.gguf"
timeout_ms = 60000
max_tokens = 2048
temperature = 0.1
stop_sequences = []
max_file_bytes = 262144
estimated_prompt_tokens = 16000
include = []
exclude = []
```

## Path Normalization

Reviva normalizes path-like fields during `init`, `scan`, and `review`.

- `review_profile_file`: persisted as absolute path
- `llama_model_path`: persisted as absolute path
- `llama_server_path`: persisted as absolute path when given as a path

Special case:

- If `llama_server_path` is a bare command name (for example `llama-server`), it stays as-is to allow PATH lookup.

## Field Reference

| Field | Type | Default | Description |
| --- | --- | ---: | --- |
| `backend_url` | string | `http://127.0.0.1:8080` | Completion backend base URL. |
| `model` | string? | `null` | Optional model name sent to backend. |
| `prompt_wrapper` | `chatml` \| `plain` | `chatml` | Prompt packaging strategy before backend call. |
| `llama_lifecycle_policy` | `manual` \| `ensure-running` \| `ensure-running-and-stop` | `ensure-running-and-stop` | `llama-server` process policy for local backend. |
| `llama_kv_cache` | bool? | `null` | Enables KV cache for llama-server when set. |
| `llama_slot_id` | u32? | `null` | Optional llama-server slot pinning. |
| `review_profile` | string? | `null` | Built-in review profile name. |
| `review_profile_file` | string? | `null` | File-based profile path. |
| `llama_server_path` | string? | `null` | Path to `llama-server` binary or bare command name. |
| `llama_model_path` | string? | `null` | GGUF file path or model directory for local llama-server flow. |
| `timeout_ms` | u64 | `60000` | Backend request timeout (milliseconds). |
| `max_tokens` | u32 | `2048` | Backend max generation token cap. |
| `temperature` | f32 | `0.1` | Backend generation temperature. |
| `stop_sequences` | string[] | `[]` | Backend stop sequence list. |
| `max_file_bytes` | usize | `262144` | Per-file scan/review byte limit (default 256 KiB). |
| `estimated_prompt_tokens` | usize | `16000` | Prompt budget estimate threshold. |
| `include` | string[] | `[]` | Allow-list path patterns. If non-empty, only matching paths are reviewable. |
| `exclude` | string[] | `[]` | Deny-list path patterns removed from scan and explicit review. |

## Include / Exclude Semantics

Evaluation order for each candidate path:

1. `include` check (if non-empty)
2. `exclude` check
3. local ignore patterns (`.gitignore` and repo-local ignores)
4. built-in auto exclusions (`.git/`, `.github/`, `.reviva/`, `node_modules/`, `.env*`, lockfiles, common key/cert files, and selected non-code metadata files)

If a path is excluded, explicit `review --file ...` also fails fast with a plain error.

## Prompt Wrapper Guidance

Use `chatml` unless you have a backend/model-specific reason.

- `chatml` is safer for most instruction-tuned local models.
- `plain` can be useful for raw completion-style setups, but may degrade output quality if your model expects chat formatting.

## Practical Advice

- Keep `include` narrow in large monorepos.
- Keep `exclude` explicit for generated/vendor paths.
- Treat `estimated_prompt_tokens` as conservative estimate, not tokenizer truth.
- Keep `model` optional when backend resolves model server-side.
