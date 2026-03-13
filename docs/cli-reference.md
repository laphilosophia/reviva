# CLI Reference

This document is the authoritative command surface for `reviva`.

## Command Index

```text
reviva init [--repo PATH] [--no-scan] [--rewrite-config]
reviva scan [--repo PATH]
reviva review --repo PATH [--mode MODE] [--profile NAME] [--profile-file PATH] [--max-findings N] [--max-output-tokens N] [--file PATH]... [--boundary-left PATH --boundary-right PATH] [--incremental-from GIT_REF] [--note TEXT] [--prompt-wrapper plain|chatml] [--kv-cache on|off] [--kv-slot SLOT_ID] [--llama-lifecycle manual|ensure-running|ensure-running-and-stop] [--preview-only] [--llama-model-path PATH_OR_DIR] [--llama-server-path PATH]
reviva set save --repo PATH --name NAME --file PATH...
reviva set load --repo PATH --name NAME
reviva set list --repo PATH
reviva session list --repo PATH
reviva session show --repo PATH --id SESSION_ID
reviva findings list --repo PATH [--session SESSION_ID] [--triage]
reviva export --repo PATH --session SESSION_ID [--format md|json] [--output PATH]
```

## `reviva init`

Initialize `.reviva/` state for a repository.

- `--repo PATH`: repository root
- `--no-scan`: skip initial repo scan/map
- `--rewrite-config`: rewrite config with current schema/default fields

Notes:

- Creates `.reviva/config.toml` if missing.
- Creates derived indexes for findings and sets.
- Path-like config fields are normalized to absolute values when possible.

## `reviva scan`

Traverse repository and emit reviewable file candidates.

- `--repo PATH`: repository root

Outputs:

- per-file scan line
- `.reviva/repo-map.json` snapshot

`include` and `exclude` from config are applied.

## `reviva review`

Run a constrained review for explicit target(s).

### Target options

- `--file PATH` (repeatable)
- `--boundary-left PATH --boundary-right PATH`
- `--incremental-from GIT_REF`

Rules:

- `--incremental-from` cannot be combined with file/boundary options.
- In non-TTY shells, one explicit target form is required.

### Review controls

- `--mode MODE`
- `--profile NAME`
- `--profile-file PATH`
- `--note TEXT`
- `--max-findings N`
- `--max-output-tokens N`
- `--preview-only`

### Prompt transport controls

- `--prompt-wrapper chatml|plain`

`prompt_wrapper` meaning:

- `chatml`: wraps prompt into ChatML-style system/user format.
- `plain`: sends raw prompt without wrapper.

Use `plain` only for backends/models that explicitly need raw completion text.

### llama-server controls

- `--llama-lifecycle manual|ensure-running|ensure-running-and-stop`
- `--llama-model-path PATH_OR_DIR`
- `--llama-server-path PATH`
- `--kv-cache on|off`
- `--kv-slot SLOT_ID`

Local management applies only when backend URL is local (`http://127.0.0.1:8080` or `http://localhost:8080`).

## `reviva set`

Save/load/list named target sets.

### `reviva set save`

- `--repo PATH`
- `--name NAME`
- `--file PATH` (repeatable)

### `reviva set load`

- `--repo PATH`
- `--name NAME`

Prints set paths line-by-line.

### `reviva set list`

- `--repo PATH`

Prints saved set names with path counts.

## `reviva session`

### `reviva session list`

- `--repo PATH`

Lists session summaries.

### `reviva session show`

- `--repo PATH`
- `--id SESSION_ID`

Shows normalized finding counts, interpretation summary, warnings, and incremental metadata.

## `reviva findings list`

List findings, optionally filtered by session.

- `--repo PATH`
- `--session SESSION_ID` (optional)
- `--triage` (optional)

Normalization states:

- `structured`
- `partial`
- `raw_only`

## `reviva export`

Export one session as Markdown or JSON.

- `--repo PATH`
- `--session SESSION_ID`
- `--format md|json` (default `md`)
- `--output PATH` (optional)

Without `--output`, file is written under `.reviva/exports/`.

## Common Errors

- No explicit target in non-TTY shell: provide `--file`, boundary flags, or `--incremental-from`.
- Target excluded by config/ignore: update `include`/`exclude` or select another target.
- Prompt budget refusal: narrow target or shorten note.
- Missing llama model path in non-interactive run: pass `--llama-model-path` or set config.

