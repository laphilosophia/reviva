# Contributing to Reviva

Thanks for considering a contribution.

## Before You Start

- Read [README](README.md)
- Read [CLI References](docs/cli-reference.md)
- Read [Config References](docs/config-reference.md)

Reviva is intentionally a constrained, local-first review appliance. Please keep changes aligned with that scope.

## Development Setup

```bash
cargo build
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Contribution Rules

- Keep diffs focused and minimal.
- Do not introduce agent-style/autonomous behavior unless explicitly requested.
- Preserve explicit prompt inspectability and raw output persistence.
- Keep CLI as the primary product surface.
- Update docs for any user-facing behavior changes.

## Pull Request Checklist

- Code builds and tests pass locally.
- Formatting and clippy checks pass.
- Added/updated tests for behavior changes.
- Updated relevant docs.
- PR description explains why the change is needed.

## Commit Style

Use clear, imperative commit messages (for example: `add repo map include/exclude normalization`).
