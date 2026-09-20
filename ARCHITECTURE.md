# ShadowCode architecture

ShadowCode 0.19.0 has one Python harness shared by the CLI, TUI, desktop API,
and MCP server. React is a client of that harness.

```text
CLI / TUI / React desktop / MCP
                |
       AgentRunner + runtime
                |
  Context / Plan / Tools / Permissions / Verify
                |
         Model interface
                |
 Ollama / OpenAI-compatible / local / mock
```

## Runtime and persistence

`agent/loop.py` drives understand → plan → inspect/act → observe → verify.
The verifier can request a fix cycle. Tool calls and observed results produce
structured events; final status reflects verification and cancellation.
`planning/plan.py` supplies typed plan phases. Plans are emitted after automatic
progress and explicit model updates, including in the final event.

`store.py` uses SQLite for sessions, tasks, events, models, projects, pins, and
`desktop_jobs`. Schema changes are additive. Events have monotonically increasing
IDs and an index on `(session_id, id)`.

`runtime.py` owns desktop/MCP jobs and worker threads. A task gets an event cursor
before its worker starts. A workspace admits one active job per manager. Cancellation
sets a durable request and releases pending approvals; terminal status follows
worker exit. Finished records persist. On service startup, unfinished records are
marked interrupted; a worker cannot continue across process termination.

`jobs.py` is the separate legacy CLI background-job implementation, using
`jobs.db`. It is not the desktop job store. Run one desktop API service per XDG
profile; multiple independent API/MCP managers are not a distributed scheduler.

## Session continuity

Activating a session explicitly changes the API workspace, checks that its folder
exists, and returns its tasks, event history, and last event ID. Follow-up tasks
hydrate up to 24 recent prompt/result events within a bounded character budget.
Branched sessions copy the event history. Old tool calls are not replayed into a
new model request. Project memory is a separate source of context.

The desktop hydrates the saved transcript and asks for the current/latest job.
SSE requests begin after the hydrated cursor. `Last-Event-ID` and the `after`
query parameter support reconnects. The server reads ascending pages, drains
remaining events, then emits `job.done`. There is no fixed-length-array cursor.
The client ignores duplicate event IDs and matches parallel tool results by
call ID. A connection error keeps the task running and enables polling fallback.

## Frontend

- `App.tsx`: application state, project activation, composer, shortcuts, panels.
- `components/Sidebar.tsx`: projects, task search, pinned tasks.
- `hooks/useConversation.ts`: stream lifecycle, reconnects, terminal snapshots.
- `lib/transcript.ts`: pure event reducer and replay.
- `components/Markdown.tsx`: safe Markdown, code copy; no raw HTML or remote image
  loading from model output.
- `components/Dialog.tsx`: modal focus containment and restoration.
- `components/Drawer.tsx`: files, staged/unstaged review, bounded terminal,
  task management, goals, skills, health, and background processes.
- `index.css`: shared controls; `workspace.css`: workspace layout and themes.

Task selection, pins, sidebar visibility, and unsent text drafts live in local
browser storage. Durable agent work and history live in SQLite. Scroll following
stops when the user reads older content. Appearance follows saved app settings.

## Review and permissions

`review.py` invokes Git with literal pathspecs, no external diff driver, a timeout,
and NUL-delimited status parsing. New text files get bounded previews. Staged and
unstaged diffs stay separate. Hunk mutations compare the supplied hunk to the
current diff before applying it. New files are staged whole.

The HTTP API accepts loopback hostnames and same-origin browser requests, blocks
cross-site access, and adds CSP/frame/content-type protections. CLI requests
without Origin remain supported. Direct workspace mutations honor read-only
permissions and reject writes while a desktop task owns that workspace.

Filesystem tools use `WorkspaceSandbox` path resolution. Shell command controls
are policy checks, not kernel isolation. See [SECURITY.md](SECURITY.md).

## Distribution

`resources.py` locates assets in source, a wheel's `_assets`, or PyInstaller's
bundle. Wheels include the compiled UI. PyInstaller produces a portable directory;
`build-linux.sh` wraps it as an x86_64 AppImage and archive. The standalone launcher
keeps its AppImage mount/extraction alive while serving the API. The browser uses
a separate profile and the existing `shadow-agent` desktop identity.

Source installations use an isolated `.venv`. AppImage installation writes a
stable `~/Applications/ShadowCode.AppImage` link and replaces the launcher after
validating the executable. Neither path migrates or deletes user configuration.

## Verification

Python tests cover the harness, APIs, database migration, cursor replay, recovery,
cancellation, workspace selection, origin checks, and Git review. Vitest exercises
the reducer. Playwright uses a temporary XDG profile and the real API with a mock
model; it covers workflow, reload/reconnect, responsive layout, and accessibility.
A local-model smoke test supplements deterministic tests before a release.
