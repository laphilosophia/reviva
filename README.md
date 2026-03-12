# Reviva

Reviva is a local-first code review terminal for constrained, inspectable, and repeatable repository analysis with local LLMs.

> It is not a coding agent, not an IDE copilot, and not a general-purpose chat interface. Its job is narrower: traverse a repository, let the user select files or boundaries, run focused review modes against a local model backend, and store findings in a form that can be revisited later.

> The project exists because local review workflows are often degraded by opaque editor integrations, prompt-template drift, hidden tool-calling behavior, and fragile chat abstractions. Reviva removes that layer and keeps the review path explicit.

## Why this exists

A local model may be perfectly capable of reviewing code, yet still become unreliable once it is wrapped by editor extensions, integrated chats, agent loops, or hidden prompt orchestration. The result is familiar: malformed responses, self-generated questions, broken role formatting, unstable context behavior, or review output that cannot be trusted operationally.

Reviva addresses that by reducing the surface area.

- The user selects the files.
- The review mode is explicit.
- The prompt is inspectable.
- The backend is local.
- The output is preserved.
- The findings are stored.

## What it does

Reviva scans a repository, allows explicit target selection, builds a controlled prompt for a selected review mode, sends it to a local inference backend, displays the response, and records findings.

> The initial focus is semantic review, not code generation.

The model is treated as a constrained reviewer, not an author. Supported review modes are intended to stay narrow and operational:

```text
-> contract
-> boundary
-> boundedness
-> failure-semantics
-> performance-risk
-> memory-risk
-> operator-correctness
-> launch-readiness
-> maintainability
```

## What it is not

- Reviva does not replace Semgrep, CodeQL, profilers, tests, or benchmarks.
- Reviva does not autonomously plan work.
- Reviva does not mutate repository files by default.
- Reviva does not depend on cloud inference.
- Reviva does not attempt to become a chatbot.

## Design goals

The project is built around a few hard constraints.

1. Local-first operation. Repository contents should stay local unless the user explicitly configures a remote backend.
2. Prompt transparency. Prompt construction must be visible and inspectable.
3. Narrow review modes. Broad “review the repo” behavior is low-signal and unstable. Focused review modes are more useful.
4. Reproducibility. The same file set, review mode, and model settings should produce operationally comparable output.
5. Plain failure behavior. Empty responses, malformed output, timeouts, prompt oversize, and backend errors must be surfaced directly rather than hidden.

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

## Architecture

Reviva assumes an external local inference server.

The first target is a completion-style HTTP backend, such as `llama-server`, because completion-style requests give explicit control over prompt construction and avoid some of the fragility introduced by chat-template handling.

Chat-style backends may be supported later, but only if prompt behavior remains explicit and testable.

## Output philosophy

The first version prefers stable plain-text outputs with fixed sections over aggressive structured-output assumptions.

Raw model output must always be preserved.
If finding extraction fails, the raw output still remains part of the session record.
The system should not discard results merely because formatting drifted.

A finding is expected to carry at least:

```text
-> severity
-> risk class
-> location
-> issue
-> why it matters
-> confidence
-> action
```

## Storage model

Reviva stores its state locally in a human-inspectable format.

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

Reviva defaults to local-only behavior. This is review infrastructure, not data collection.

```text
Not execute repository code.
Not include hidden telemetry.
Vlearly display which backend URL is active.
Never silently send repository content elsewhere.
```

## Contributing

Contributions are welcome, but changes that push the system toward autonomous agent behavior, hidden orchestration, or chat-centric UX will likely be rejected. The tool should stay small, explicit, and operationally trustworthy.

If you contribute, keep the following in mind:

- prefer explicit behavior over convenience
- preserve prompt inspectability
- keep review modes narrow
- do not hide backend behavior
- avoid feature creep into copilot territory

## License

Apache 2.0

## Final statement

Reviva is a local review appliance for repositories. It exists to make LLM-assisted semantic review inspectable, reproducible, and useful without inheriting the instability of full agent stacks.
