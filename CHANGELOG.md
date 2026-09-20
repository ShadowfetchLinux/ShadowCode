# Changelog

## 0.20.0 — native development (unreleased)

- Managed worktrees support inspection and reviewed clean removal. Dirty/ignored
  files, detached or locked checkouts, active tasks and background servers block
  removal. Branches and commits remain; private records are archived after success.

- Native worktree creation starts an isolated branch from a local commit while
  preserving source checkout edits. CLI inventory and private recovery records
  retain visibility into interrupted creation. The complete worktree workflow
  remains in development.

- Desktop polling uses compact recent/active job records instead of repeatedly
  transferring full saved results. Older active jobs remain visible; expanding
  queued prompts and opening conversations fetch their complete records on demand.

- CLI conversation and job IDs resolve against the full saved history instead
  of recent-list limits. Old records remain available for continuation, export,
  rename and job inspection; default conversation selection is project-scoped.
  Indexed lookup avoids loading unrelated job results just to resolve a prefix.

- Native `tui` frontend shares the Rust engine with desktop and CLI: Unicode
  input, conversation/model/project pickers, task modes, queued follow-ups,
  explicit approvals, tool cards and paged saved history. Terminal-owned workflows
  cancel on disconnect while unrelated work in an attached engine continues.
  Real PTY checks exercise submission, approval, planning and terminal restoration.

- Concurrent desktop/CLI settings changes preserve unrelated fields, project
  trust, integration grants and secret entries. Read–modify–write operations are
  serialized through the owning native engine. Rejected updates preserve the
  saved file; configuration/secret reads reject special files and remain bounded,
  and writes cannot exceed the size accepted on restart.

- Native project plugins install reviewed built-in or local JSON bundles into
  real skills, slash commands, hooks and MCP definitions. Settings and CLI show
  contents and require current project/content hashes; executable integrations
  need separate activation. Private journals support interrupted-install cleanup,
  uninstall preserves local edits, and legacy plugin directories remain intact.
  Native Python/Linux/iOS workflows replace executable Python plugin loading.

- MCP registration prints Codex, Claude Code and Cursor configuration for stdio
  or an existing authenticated HTTP gateway. Output references credential
  variables without exposing secrets; literal path encoding and option validation
  prevent accidental configuration changes. Independent official TypeScript SDK
  1.x/2.x probes exercise both transports with real native approvals and test jobs.

- Native MCP serving supports authenticated loopback Streamable HTTP alongside
  stdio. Both expose the same project-scoped tools, approvals and owned tasks.
  HTTP reconnects preserve the gateway's jobs; gateway shutdown cancels unfinished
  owned work while unrelated engine tasks continue. Mandatory credentials, host
  checks, denied browser origins, bounded traffic and protocol validation apply.
  The executable probe runs both transports against source and packaged builds.

- Rust task engine with confined file tools, bounded processes, streaming local
  and compatible models, scoped approvals, checkpoints, persisted queues, and
  recovery. Settings and legacy history are migrated with backups.
- Tauri desktop window embeds the compiled interface and communicates through
  IPC, without a Python runtime or browser launcher. Native dialogs, external
  links, window state, notifications, and managed shutdown are implemented.
- Native profile directories are private to the current account, including
  existing profiles, without deleting data or changing existing parent-directory
  permissions. Unsafe profile leaves and lock files are rejected. Notifications
  keep full text contrast throughout their entrance animation.
- Native SQLite tools, CLI and MCP queries replace the Python built-in reader,
  with parameter binding, live WAL support, confined paths, denied SQL writes
  and bounded/cancellable results. Bundled SQLite is updated to 3.53.2, including
  its WAL-reset corruption fix.
- Required task inspection recognizes successful native database reads. Small
  model contexts can use a shorter, enforced response budget while retaining
  required input; requests that still cannot fit fail before contacting the model.
- The native desktop can queue follow-up messages while the current task streams,
  show waiting work across project conversations and cancel individual queued
  tasks. Reloads select the running task first; queue cancellation cannot stop a
  task that has already begun. Sidebar activity and transcript progress now
  distinguish queued, running and completed work.
- Native goals preserve milestone results, pause and resume, require recorded
  inspection/verification for the default checklist, and stop on failed checks.
  Goal tasks appear live in the conversation; results and progress are accessible
  from the drawer.
- Native model routing adds editable Plan/Build/Review/Test selections, explicit
  task overrides, and recorded selection/fallback notices. Model registration
  distinguishes identical names on different endpoints and preserves saved
  credential references and context limits during discovery.
- Native background processes run development servers and watchers alongside
  tasks, retain bounded live logs and durable history, honor project permissions,
  and stop managed child processes when the application closes. Legacy history
  is imported without signalling stored PIDs or replaying old commands.
- Models can start project servers under shell permissions and command hooks,
  inspect bounded status/logs, and request scoped approval to stop them. Plan
  and Review retain read-only access. Background servers appear in the shared
  panel and survive coding-task completion or cancellation until explicitly
  stopped or the owning application closes, with durable task attribution.
  Background cards use readable labels and explicit start/stop prompts; approval
  headings wrap on compact windows and warning labels meet light-theme contrast.
  Following new activity now scrolls before paint, including task submission,
  so a newly added approval remains in view.
- Native slash commands run real tasks and terminal actions, persist command
  cards, and support explicit project skills with source/hash provenance. Skills
  preserve Plan/Review restrictions and queued instructions; the editor validates
  metadata and rejects stale saves. Project notes guide subsequent tasks.
- Native CLI runs tasks without a display, streams ordered event JSON, handles
  terminal and external approvals, and exposes sessions/export, checkpoints,
  goals, model settings, skills, and background controls. It shares an open
  desktop or explicit headless owner through a private Unix connection while
  keeping project selection independent. Interruptions, broken output pipes,
  and disconnected manual commands clean up their owned work.
- Native window automation covers actual tool execution, approvals, reload,
  cancellation, goal progression/pause, accessibility, and subprocess cleanup.
- Native lifecycle hooks use reviewed YAML/JSON command definitions with explicit
  project/content approval in Settings or the CLI. Command/commit gates block
  failed actions, completion checks drive bounded repair, and persisted results
  survive reload. Changed definitions require review; read-only tasks keep hooks
  inactive, and cancellation/shutdown wait for command cleanup. Legacy Python
  callbacks are surfaced for migration without importing them.
- File tools explain how to create files with `expected_hash: missing` and reject
  malformed hashes with actionable guidance. Existing read/stale-hash protections
  remain in force. Completion-check diagnostics are bounded before model repair.
- Ollama requests preserve runtime repair and compaction instructions for model
  templates that ignore later system messages. Original positions remain labelled,
  user/tool data retains its role, and saved conversation order is unchanged.
- Native AppImage and Debian build scripts preserve per-format executable
  metadata, include versioned dependency notices, and check startup, versions,
  matching compiled code, notice hashes, and absence of Python sidecars. The
  packaged AppImage runs the same real-window workflow as the debug executable.
- Source-built AppImage runtime gives every extraction-mode launch a private
  directory, forwards shutdown signals, and cleans up after payload exit.
  Concurrent CLI/window launches preserve each other's resources. The build
  retains runtime source archives, patches, notices, and compiler provenance;
  verification checks the packaged runtime code and matching source artifact.
- Manual native-window probes with installed Ollama models cover minimal coding
  edits, visible command approval, independent verification, saved continuation,
  cancellation during actual streaming, checkpoint rewind, and process shutdown.

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
