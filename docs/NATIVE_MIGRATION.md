# Native desktop migration — 0.20

The target is a Python-free Rust application with a dedicated Tauri window and
the existing React visual design. This document tracks engineering acceptance;
0.19 remains the supported release until the native gates are proved.

## Product and compatibility gates

- Ship an executable and AppImage containing no Python interpreter, Python
  sidecar, or browser-launch dependency. Embed the compiled interface. The native
  desktop communicates through Tauri IPC; native CLI clients share the engine
  through a private Unix socket, with no desktop HTTP listener required.
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

- [Isolated worktree creation](NATIVE_WORKTREES.md) from local commits, with a
  separate managed branch, preserved source edits, private recovery records and
  CLI inventory, desktop Settings controls and reviewed clean removal that retains
  branches and commits.
  CLI and desktop rescue restore a missing checkout’s retained commit into a
  separate checkout while preserving original recovery metadata. Damaged-path
  repair now restores missing connection files at the original managed location;
  moved paths and conflicting/lost metadata still need recovery work. CLI and desktop copy
  preserves reviewed staged/unstaged patches and regular untracked files in a
  new checkout without resetting the source.
  Reviewed CLI and desktop return prepare a merge without committing, preserving source
  branch work and leaving conflicts for explicit resolution or Git merge abort.

The Rust workspace is being introduced alongside the Python release. Existing
features remain acceptance requirements; an unimplemented native feature must
not be silently replaced with a stub or removed from the release claims.

Implemented foundation:

- SQLite migration with a consistent backup, legacy goal import, session
  branching/search/pins, stable event cursors, persisted jobs and message history,
  interrupted-job recovery, and idempotent token accounting.
- Validated configuration and private atomic secret storage. Synchronous
  read–modify–write operations serialize concurrent desktop/CLI changes, retaining
  independent settings, trust, hook/MCP grants and credential entries. Reads
  require bounded regular files; writes cannot exceed the restart read limit.
  Project overlays can tune bounded agent settings or reduce permissions; they cannot redirect
  credentials, grant permissions, or register executable integrations.
  Native profile directories use mode 700, preserving legacy contents and
  existing parent permissions; unsafe directory leaves and lock files fail closed.
- Workspace directory capabilities, atomic text edits, stale-content checks,
  bounded search and file reads, and Git metadata protections.
- A native subprocess runner with bounded concurrent output capture, cancellation,
  timeouts, process-group cleanup, and protection against detached output pipes.
  This remains a user-level process runner, not an operating-system sandbox.
- Native Ollama and compatible streaming transports with incremental UTF-8/SSE
  parsing, parallel tool-call assembly, usage accounting, bounded responses,
  cancellation, and rejection of incomplete tool arguments.
- [Provider-aware model registration and routing](NATIVE_ROUTING.md), with
  editable Plan/Build/Review/Test selections, preserved custom credentials and
  context limits, queued configuration snapshots, explicit per-task overrides,
  persisted model/fallback notices, and goal verification routed to Test.
- [Native commands and project skills](NATIVE_WORKFLOWS.md) with confined
  discovery, explicit invocation, frozen queued instructions, model routing,
  preserved Plan/Review permissions, source/hash provenance, durable command
  cards, validated editing, and project notes included in task guidance.
- [Native CLI](NATIVE_CLI.md) for tasks, commands/skills, approvals, sessions and
  export, model settings, goals, checkpoints, and background controls. It starts
  without a display and shares an active desktop or explicit headless owner
  through a private local connection. Client navigation does not switch the
  desktop project. Owned tasks cancel on interrupt/output failure; watching an
  existing task leaves it running. Updater/automatic diagnostic repairs and integration
  migration remain separate requirements.
- Desktop startup can attach to an existing persistent headless/TUI engine
  through independent connection-scoped navigation. Closing an attached window
  leaves shared work running. A bounded event feed drives durable replay and
  completion notifications. Automatic reattachment remains open; see
  [desktop lifetime](NATIVE_DESKTOP.md).
- [Native terminal interface](NATIVE_TUI.md) with a shared engine, model/session/project
  pickers, Unicode composer, explicit approvals, task modes, queued follow-ups,
  tool cards and paged history. Real PTY checks cover submission, approval and
  cleanup. Broader terminal navigation, visual and stress checks remain open.
- [Native project plugins](NATIVE_PLUGINS.md) with built-in and custom bundle
  review/import in Settings and CLI, namespaced usable workflows, separately
  activated hooks/MCP, collision and stale-hash checks, private recovery journals,
  and uninstall that preserves edits and legacy plugin contents. Bundles cannot
  install dependencies, execute scripts or change permissions during installation.
- [Native lifecycle hooks](NATIVE_HOOKS.md) with explicit project/content
  activation, bounded sequential command execution, gates before commands and
  commits, checks after edits/tests, completion repair, and error/compaction
  events. Settings and CLI expose review/activation; durable cards expose results.
  Changed definitions fail closed, read-only tasks stay inert, and cancellation
  cleans up owned subprocesses. Legacy Python callbacks require conversion.
- [Managed background processes](NATIVE_BACKGROUND.md) for project servers and
  watchers, with permission checks, bounded live log tails, durable status and
  audit events, project-scoped controls, bounded concurrency, legacy history
  import, and stop/shutdown that waits for process-group cleanup. Model tools
  use scoped shell/start and stop approvals, command hooks, bounded status/log
  reads and durable task attribution. These project processes intentionally
  survive coding-task completion or cancellation until explicitly stopped.
