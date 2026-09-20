# Native desktop migration — 0.20

The target is a Python-free Rust application with a dedicated Tauri window and
the existing React visual design. This document tracks engineering acceptance;
0.19 remains the supported release until the native gates are proved.

## Product and compatibility gates

- Ship an executable and AppImage containing no Python interpreter, Python
  sidecar, or browser-launch dependency. Embed the compiled interface. The native
  desktop communicates through Tauri IPC; a loopback service is optional for CLI
  integrations and browser automation, not required for desktop operation.
- Preserve existing XDG settings, secrets, sessions, tasks, events, goals, and
  workspaces. Back up before schema migration, use atomic writes, and lock the
  active profile so concurrent managers cannot corrupt job recovery.
- Carry forward local/Ollama and OpenAI-compatible model discovery, custom
  endpoints, model testing, model selection/routing, streaming, usage, context
  compaction, cancellation, and recoverable persisted task execution.
- Carry forward file/search/edit/patch/terminal/git tools, parallel safe reads,
  task checkpoints and conflict-aware rewind, review/staging/commits, explicit
  approvals, read-only planning/review modes, project trust, and audit events.
- Carry forward goals/milestones, task continuation/branch/export/search/pins,
  persistent project instructions and skills, hooks, plugins, background
  processes, MCP client/server, and the CLI.
- Add a cohesive native desktop experience: OS folder/file pickers, single
  instance activation, managed shutdown, notifications, persistent window state,
  safe external-link opening, and install/update information.
- Extend flagship workflows with visible execution plans and evidence, bounded
  context and costs, queued follow-ups, isolated worktree workflows, and bounded
  delegated read/review tasks with clear ownership and cancellation.
- Keep light/dark themes, responsive layout, keyboard navigation, focus
  containment, readable Markdown/code, draft recovery, and accessible controls.

## Verification and release gates

- Native unit/integration coverage must exercise behavior and failure modes,
  including legacy-data migration, path escapes, stale writes/rewinds, shell
  output floods, hung subprocesses, cancellation, approval isolation, provider
  failures, malformed streams, event replay, and restart recovery.
- Run real coding tasks with multiple available local models in disposable
  projects, covering file edits, tool loops, verification, continuation, and
  cancellation. Record the actual outcomes and limits.
- Stress concurrent sessions, long transcripts, large outputs, repeated task
  execution, UI navigation, and shutdown. Inspect memory/process cleanup and
  persistence after interruption.
- Exercise both the real native window and the browser test transport. Verify
  accessibility and appearance at desktop and compact sizes.
- Build on a clean CI runner, verify the downloadable artifact on this machine,
  include dependency notices and checked package metadata,
  update all user/developer/security/release documentation and repository info,
  publish source plus checksummed binaries to GitHub, and install the verified
  release in Applications while retaining user data.

## Current implementation

The Rust workspace is being introduced alongside the Python release. Existing
features remain acceptance requirements; an unimplemented native feature must
not be silently replaced with a stub or removed from the release claims.

Implemented foundation:

- SQLite migration with a consistent backup, legacy goal import, session
  branching/search/pins, stable event cursors, persisted jobs and message history,
  interrupted-job recovery, and idempotent token accounting.
- Validated configuration and private atomic secret storage. Project overlays
  can tune bounded agent settings or reduce permissions; they cannot redirect
  credentials, grant permissions, or register executable integrations.
- Workspace directory capabilities, atomic text edits, stale-content checks,
  bounded search and file reads, and Git metadata protections.
- A native subprocess runner with bounded concurrent output capture, cancellation,
  timeouts, process-group cleanup, and protection against detached output pipes.
  This remains a user-level process runner, not an operating-system sandbox.
- Native Ollama and compatible streaming transports with incremental UTF-8/SSE
  parsing, parallel tool-call assembly, usage accounting, bounded responses,
  cancellation, and rejection of incomplete tool arguments.
- A persisted native task loop, per-workspace FIFO follow-ups, four concurrent
  workspaces, scoped approvals, cancellation that waits for cleanup, restart
  recovery, and atomic completion/usage/event commits. Continuation and branching
  retain valid function-call histories; interrupted calls are never blindly replayed.
- Native filesystem, search, patch, shell, Git, and plan tools with durable audit
  events. Patches preflight every file, reject ambiguous context, preserve line
  endings, and journal edits for conflict-aware rewind. Shell and Git side effects
  are not represented as filesystem-tool checkpoints.
- Context compaction preserves complete tool-call groups and the current request.
  Explicit requests to read a named file attach its current confined contents.
  Providers that omit token usage receive labelled estimates for budget checks.
- A shared native command service for onboarding/model configuration, project
  trust, sessions/branching/export/pins, task replay and approvals, files,
  instructions/skill storage, terminal commands, Git review, and checkpoint rewind.
  Manual mutations reserve the same workspace registry as agent tasks. Shutdown
  cancels manual commands and waits for their cleanup; navigation preserves audit
  attribution. Per-hunk staging/discard validates the current diff, and exports
  traverse the full event history up to an explicit 32 MB limit.
- [Native engine verification](NATIVE_VERIFICATION.md), including scripted failure
  cases, concurrent tasks, and real coding probes against two local models.
- A Tauri desktop executable with embedded interface assets and Rust IPC, native
  dialog/export/link adapters, default-profile single-instance activation,
  persisted window state, notifications, and managed close/termination handling.
  Native UI replay preserves streaming message IDs, tool paths, and durable
  completion. The actual window is exercised through WebKit WebDriver.
- [Native goals and milestones](NATIVE_GOALS.md) with persisted task attribution,
  sequential execution through the shared workspace queue, pause/cancellation,
  resume that skips completed work, and conservative recovery after interruption.
  The default inspection requires current reads; final verification requires a
  recorded successful command. Failed verification blocks further progression.
  Manual checklist edits are labelled and disallowed while the goal is running.
- Repeatable Linux AppImage and Debian packaging with the native ELF executable,
  embedded UI, legacy `shadow ui` launcher compatibility, exact version metadata,
  and package inspection for Python runtimes or sidecars. Both packages contain
  application dependency notices; the AppImage adds an inventory and copyright
  texts for its actual bundled libraries. Package checks verify every listed
  notice's checksum. Corresponding-source artifacts and final release validation
  remain separate gates.
- Clean Ubuntu 24.04 CI builds and tests both native package formats. The
  downloaded CI artifacts pass checksum and package inspection locally, and the
  AppImage passes the full native-window workflow on both machines. Separate
  local AppImage probes exercise actual Ollama gpt-oss and Qwen coding tasks,
  approvals, saved continuation, streaming cancellation, and checkpoint rewind.

The remaining orchestration/integrations, full desktop interaction/stress
coverage, and release packaging are still in progress. The native
branch is not yet a replacement for the 0.19 release.

See [native desktop development](NATIVE_DESKTOP.md) for prerequisites and isolated
build/run instructions. Compile the interface before checks that include the
desktop crate.

Developer checks:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# Read-only protocol probe against an already installed local model:
cargo run -p shadowcode-core --example probe_model -- gpt-oss:20b
# Real coding task, command approval, continuation, and rewind in a temp project:
cargo run -p shadowcode-core --example probe_task -- gpt-oss:20b
```

Protocol references: [Tauri commands](https://v2.tauri.app/develop/calling-rust/),
[Tauri events](https://v2.tauri.app/develop/calling-frontend/),
[compatible function-call streaming](https://developers.openai.com/api/docs/guides/function-calling),
and [Ollama chat](https://docs.ollama.com/api/chat).
