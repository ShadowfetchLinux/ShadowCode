# Changelog

## 0.20.0 — native development (unreleased)

- Rust task engine with confined file tools, bounded processes, streaming local
  and compatible models, scoped approvals, checkpoints, persisted queues, and
  recovery. Settings and legacy history are migrated with backups.
- Tauri desktop window embeds the compiled interface and communicates through
  IPC, without a Python runtime or browser launcher. Native dialogs, external
  links, window state, notifications, and managed shutdown are implemented.
- Native goals preserve milestone results, pause and resume, require recorded
  inspection/verification for the default checklist, and stop on failed checks.
  Goal tasks appear live in the conversation; results and progress are accessible
  from the drawer.
- Native window automation covers actual tool execution, approvals, reload,
  cancellation, goal progression/pause, accessibility, and subprocess cleanup.
- Native AppImage and Debian build scripts preserve per-format executable
  metadata, include versioned dependency notices, and check startup, versions,
  matching compiled code, notice hashes, and absence of Python sidecars. The
  packaged AppImage runs the same real-window workflow as the debug executable.

This development branch is not yet the replacement release. Remaining
integrations, broader stress checks, packaging, and installation are tracked in
[the migration gates](docs/NATIVE_MIGRATION.md).

## 0.19.0 — 2026-09-19

### Workspace

- Persistent project/task sidebar with search and pinned tasks.
- Refined light and dark surfaces, responsive composer, consistent iconography.
- Markdown responses, code-copy buttons, task plans, saved text drafts, and
  scrolling that respects the user's position.
- Accessible modal dialogs, labeled controls, keyboard navigation, reduced-motion
  support, and explicit approval buttons.
- Integrated bounded terminal and staged/unstaged review, including new files.

### Runtime and correctness

- Durable desktop job records, interrupted-run recovery, and active-run reconnect.
- ID-based SSE replay fixes duplicate prior tasks and the 800-event stall.
- Tool-call identifiers correctly correlate parallel operations.
- Session activation switches the backend workspace; follow-ups recover bounded
  conversation context and preserve custom titles.
- Cancellation waits for worker exit and releases pending approvals.
- Concurrent workspace jobs are rejected; direct edits wait for active jobs.
- Live and final plan changes are persisted and displayed.
- Git uses literal paths/NUL-delimited status; stale hunk requests are rejected.
- Cross-origin browser requests and untrusted Host headers are rejected.
- Direct workspace mutations enforce read-only mode.

### Distribution and maintenance

- Standalone Linux x86_64 AppImage and portable archive with checksums.
- Python wheels include the compiled interface; MCP is a declared dependency.
- Source installer uses an isolated venv without uninstalling system packages.
- AppImage installer validates the executable before replacing earlier builds.
- CI checks Python, TypeScript, browser workflows, and accessibility; tag workflow
  produces release assets. Updated architecture, security, usage, and release docs.

## 0.18.0

Introduced the minimal drawer-based desktop, Goal Mode, per-task rewind, session
management, model picker, notifications, and source-checkout updater. Release
history is available on [GitHub](https://github.com/ShadowfetchLinux/ShadowCode/releases).
