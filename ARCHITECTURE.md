# ShadowCode architecture

Native 0.20 is a Rust engine with a Tauri desktop, a private Unix-socket CLI,
and the existing React visual design. The 0.19 Python harness remains the last
supported release until the [native gates](docs/NATIVE_MIGRATION.md) are proved.

```text
CLI / TUI / Tauri desktop / MCP
                |
          Native Engine
                |
  Context / Plan / Tools / Permissions / Verify
                |
         Model interface
                |
 Ollama / OpenAI-compatible / local / mock
```

## Native runtime (0.20)

`native/core` owns understand → plan → inspect/act → observe → verify. The
verifier can request a fix cycle. Tool calls and observed results produce
structured events; final status reflects verification, cancellation, and
restart interruption. Plans are emitted after automatic progress and explicit
model updates.

`store.rs` uses SQLite for sessions, tasks, events, models, projects, pins,
jobs, goals, notes, and background processes. Schema changes are additive
(`user_version` 24). Events have monotonically increasing IDs and an index on
`(session_id, id)`. Restart recovery marks unfinished jobs interrupted and
writes a durable `agent.completed` event so history and export see the stop.

`engine.rs` owns desktop/CLI/MCP jobs. A workspace admits one active agent job
plus queued follow-ups. Cancellation sets a durable request and releases pending
approvals; terminal status follows worker exit. A profile lock prevents a second
manager from recovering live jobs. The desktop may attach to a persistent
headless/TUI engine; closing an attached window leaves that work running.
If that owner process exits and a matching engine returns on the same
socket, an attached view reopens its lease (`view.reattached`) without
starting jobs, retrying mutating requests, or replaying completed tools.
The UI then fetches committed rows from the last event cursor.

Autonomy (0.21 branch) adds inspectable layered context accounting before each
model request, a deterministic compaction keep-list, replay classes for crash
recovery, progressive loop handling, and claim/observed/verified completion.
These extend `context.rs` and `engine.rs`; they do not replace checkpoints or
the permission checker. Named autonomy profiles never raise configured caps
and never silently kill a task. Tool results that exceed their byte or range
budget set `truncated` and a model-visible note. Worktree repair still
requires the recorded real path (`guess_paths: false`).

Filesystem tools resolve paths with directory capabilities. Shell classification
is a policy check, not kernel isolation. See [SECURITY.md](SECURITY.md).

## Session continuity

Activating a session changes the selected workspace, checks that its folder
exists, and returns a bounded recent page plus `history_page` cursors on the
native desktop. Follow-up tasks hydrate a complete tool-call/result tape when
one exists; interrupted calls are never blindly replayed. Old tool calls are
not re-executed. Project notes are a separate context source.

The desktop hydrates the saved page and asks for the current/latest job. Native
event delivery is durable-fetch plus a bounded wakeup feed. Duplicate event IDs
are ignored. A connection error keeps the task running and retries from the
last cursor. `job.done` is synthesized from persisted job status after the
durable completion event.

## Frontend

- `App.tsx`: application state, project activation, composer, shortcuts, panels.
- `hooks/useConversation.ts`: stream lifecycle, reconnects, paged history.
- `lib/transcript.ts`: pure event reducer and replay.
- `lib/jobEvents.ts`: native catch-up and adjacent stream compaction.
- `components/Markdown.tsx`: safe Markdown; no raw HTML, remote images, or
  non-http(s) link schemes from model output.
- `components/Dialog.tsx`: modal focus containment and restoration.
- `components/Drawer.tsx`: files, review, terminal, goals, skills, health,
  background processes.
- `index.css` / `workspace.css`: shared controls, layout, and themes.

Task selection, pins, sidebar visibility, and unsent drafts live in local
browser storage. Durable agent work and history live in SQLite. Scroll following
stops when the user reads older content.

## 0.19 Python harness

The supported release still uses `agent/loop.py`, FastAPI on loopback, and the
same React client over HTTP/SSE. `runtime.py` owns desktop/MCP jobs; `jobs.py`
is a separate legacy CLI background-job store. See the 0.19 user guide and
[SECURITY.md](SECURITY.md) for that API's host/origin rules.

## Distribution

Native packages embed the compiled interface and a Rust executable. They contain
no Python interpreter or browser launcher. The 0.19 AppImage still bundles
Python. Neither installer migrates or deletes user configuration. Verify
downloads against `SHA256SUMS`.

## Verification

Native unit/integration tests cover migration, recovery, path confinement,
approvals, provider streams, MCP, worktrees, and process cleanup. Vitest covers
the reducer, job stream, and Markdown safety. Playwright and the native WebKit
window suite exercise workflow, reload/reconnect, and accessibility. Recorded
outcomes live in [docs/NATIVE_VERIFICATION.md](docs/NATIVE_VERIFICATION.md).