- Sustained native-engine verification covers 200 tasks across four projects,
  stalled-provider cancellation, repeated subprocess output floods, sampled
  memory/descriptors, process cleanup and persisted history after restart.
  Desktop memory, long individual conversations and larger repository stress
  remain separate checks.
- A persisted native task loop, per-workspace FIFO follow-ups, four concurrent
  workspaces, scoped approvals, cancellation that waits for cleanup, restart
  recovery, and atomic completion/usage/event commits. Continuation and branching
  retain valid function-call histories; interrupted calls are never blindly replayed.
- [Paged native desktop history](NATIVE_HISTORY.md) bounds saved snapshots and
  supports older/newer/latest navigation while current work continues. Original
  records and exports remain intact. Long-running live transcript memory and
  renderer virtualization remain separate gates.
- A [desktop follow-up queue](NATIVE_QUEUE.md) with visible project-wide waiting
  messages, model/mode snapshots, independent queued cancellation, continued
  streaming, conversation switching and reload recovery. Active history remains
  discoverable beyond recent-list limits; a stale queue action cannot cancel a
  task that has started running.
- Native filesystem, search, patch, shell, Git, and plan tools with durable audit
  events. Patches preflight every file, reject ambiguous context, preserve line
  endings, and journal edits for conflict-aware rewind. Shell and Git side effects
  are not represented as filesystem-tool checkpoints.
- Context compaction preserves complete tool-call groups and the current request.
  Explicit requests to read a named file attach its current confined contents.
  Providers that omit token usage receive labelled estimates for budget checks.
- A shared native command service for onboarding/model configuration, project
  trust, sessions/branching/export/pins, task replay and approvals, files,
  instructions/skill editing and execution, terminal commands, Git review, and checkpoint rewind.
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
- Clean Ubuntu 24.04 CI builds and tests both native package formats. The tagged
  native-release workflow repeats the Rust, UI, CLI, TUI, MCP, stress, window,
  package, runtime-source and packaged-app checks before it publishes checksummed
  AppImage, Debian and retained-runtime-source artifacts. The
  downloaded CI artifacts pass checksum and package inspection locally, and the
  AppImage passes the full native-window workflow on both machines. Separate
  local AppImage probes exercise actual Ollama gpt-oss and Qwen coding tasks,
  approvals, saved continuation, streaming cancellation, and checkpoint rewind.

The remaining orchestration/integrations, full desktop interaction/stress
coverage, and release packaging are still in progress. The native
branch is not yet a replacement for the 0.19 release.

The [native MCP client](NATIVE_MCP.md) has bounded Rust stdio and Streamable HTTP
transports, inert Settings/CLI registration, explicit project/content activation, lazy tool
discovery, individually approved calls, and task-owned process/stream cleanup.
HTTP uses explicit credential references, prevents redirects and automatic call
retries, and supports modern and legacy Streamable HTTP negotiation. The native
stdio and authenticated loopback HTTP servers expose seventeen tools, four resources and two prompts through
the shared engine, with a fixed project, reduced per-task permissions, explicit
approval delegation and owned jobs. HTTP uses a gateway owner across stateless
requests, with mandatory bearer authentication and bounded traffic. Registration
prints Codex TOML or Claude Code/Cursor JSON with client-specific credential
references. Independent TypeScript SDK 1.x and 2.x probes exercise both transports
against the actual executable; host-application coverage remains explicitly scoped
in the verification record. Private engine-side ownership now cancels
MCP jobs after gateway SIGKILL, including unread submission replies, queued tasks
and active command children; unrelated work continues.

[Native project inspection](NATIVE_INSPECTION.md) supplies bounded maps, generated
note sections that preserve user text, native runtime/profile/project diagnostics,
and literal-path Git history. CLI and desktop commands use the same service.
MCP test jobs run actual commands without a model, with exact approvals, hooks,
queue ownership, real output/exit status, and timeout/process cleanup.

[Native task notes](NATIVE_MEMORY.md) extend project memory with scoped persistent
notes, legacy-file preservation, atomic edits with stale-hash rejection, bounded
continuation context, independent branch archives and complete note exports.
The desktop, CLI and MCP server share this service.

[Native SQLite inspection](NATIVE_SQLITE.md) replaces the Python built-in tools
with confined, bounded read-only queries, typed results, parameter binding,
live WAL coordination and cancellation. Model tools, CLI and MCP share the
reader. The history store also uses the updated bundled SQLite 3.53.2 library.

AppImage extraction now uses a source-pinned runtime patch with private
per-invocation directories, signal forwarding, and checked cleanup. A dedicated
regression exercises concurrent owners/clients, deferred WebKit resources,
process groups, `nohup`, and path limits. Packaged window automation also checks
repeated default-profile activation under disposable XDG roots and private DBus.
The runtime's notices, source archive, and compiled code are verified as part of
packaging. Source retrieval accepts listed mirrors only for the pinned bytes;
retained sources are reused and rechecked. A separate regression rebuilds the
shipped runtime sources in a container with networking disabled and compares
the resulting machine code. Corresponding sources for all remaining redistributed
dependencies and the broader release gates above still apply.

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
