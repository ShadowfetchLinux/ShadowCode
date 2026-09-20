# Changelog

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
