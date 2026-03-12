# Reviva

Reviva is a local-first code review terminal for constrained, inspectable, and repeatable repository analysis with local LLMs.

It is not a coding agent, not an IDE copilot, and not a general-purpose chat interface. Its job is narrower: traverse a repository, let the user select files or boundaries, run focused review modes against a local model backend, and store findings in a form that can be revisited later.

The project exists because local review workflows are often degraded by opaque editor integrations, prompt-template drift, hidden tool-calling behavior, and fragile chat abstractions. Reviva removes that layer and keeps the review path explicit.

## Why this exists

A local model may be perfectly capable of reviewing code, yet still become unreliable once it is wrapped by editor extensions, integrated chats, agent loops, or hidden prompt orchestration. The result is familiar: malformed responses, self-generated questions, broken role formatting, unstable context behavior, or review output that cannot be trusted operationally.

Reviva addresses that by reducing the surface area.

The user selects the files.
The review mode is explicit.
The prompt is inspectable.
The backend is local.
The output is preserved.
The findings are stored.

That is the whole product.

## What it does

Reviva scans a repository, allows explicit target selection, builds a controlled prompt for a selected review mode, sends it to a local inference backend, displays the response, and records findings.

The initial focus is semantic review, not code generation.

Supported review modes are intended to stay narrow and operational:

- contract
- boundary
- boundedness
- failure-semantics
- performance-risk
- memory-risk
- operator-correctness
- launch-readiness
- maintainability

The model is treated as a constrained reviewer, not an author.

## What it is not

Reviva does not replace Semgrep, CodeQL, profilers, tests, or benchmarks.
Reviva does not autonomously plan work.
Reviva does not mutate repository files by default.
Reviva does not depend on cloud inference.
Reviva does not attempt to become a chatbot.

If a feature makes the tool feel like an agent, it is probably out of scope.

## Design goals

The project is built around a few hard constraints.

First, local-first operation. Repository contents should stay local unless the user explicitly configures a remote backend.

Second, prompt transparency. Prompt construction must be visible and inspectable.

Third, narrow review modes. Broad “review the repo” behavior is low-signal and unstable. Focused review modes are more useful.

Fourth, reproducibility. The same file set, review mode, and model settings should produce operationally comparable output.

Fifth, plain failure behavior. Empty responses, malformed output, timeouts, prompt oversize, and backend errors must be surfaced directly rather than hidden.

## High-level workflow

The expected workflow is simple:

1. Scan a repository.
2. Select one file, a file set, or a boundary pair.
3. Choose a review mode.
4. Preview the generated prompt.
5. Send the request to a local backend.
6. Inspect the raw response.
7. Extract and save findings.
8. Export the session if needed.

The system is intended for deliberate review sessions, not passive assistant chatter.

## Architecture

The project is split into a small number of clear layers:

- repository traversal and selection
- prompt construction
- backend transport
- response rendering
- findings extraction
- session and findings persistence

The UI must remain thin. Review logic belongs in the core, not in the terminal layer.

## Planned workspace layout

```text
review-bot/
├─ Cargo.toml
├─ crates/
│  ├─ reviva-core/
│  ├─ reviva-repo/
│  ├─ reviva-prompts/
│  ├─ reviva-backend/
│  ├─ reviva-storage/
│  ├─ reviva-export/
│  ├─ reviva-cli/
│  └─ reviva-tui/        # optional later
├─ fixtures/
│  └─ sample-repo/
├─ docs/
│  ├─ SPEC.md
│  └─ ARCHITECTURE.md
└─ README.md
```

The CLI is the primary v1 interface. A TUI may be added later, but it should remain a thin shell over the same core flows.

## Backend model

Reviva assumes an external local inference server.

The first target is a completion-style HTTP backend, such as `llama-server`, because completion-style requests give explicit control over prompt construction and avoid some of the fragility introduced by chat-template handling.

Chat-style backends may be supported later, but only if prompt behavior remains explicit and testable.

## Output philosophy

The first version prefers stable plain-text outputs with fixed sections over aggressive structured-output assumptions.

Raw model output must always be preserved.
If finding extraction fails, the raw output still remains part of the session record.
The system should not discard results merely because formatting drifted.

A finding is expected to carry at least:

- severity
- risk class
- location
- issue
- why it matters
- confidence
- action

## Storage model

Reviva stores its state locally in a human-inspectable format.

Suggested layout:

```text
.reviva/
├─ config.toml
├─ sets/
├─ sessions/
├─ findings/
└─ exports/
```

The exact file structure may evolve, but the principle will not: local, explicit, inspectable.

## Security and privacy

Reviva defaults to local-only behavior.
It must not execute repository code.
It must not include hidden telemetry.
It must clearly display which backend URL is active.
It must never silently send repository content elsewhere.

This is review infrastructure, not data collection.

## MVP scope

v1 is complete when all of the following are true:

- repository scanning works
- file selection works
- review modes are selectable
- prompt preview exists
- local backend requests work
- raw responses are displayed
- sessions can be saved
- findings can be extracted and stored
- exports can be generated as Markdown and JSON

That is enough to make the tool useful.

## Non-goals for v1

The following are intentionally out of scope for the first release:

- autonomous coding
- automatic patch generation
- semantic indexing
- background review workers
- hidden multi-step orchestration
- cloud-first features
- editor dependence
- full agent loops
- “fix everything” workflows

These may be explored later, but they are not part of the product definition.

## Current status

The project is in active design and implementation.

The immediate priority is a stable CLI-based review workflow:
repository scan, target selection, prompt building, local inference, response capture, and findings persistence.

## Intended users

Reviva is meant for developers who want local semantic review without surrendering control to opaque IDE assistant layers.

It is especially relevant when:

- local LLM review is useful
- editor integrations are unstable
- review sessions need to be repeatable
- findings need to be saved and revisited
- repository code should remain local
- the user wants a review appliance, not an agent stack

## Contributing

The project should remain narrow in scope.

Contributions are welcome, but changes that push the system toward autonomous agent behavior, hidden orchestration, or chat-centric UX will likely be rejected. The tool should stay small, explicit, and operationally trustworthy.

If you contribute, keep the following in mind:

- prefer explicit behavior over convenience
- preserve prompt inspectability
- keep review modes narrow
- do not hide backend behavior
- avoid feature creep into copilot territory

## Roadmap

Near-term priorities:

- workspace bootstrap
- repository traversal
- file selection
- review mode system
- prompt builder
- completion-style backend client
- session storage
- findings extraction
- Markdown and JSON export

Later, but not required for usefulness:

- diff review
- optional TUI
- optional grammar-constrained outputs
- optional scanner side-context ingestion
- optional editor integrations

## License

Apache 2.0

## Final statement

Reviva is a local review appliance for repositories. It exists to make LLM-assisted semantic review inspectable, reproducible, and useful without inheriting the instability of full agent stacks.
