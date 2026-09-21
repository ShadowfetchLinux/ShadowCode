# Native engine verification — development branch

Recorded on 2026-09-20. This is evidence for the Rust engine under development,
not a claim that the native desktop release is complete. The full release gates
remain in [NATIVE_MIGRATION.md](NATIVE_MIGRATION.md).

## Automated checks

`cargo fmt --all --check`, Clippy with warnings denied, and all **234 native unit and
integration tests** pass on the development machine. The suite covers:

- Config validation, private secrets, untrusted project overlays, profile locks,
  legacy SQLite backups and goal import, session branching, and durable replay.
- Configuration concurrency: twelve synchronized writers previously lost
  independent settings and secret entries; the regression now preserves all of
  them. Eight service clients concurrently retain project trust, settings,
  credential entries, MCP registrations/grants and unrelated hooks through
  plugin install/activation/removal. Rejected edits preserve exact saved bytes;
  oversized writes do not brick the profile, and named pipes fail without waiting.
- Private native profile directories and lock permissions, migration of permissive
  existing directories without changing their contents or parent permissions,
  relocated profile parents, and rejection of symlink leaves, symlink/hard-linked
  locks, directories and FIFOs without changing their targets or hanging startup.
- 2,000 events from eight concurrent writers; 32 concurrent workspaces plus 24
  immediate follow-ups, then restart and exact completion/usage checks.
- Native plugin schema/path/size validation, built-in and custom installs,
  installed workflow instructions reaching a model, actual activated completion
  hooks, inert MCP registration, revocation on removal, edited-file preservation,
  interrupted-journal cleanup, legacy preservation, stale/project/hash/trust
  checks, active-workspace exclusion, conflicting names and full catalogs.
- Native tool execution, stale edits, ambiguous/malformed multi-file patches,
  CRLF and missing final newlines, partial-write checkpoint recovery, and rewind
  conflict preflight that preserves unrelated edits.
- Approval isolation, expiry, cancellation, aborted futures, read-only tasks,
  queued cancellation, hung providers, and managed shutdown.
- Queued-only cancellation refuses tasks that have already started and preserves
  their running state. Task startup and cancellation share a transition lock.
  Active work remains listed behind 10,005 newer completed jobs; conversation
  selection prefers running work, then FIFO waiting work, then actual completion
  time, including when a newer submission was cancelled before its predecessor.
- Tool-call/result pairing across compaction and recovery, live file attachments,
  verification retries, token limits, and labelled estimates for absent usage.
- Smaller response reserves for constrained models, enforced in the actual
  provider request. Required input remains intact, 4K goal verification still
  passes with the expanded tool catalog, and oversized input fails before HTTP.
- Native SQLite table/query tools, typed results and bound parameters, denied
  SQL writes/attachments/extensions/multiple statements, symlink/sidecar/FIFO
  confinement, held-directory behavior after a rename, explicit row/byte limits,
  VM/time limits, cancellation and lock release. An independent Node SQLite
  writer verifies live committed WAL reads, recovery without a shared-memory
  file and reads after a writer closes, with unchanged database/WAL bytes.
  Wide existing schemas remain readable through projected columns; oversized
  results, exclusive locks and corrupt files produce bounded errors. Read-only
  model tools and the official MCP client share the same reader. Successful SQL
  reads satisfy the task's inspection check; rejected queries do not.
- Streaming UTF-8/SSE/NDJSON, malformed/truncated provider responses, large frame
  batches, bounded process output, concurrent commands, and process-group cleanup.
- Ollama templates receive later runtime repair/compaction guidance in their
  leading system block, with original positions labelled; user/tool data keeps
  its original role, tool arguments retain Ollama's native representation, and
  stored conversation history remains chronological.
- Application commands for onboarding, trusted/read-only projects, credentials,
  sessions, fresh-task approvals, and deletion after a project folder is removed.
- Nonempty session listing and literal search across titles and saved prompts,
  including `%`, `_`, `!`, and backslash characters.
- Manual terminal/agent workspace exclusion, cancellation of a terminal's child
  process during shutdown, and attribution to its original session after navigation.
- Git staging/discard of individual hunks, stale-hunk rejection, literal filenames,
  untracked and binary previews, and files without a final newline.
- Export of 10,005 events without silently truncating history, ordered pagination,
  and snapshot cursors that cannot skip an event arriving during session loading.
- Goal task execution and actual file/command verification, prevention of duplicate
  runs and cross-goal milestone edits, pause and resume without repeating done
  milestones, missing/failed verification, required inspection, shutdown, and
  recovery that preserves completed work without replaying interrupted commands.
- Provider/endpoint-scoped model discovery, preserved credential references and
  context limits, ambiguous name rejection, alias collision protection, queued
  routing snapshots, explicit overrides, visible missing-model fallback, unchanged
  planning permissions, and goal verification through the Test model. A provider
  failure is checked to never send the task to the default endpoint; invalid
  settings are checked before writing secrets.
- Background permission checks, logs without newlines, split UTF-8 output,
  output floods with a bounded live tail, graceful/forced cancellation, child
  cleanup, concurrent stop calls, per-project/global limits, project isolation,
  coexistence with agent tasks, and shutdown. Restart/import tests preserve the
  original legacy database and verify that an unrelated process with a stored PID
  is never signalled. Active processes remain visible past 100 newer history
  entries; failed audit storage prevents spawning an unrecorded command.
- Agent background tools verify scoped start/stop approval and denial, cancelled
  pending starts, project isolation before log disclosure, root/network/read-only
  restrictions, strict arguments, command hooks and invalidated file observations.
  UTF-8 previews and recent history are bounded while retaining every active
  process. Deleted origin conversations do not break process cleanup or history.
  Scripted model loops verify shared panel state, read-only inspection, continued
  project-process lifetime after task completion/cancellation, and shutdown cleanup.

- Project workflow discovery and ambiguous/invalid metadata, confined paths,
  literal bounded argument expansion, selected-only model context, read-only
  mode enforcement, queued source snapshots, source/hash events, project notes,
  terminal exit status and durable cards, session branching/resume/pins,
  untrusted/cross-project rejection, latest-checkpoint rewind, and stale edits.
- Private local control transport: socket permissions, protocol/size rejection,
  preservation of existing files and live endpoints, per-client project/session
  isolation, cleanup after a manual-command client disconnects, bounded idle
  connection shutdown, and temporary-owner restrictions with profile-lock release.
- Lifecycle commands: inert discovery, explicit workspace/hash activation,
  symlink rejection, changed/missing definitions, root/network/read-only checks,
  command/commit gates, literal filenames, one formatter invocation per patch
  path, invalidated file observations and checkpoint conflicts. Real task loops
  cover completion repair/exhaustion, bounded repair context from multiple noisy
  checks, queued manifest changes, compaction and
  provider failure. Output floods, timeouts, cancellation and engine shutdown
  assert that recorded parent/child processes actually stop.
- MCP stdio negotiation, framing/catalog bounds, exact arguments, error results,
  denied sampling/elicitation, inert registration, workspace/hash activation,
  literal environment privacy, single-pass credential redaction, exact tool-call
  approval and denial, stale definitions during approval, read-only exclusion,
  connection reuse and closed-catalog rejection. Engine success, provider errors,
  step limits, cancellation and shutdown all wait for process cleanup. The test
  peer is Node-based test infrastructure, not a bundled application dependency.

- Streamable HTTP modern discovery and legacy session negotiation, Unicode
  protocol headers, fragmented SSE, bearer credential references/redaction,
  remote-network activation, redirect rejection, expired sessions without retry,
  bounded/malformed/truncated responses, private discovery errors, abandoned
  initialization, and stream/session cleanup at task completion or cancellation.
- Native MCP server negotiation with the official SDK, tools/resources/prompts,
  fixed-project reads and writes, session filtering before limits, trust and
  per-task permission ceilings, ownership of delegated tasks and exact approvals,
  real approved execution, connection cancellation/EOF/abandoned-future cleanup,
  malformed/truncated/oversized frames and input floods. A separate shared task
  is checked to continue after the MCP connection stops.
- Engine-side task ownership over a private socket: bounded owner capacity with
  room for ordinary control requests, recovery after invalid task submissions,
  cleanup after completed history deletion, queued cancellation behind unrelated
  work, aborted control handlers, unread submission replies, malformed frames
  and cross-project submission rejection.
- Bounded project inspection, ignored dependencies, symlink/FIFO exclusion,
  content/depth limits, nested manifests and ambiguous test-command selection.
  Generated memory sections preserve user notes and reject stale/malformed maps.
  Diagnostics distinguish actual checks from untested model connectivity; only
  the explicit connectivity probe contacts the model. Git history handles literal
  pathspecs, linked worktrees and repositories without a first commit.
- Model-free test jobs retain exact command approval even under shell auto-approval,
  reject read-only requests, preserve stdout and real exit status on failure,
  run completion hooks once without model repair, and stop timed-out command
  children. A failed completion hook retains the successful command's output
  while correctly failing the overall task. MCP read-only access cannot save maps,
  start test jobs or approve another connection's task.
- Task-note persistence across restart, migration backup, legacy-file preservation,
  concurrent appends without lost notes, stale replacement rejection, explicit
  clearing without resurrecting legacy text, cross-project/read-only/busy-workspace
  rejection, UTF-8 limits and symlink/special-file exclusion. Branches retain full
  notes independently after source deletion; continuation sees labelled excerpts,
  and JSON/Markdown exports retain the complete saved notes. MCP tests verify
  fixed-project task IDs and write grants for append/replacement.

`scripts/test-native-mcp-server.mjs` exercises the real executable without a
display or Python. Its first four scripted model requests perform a read, file
write, approved terminal verification and completion. The probe checks checkpoint
restoration, all four resources, both prompts, read-only defaults, registration with stable
paths, JSON-RPC-only stdout, EOF cleanup and profile restart. Two further requests
run an unrelated detached task and an approved long-running terminal command on
a shared engine. SIGKILL of the MCP gateway is checked to cancel its running/queued
jobs and stop the command child, without stopping the unrelated task or invoking
the model for cancelled queued work. The six-request probe passes locally; CI
is configured to repeat it against the source binary and packaged AppImage.
The seventeen-tool catalog includes project inspection, native diagnostics, SQLite and
approved test execution, which the probe exercises without extra model requests.
These fixtures do not substitute for broader client and real-model interoperability.

The same executable probe now passes with `SHADOW_MCP_TRANSPORT=http`: six
scripted model requests, modern per-request negotiation and legacy initialization,
mandatory authentication, invalid bind/credential rejection, read-only defaults,
real edits/tests, exact approvals, checkpoint rollback, and gateway SIGKILL cleanup.
Each HTTP request opens a fresh connection; stdin EOF preserves the gateway and
its delegated jobs. SIGTERM closes temporary owners cleanly. CI repeats both
transports against the native binary and packaged AppImage.

Five Rust HTTP-server tests additionally exercise official-SDK interoperability,
project and owner isolation, duplicate/mismatched metadata, invalid credentials,
browser origins and hosts, malformed and oversized bodies, slow partial requests,
reconnects, queued cancellation, child cleanup, and abandoned-future profile/socket
release. Dependency-notice generation passes with 540 application dependencies,
including the HTTP server's newly resolved `httpdate` dependency.

Client registration is checked through the executable for generic, Claude Code,
Cursor and Codex output, both transports, permission flags, incompatible options
and credential non-disclosure. Rust tests parse emitted JSON/TOML to round-trip
paths containing quotes, newlines, backslashes and Unicode, reject client-side
path interpolation, and reject ambiguous or nonlocal gateway URLs.

`scripts/test-native-mcp-peer.mjs` uses the independently installed official
TypeScript SDKs **1.30.0** and **2.0.0**. All four SDK/transport combinations pass
catalog/resource/prompt discovery, structured tool results, approved native test
execution, actual stdout/exit status, shutdown and profile reopening. The packages
are pinned under `scripts/native-mcp-peer/` and used only for development tests;
Node and these clients are not included in the application. CI repeats the matrix
against the source executable and AppImage.

```sh
npm --prefix scripts/native-mcp-peer ci --ignore-scripts --no-audit --no-fund
node scripts/test-native-mcp-peer.mjs
```

Real Ollama coding tasks also pass through the external SDK: **gpt-oss:20b** over
v2 HTTP and **qwen3:14b** over v1 stdio. Each inspected a broken JavaScript total,
changed only the implementation, requested the exact `node --test totals.test.mjs`
approval, and completed successfully. The probe independently reran all three
tests, checked that the tests were unchanged, and restored the original source
byte for byte through the MCP checkpoint API. GPT-OSS used four steps and 8,066
reported tokens; Qwen used four steps and 9,365. These are bounded functionality
checks, not a general model-quality or performance benchmark.

```sh
SHADOW_MCP_MODEL=gpt-oss:20b SHADOW_MCP_PAIR=v2-http \
  SHADOW_MCP_PEER_ARTIFACTS=artifacts/native-mcp-peer-gpt-oss \
  node scripts/test-native-mcp-peer.mjs
SHADOW_MCP_MODEL=qwen3:14b SHADOW_MCP_PAIR=v1-stdio \
  SHADOW_MCP_PEER_ARTIFACTS=artifacts/native-mcp-peer-qwen \
  node scripts/test-native-mcp-peer.mjs
```

Each run uses a disposable project/profile and closes its native owner. Per-pair
reports retain job outcomes, approvals and paginated durable events. The final
matrix additionally verifies that the SDK's stdio owner process has exited.

The host's distro `rustdoc` needs its LLVM library directory in the loader path
for doc tests. The full suite was run with:

```sh
LD_LIBRARY_PATH=/usr/lib/rustlib/x86_64-unknown-linux-gnu/lib cargo test --workspace --locked
```

The pinned CI toolchain does not require this host-specific workaround.

## Native project plugins

Seven Rust integration tests cover validation and the complete bundle lifecycle.
The native window additionally installs/removes the Python built-in, reviews and
imports a custom bundle, checks inactive hooks, enables/disables its hook through
Settings, runs its installed review skill through the model and file tools, and
removes it after a local edit. The edited skill remains visible in the removal
report and on disk; the unchanged hook disappears. Preview receives keyboard
focus, and light/dark/compact plugin screens pass Axe. The full window run now
records **29 scripted model requests and 21 passing accessibility views**.

The native CLI performs built-in and local-file installs with reviewed hashes,
checks skill discovery and stale-hash refusal, and verifies edited-file retention.
The [plugin guide](NATIVE_PLUGINS.md) documents the supported schema, example
bundle, limits, separate executable activation and partial-install recovery.
Legacy Python bundles remain on disk and are explicitly reported for conversion.

## Sustained native-engine stress

`node scripts/test-native-stress.mjs` runs the native executable without a display
against a disposable profile and scripted compatible provider. Fifty batches
submit one task to each of four projects: **200 completed tasks / 405 provider
requests**, including actual bounded reads of 105 KB files and roughly 32 KB
assistant answers. Five deliberately stalled provider requests are cancelled.
Ten manual subprocesses each produce 1 MiB of output; each must exit successfully
and report truncated, nonempty bounded output.

The final local run sampled engine RSS at 83,464–85,352 KiB, with 84,908 KiB after
the subprocess phase: 828 KiB above the 40-task warm-up sample. All samples had
14 descriptors and 21 threads; no engine-owned child processes remained at the
sample points. SQLite integrity passes, exactly 200 completed and five cancelled
jobs persist with no active jobs, and an early job remains readable after clean
shutdown and reopening the profile. CI runs the same probe and uploads its JSON
measurements or failure diagnostics under `native-stress-diagnostics`.

The regression allows less than 64 MiB RSS growth after warm-up, sampled RSS below
384 MiB, and at most twelve additional descriptors. These are regression bounds,
not a general memory guarantee. Samples measure the engine process, not WebKit,
model-server memory, or transient peaks between samples. Large repository maps,
long individual conversations, desktop memory and longer-duration soaks remain
separate release checks. `SHADOW_STRESS_BINARY` must point directly to the native
ELF executable; an AppImage launcher is deliberately rejected for RSS attribution.

## Native terminal interface

The initial `shadowcode tui` frontend passes a real-PTY probe with **8 scripted
model requests** (`scripts/test-native-tui.mjs`). It verifies bracketed multiline
Unicode paste without accidental submission, local file-tool evidence, an attached
CLI, approval remaining pending while typing `y`, F4/Tab/Enter approval of the exact
command with a real file effect, read-only planning, cancellation on quit and
restored terminal settings. A second terminal attaches to `serve`; its queued task
is cancelled on quit while an unrelated running task survives. Artifacts are in
`artifacts/native-tui/`. CI includes source and AppImage variants; this local probe
used the development executable, not a newly packaged AppImage.

Eight added Rust tests cover bounded/escaped transcripts, grapheme editing,
Unicode wrapping and small screens, safe approval selection, queue-full draft
retention, workflow ownership and exclusive older-history pages. Clippy and the
full 216-test suite pass. The existing 15 CLI groups / 30 model requests also pass.
Dependency notice generation now covers 575 application dependencies. Terminal
picker/navigation, visual and sustained stress coverage remain open. Two further
regression tests verify compact-dialog scrolling/resizing and trust confirmation
bound to the displayed project; all ten library tests pass after that fix. The
real-PTY probe now resizes through 30×8, 60×18, 80×18, 81×18, 110×40 and
110×32, checks the minimum-size warning, scrolls compact help to the final line
and back with Home, and preserves the exact multiline Unicode draft through
these transitions. Assertions inspect fresh complete frames because incremental
ANSI updates can split words. This covers resize recovery; broader picker
navigation, visual review and sustained terminal stress remain open.

## Reviewed copy of uncommitted work

Twelve real-Git worktree tests pass. Copy scenarios verify distinct staged and
unstaged text and binary changes, intent-to-add entries, untracked executable
permissions, ignored-file exclusion, unchanged source contents/index semantics,
stale untracked-content rejection, untracked symlink rejection and prompt
handling of a FIFO excluded by Git. A refused destination reservation retains a
`needs_attention` record and the partial checkout without resetting source edits.
The native executable passes 20 CLI scenario groups / 32 scripted model requests,
including separate patch review, stale-hash refusal, copied file contents and
source/destination staging checks. Workspace Clippy and the native build pass.
Desktop copy controls now have native-window coverage; damaged-checkout repair
remains open. Bounded Git-visible copy coverage does not claim arbitrary
filesystem or submodule replication.

## Terminal input after resize

CI run `35544397210` failed the terminal probe after Help stayed open and the
following Enter did not submit the draft. Its retained ANSI recording showed
the Help overlay still present. A local full-color run reproduced a related
Home/resize timeout, which the previous inherited `NO_COLOR` environment had
not exposed. Crossterm now uses its level-triggered `use-dev-tty` input backend
instead of the edge-triggered Mio source, avoiding dropped readiness across
simultaneous input and resize notifications. The PTY fixture explicitly enables
truecolor and waits for the visible composer cursor after Escape before sending
another key.

Three consecutive full-color PTY runs pass, each with eight scripted model
requests, resize/help navigation, approvals, cancellation and terminal restoration.
The native build, library tests and workspace Clippy pass. Dependency notice
generation includes the new locked `filedescriptor` dependency and now covers
576 application dependencies. These local results address the observed failure;
the replacement CI run must still complete before its result is claimed.

## Reviewed return of worktree commits

Nine worktree tests pass. Return-specific scenarios exercise diverged branches,
source HEAD preservation until a separate commit, stale hashes after source
commits, dirty/untracked files, active worktree reservations, source background
processes, already-integrated commits, conflicts retained for manual resolution
or abort, ignored-file collisions and changed source repository roots. Testing
found that Git's `--no-overwrite-ignore` alone did not prevent an ignored-file
overwrite in this merge path; explicit incoming-path/ignored-path intersection
checks now reject it before mutation. Original worktree commits remain intact.
The native CLI passes 19 scenario groups / 32 scripted model requests, including
review, stale-hash rejection, uncommitted return and a separate merge commit.
Clippy and the native build pass. Desktop return is verified separately below.

## Desktop worktree return review

The native Settings panel shows the incoming diff, exact source and worktree
commits, source path, and explicit no-commit/conflict behavior. UI tests cover
focused review, exact source/ID/hash submission and a conflict response displayed
as an error rather than success. All 25 UI tests and seven browser scenarios pass.
The native-window probe commits an incoming file in an isolated checkout, opens
its desktop review, prepares the merge, verifies the returned file and unchanged
source HEAD, and explicitly aborts through Git. The source-open control becomes
available after the return. Light/dark/compact accessibility reports contain zero
violations; the captured return review is visually inspected. This UI test does
not claim automatic conflict resolution or automatic committing.

## Missing-worktree committed recovery

Six worktree tests pass, including a real Git checkout with an unmerged commit
and staged changes moved away from its managed path. Recovery refuses existing
paths, locked registrations and stale hashes. Reviewed restoration creates a
separate checkout at the retained commit; the original index and recovery record
are byte-for-byte unchanged, and the original branch, registration and moved
files remain. The native executable passes 18 CLI scenario groups / 32 scripted
model requests, including recovery preview, stale-hash rejection and restoration.
Clippy and the native build pass. This restores committed work only; automatic
reconstruction of missing uncommitted files is not claimed. Damaged-path repair
remains open; desktop rescue is verified separately below.

## Desktop recovery review

Settings exposes the missing-checkout recovery review and an explicit restore
button. The UI regression verifies displayed-source binding, focused review,
visible uncommitted-file limitations, exact hash submission and review clearing
after stale-state rejection. All 24 UI tests and seven browser scenarios pass.
The real native-window probe moves a disposable checkout away, reviews recovery,
restores into a new checkout, verifies committed file contents and checks the
original record and branch remain intact. Light, dark and compact accessibility
reports have zero violations, and the captured recovery screen is inspected.
The test waits for the persisted `ready` state before inspecting checkout files;
a `creating` recovery record intentionally precedes Git's filesystem writes.

## Desktop worktree controls

The native Settings panel creates and lists worktrees, opens the normal trust
prompt, and presents a focused removal review with exact path, branch, commit and
status. Requests bind to the displayed source project. Five service tests include
rejection after project selection changes; all 23 UI tests and seven browser
scenarios pass. UI regressions cover dirty-removal blocking, exact review hashes,
stale-review errors and explicit project opening. The native-window probe creates
and removes a real Git worktree, cancels its trust prompt, verifies retained branch
identity, and checks light/dark/compact accessibility. The worktree screenshot is
inspected for layout. Clippy and both frontend/native builds pass. Dirty-state
transfer, reviewed return of changes and damaged-checkout recovery remain open.

## Reviewed worktree removal

Five worktree tests now include locked/detached checkouts, staged and unstaged
changes, untracked and ignored files, stale review hashes, busy task reservations,
background servers, forged record paths and symlink replacement. Clean removal
retains an unmerged branch commit, preserves source files and archives the recovery
record. Eleven library tests include an atomic idle-workspace reservation that
blocks new background starts and releases correctly. The existing 16 background
process/tool tests pass. The executable's 17 CLI groups / 32 scripted model requests
now exercise inspection, rejected stale removal, successful removal, empty active
inventory and retained branch identity. Clippy and the native build pass. Missing/damaged-checkout recovery remains open; desktop controls are verified
separately above.

## Isolated worktree creation foundation

Three real-Git service tests verify creation at a selected commit, an independent
branch, unchanged staged/unstaged/untracked source content, disabled checkout
hooks, unchanged project selection, and rejection of untrusted/read-only/busy or
nested workspaces and invalid references. A deliberately blocked checkout filter
is cancelled; its process exits and a `needs_attention` recovery record survives
with source contents intact. The native CLI passes **17 scenario groups with
32 scripted model requests**, including creation, inventory, explicit trust and a
model file-inspection task inside the isolated checkout. Clippy and the executable
build pass. This proves the creation foundation; dirty-state transfer, missing-checkout
recovery controls remain open. Reviewed clean
removal is verified separately above.

## Compact desktop job polling

A 150-job stress fixture stores long Unicode prompts and large saved results. The
polling projection returns 100 recent records plus two older active records in
less than 200 KB, without copying result bodies or completion summaries. Full
prompt and result retrieval remains unchanged. All 18 foundation tests pass.
The UI's 21 tests include on-demand loading and caching of a complete queued
prompt while retaining its Test route label; all seven browser scenarios pass.
The real native-window workflow also passes with 29 scripted model requests,
including queued follow-ups, inherited results, reload, goals and accessibility.
Clippy, the interface build and native executable build pass. Sustained process
memory measurement and broader history virtualization remain separate gates.

## Full-history identifier resolution

The CLI history regression adds 10,050 newer conversations and 1,100 newer jobs
with more than 10 MB of unrelated result content to a disposable stopped profile.
Through a restarted shared engine, full and unique-prefix IDs still retrieve the
older job, rename/export the older conversation, and choose the correct default
conversation in a project whose history falls outside the old global list limit.
Ambiguous prefixes fail. The actual executable passes **16 CLI scenario groups
with 30 model requests**. The foundation suite passes all 17 tests, including
indexed ID resolution without deserializing job payloads. Clippy and the native
executable build pass. General job-list payload sizing and desktop long-history
display remain separate stress gates.

## Native CLI

`scripts/test-native-cli.mjs` exercises the actual executable with `DISPLAY` and
`WAYLAND_DISPLAY` removed, an isolated profile and a scripted compatible model.
It checks project trust, invalid options before mutation, model registration,
task completion/usage, saved continuation, ordered event JSON, session search,
exact redirected and atomic file exports, checkpoint rewind, skills and goals.
It also enables a reviewed hook by its current hash, rejects missing/stale hashes,
checks its actual completion side effect, replays human/structured hook events,
and disables it. There are nine scenario groups with 27 scripted model requests.
Actual pseudo-terminal sessions cover approval, denial, and Ctrl-C while a
prompt is waiting. Noninteractive requests never silently grant approval.

The MCP update passes 11 CLI scenario groups with 30 scripted model requests.
It adds inert server registration, hidden environment values, exact-hash
activation, disable/removal, refusal without a terminal, and actual PTY approval
of a displayed server/tool/argument object. A real MCP subprocess returns the
expected result and its recorded process group is stopped before CLI exit.

The inspection update passes 12 CLI scenario groups with the same 30 model
requests. It adds read-only/saved project maps, structured and readable native
diagnostics, durable command responses and history argument validation.

The task-memory update passes 13 CLI scenario groups with the same 30 model
requests. It checks persistent task notes, readable output, parser validation,
stale replacement rejection and actual note inclusion in a continued model
request. The MCP executable probe also saves and reads notes for its completed
native test task, without additional model calls.

The SQLite update adds a fourteenth CLI scenario group covering native table
discovery, parameter binding, denied SQL writes and input validation, without
extra model calls. This run recorded 29 scripted requests; cancellation timing
can stop one of the earlier requests before it reaches the fixture. The
executable MCP probe also queries and rejects a write
through its default read-only server. Dependency notice collection resolves
539 application dependencies with the updated SQLite library.

The plugin update passes **15 CLI scenario groups with 30 scripted model
requests**. It adds built-in and custom bundle preview/install, stale-hash refusal,
installed skill discovery, clean removal, and removal that preserves user edits.

The probe also hosts `serve`, starts and stops background commands, reads their
logs, runs a task alongside them, observes and cancels detached work, and approves
a waiting task from another CLI. SIGINT/SIGTERM/SIGHUP, a closed stdout pipe, remote
manual-command disconnect, and owner shutdown are checked for task/process
cleanup. Stopping an observer leaves the existing job running. Invalid
interactive goal options create no goal. Reports go to `artifacts/native-cli/`;
the same probe runs against the packaged AppImage in `artifacts/native-package/cli/`.
The package checks caught an argument-parser compatibility regression in
`shadowcode ui --version` and an AppImage wrapper signal that left its native
child running. The CLI now preserves that version invocation, and CLI/desktop
lifecycles observe the extraction wrapper so its loss triggers managed cleanup.

## Native window

The Rust/Tauri executable runs the embedded interface through IPC without a
Python service or browser launcher. `scripts/test-native-desktop.mjs` exercises
the actual WebKit window under Xvfb with an isolated profile and a scripted
compatible model. It submits a task, approves a terminal command, independently
checks the file written by the agent, reloads the saved conversation, cancels a
stalled request, and closes the application while a terminal child is running.
The test verifies process cleanup and captures light, dark, approval, completion,
and compact-window screenshots for inspection.

The first window run exposed a SQLite LIKE escape bug that appeared only after
sessions existed. A regression now covers real session listing and literal
search. Visual inspection also found a cancellation transcript race and an open
sidebar obscuring a resized compact window; the window test checks both.
Axe WCAG 2 A/AA and 2.1 AA checks pass in light, dark, compact, Goals, Router,
Background, Skills, Hooks, MCP, HTTP MCP, Inspection, and Diagnostics workspace views.
The compact check caught the Review button losing its accessible name
when its text was hidden; the control now retains an explicit label.
The native test also creates a goal through the drawer, completes all three
milestones, approves its verification command, checks the resulting file and
live transcript, and pauses a second goal during a stalled model request.
It also resumes that goal after deleting its old conversation, checking that a
fresh conversation is created and remains cancellable.
The native window test saves and enables a Build route through the drawer,
confirms the chosen model in actual requests and the conversation, reloads its
notice without duplication, and checks a visible default-model fallback for a
missing saved route. The Router screenshot is inspected alongside the existing
workspace views.
The window test starts a background process through the form, inspects live and
retained output, performs a coding task while it runs, and stops it through the
drawer. It then closes the application with another background process and
terminal child running, checking that the app and both managed process groups exit.
The window also creates a skill through its editor, expands it from the Skills
panel into the composer, passes arguments, executes a real read-only tool loop,
and checks its source path and effective mode in the conversation. Reload keeps
both the skill provenance and a subsequent `/status` command card. Controlled
input filling explicitly dispatches the input event before WebDriver typing
and sends Return keys for multiline text. WebKit's clear operation alone can
otherwise retain the old React value.
It invokes a headless CLI in a second project while that real desktop owns the
engine, creates a conversation, starts/reads/stops a background process, and
checks that the desktop's project, remembered project and original background
process remain unchanged.
The window enables a reviewed completion hook in Settings, checks the actual
command side effect, reloads its result once, and disables it. This exposed a
duplicate final answer when a hook card followed the model's response; completion
now matches the last answer within the same task while retaining failure notices.
The hook Settings screenshot was inspected for command readability and spacing.
The inspection update exercises `/understand` and `/doctor` as durable command
cards and `/health` as the native diagnostics panel, with twenty total scripted
model requests. The window check caught `/doctor` being intercepted by an older
panel shortcut and the Health panel showing unperformed checks as failures.
Native commands now retain their diagnostic report, while Health distinguishes
passes, warnings, failures and unperformed checks with readable details. The
unsupported native Auto-fix control is absent; automatic repair remains a release
requirement. Project maps render as Markdown, and command details wrap when the
Health panel narrows the conversation.
The window now saves task notes through `/memory --task` and verifies the durable
card. The inspection milestone's clean-runner CI found a goal stalled at a pending
approval. Desktop decisions now carry the displayed approval's conversation ID;
an explicit regression changes the backend selection before clicking Allow and
checks that the correct goal approval resolves and all milestones finish. A
late approval refresh cannot replace another selected conversation's approvals.
The subsequent [clean-runner check](https://github.com/ShadowfetchLinux/ShadowCode/actions/runs/35530082798)
passed Rust, CLI and MCP checks but caught a notification mid-fade at insufficient
contrast. Toasts now animate position with fully opaque text. The rebuilt native
window passes all twelve accessibility views and the complete twenty-request
workflow locally, including the changed-selection goal approval regression.
Twenty interface unit tests cover ordered replay, pagination, stream finalization,
listener cleanup, interruption, native tool cards, routing/fallback replay,
workflow provenance, durable command/hook cards, completion checks between
answers and task results, and disambiguated model labels that omit URL credentials. The seven existing browser
tests continue to pass through the legacy transport.

The queued-follow-up update extends the real native-window probe to 22 scripted
model requests and 15 passing Axe views. While a model response is deliberately
held open, the UI queues two follow-ups, creates another conversation in the
same project, queues and cancels its message, returns to the original task and
reloads. The running task stays selected and visible; the queue survives.
The probe cancels a waiting message without interrupting the first task, then
releases it and observes the second task start automatically with its selected
model, read-only Review tools and the completed predecessor's conversation.
Cancelled messages never contact the model. Queue screenshots and accessibility
reports cover light, dark and compact layouts; the sidebar returns to idle after
completion. UI unit tests also cover interleaved queue cancellation without
resetting the running task's plan/usage, and prompt ordering without duplication.

The model-background update extends that workflow to **27 scripted requests and
18 passing Axe views**. The actual window displays the exact start command and
independent process lifetime before approval, shows the model's process in the
shared Background panel after its task completes, reads its log, and requires
another exact-process approval to stop it. Parent and child exit are checked
immediately after Stop. Approval screenshots cover light, dark and compact
windows. The probe exposed a light-theme warning-label contrast failure and a
scroll timing issue that could leave new approvals below the viewport; both are
fixed, and the final test checks that the complete approval remains visible.
All 20 interface unit tests and seven browser tests pass with these changes.

The earlier twenty-request window workflow also passed from the local optimized
AppImage in FUSE-free extraction mode with the legacy `ui` argument. The package
checker confirms that AppImage and Debian packages contain native ELF application
code with matching versions and no Python runtime or sidecars. Packaging now
collects notices for 534 application dependencies, including the resolved Cargo
build/test graph and production npm graph. The local AppImage inventory records
156 system packages; that number depends on the build host's libraries and GTK
data. Unattributed system files stop packaging, and the extracted-package check
verifies the SHA-256 digest of every listed notice. Exact source package versions
are retained for corresponding-source release preparation.

The clean Ubuntu 24.04 [CI run for 5aaba82](https://github.com/ShadowfetchLinux/ShadowCode/actions/runs/35510335761)
passed all native/UI checks, built both formats, inspected their notices and
executable contents, and ran the actual packaged-window workflow. Those exact
development artifacts were downloaded to the development machine; their published
SHA-256 checksums matched, both packages passed inspection again, and the downloaded
AppImage passed the full scripted native-window workflow. The CI AppImage has
523 application dependencies and 113 system-package attributions; its fewer
system resources reflect the clean runner's environment. Artifacts and results
for that download are retained locally in `artifacts/native-ci-package/`.

The extraction lifetime regression now passes against the rebuilt AppImage.
Its source-built runtime gives simultaneous owners separate mode-0700 directories
under the same `TMPDIR`. Eight short clients preserve both owners' executable and
WebKit subprocess resources. SIGINT/SIGTERM/SIGHUP preserve native exit codes,
stop the recorded background process groups, and remove only the exiting
instance's directory. `nohup` remains effective in the wrapper and native child;
environment-based extraction keeps arguments intact; overlong paths fail with
cleanup. Process assertions fetch the running task's PID after asynchronous
startup and check that it is alive before testing shutdown.

The actual packaged window also passes three simultaneous default-profile
activations, confirming the same native PID and successful reload while only
that window's extraction directory remains. It uses disposable XDG storage and a
private DBus session. All existing window/CLI-sharing checks then run on that
window. This verifies repeated activation; broader window-state and desktop
integration behavior still needs its remaining checks.

The local runtime update passes 103 native integration tests, formatting/clippy,
eight CLI scenario groups with 27 scripted model requests, the actual debug
window, the packaged CLI, the packaged window, and package inspection. The
runtime-specific test is separate from those CLI/window scenarios. The checked
packages contain a 19,119,312-byte executable, an 84,429,304-byte AppImage, and an
8,853,760-byte Debian package. A separate 104,163,721-byte runtime source archive
contains the matching pinned source, patches, build recipes, and compiler
provenance. Inspection checks runtime machine code, 26 runtime notice/provenance
files, and the source archive hashes. Local results and checksums are under
`artifacts/native-package/`; CI runs the same package, runtime, CLI, and window
checks and uploads the matching source archive.

The source-built runtime milestone also passed the complete clean-runner
[CI run for a5c6b88](https://github.com/ShadowfetchLinux/ShadowCode/actions/runs/35518558352),
including both package formats, runtime regressions, packaged CLI, and the
packaged native window.

Source retrieval now verifies each candidate before publishing an archive, uses
an identical-byte Alpine mirror for zlib, and rechecks retained sources for
rebuilds. The local HTTP regression covers wrong successful responses, errors,
redirects, exhausted candidates, corrupt retained files, and temporary-file
cleanup. It never changes the pinned checksum to accommodate a failed download.

The September 20 source-retrieval update passes package inspection for both
formats: a 19,324,112-byte executable, an 84,498,936-byte AppImage, and an
8,943,832-byte Debian package. The 104,164,476-byte runtime source archive was
rebuilt with container networking disabled using the already-installed pinned
toolchain. Its runtime machine code matches the packaged code. The report's
archive checksum matches the final packaged source archive; results are in
`artifacts/native-package/source-rebuild/result.json`. CI now runs the same
download-failure and offline-rebuild checks before the existing packaged-runtime,
CLI, and window checks. This verifies runtime sources; corresponding sources for
the other redistributed components remain required.

Profile locks explicitly unlock when their last native owner is dropped. This
prevents an unrelated fork from briefly retaining the lock until it reaches
`exec`, which could reject an immediate restart after clean shutdown. A
deterministic fork regression reproduces that failure in the old implementation;
the shared guard still keeps the profile locked while owned background cleanup
is running. A forked copy cannot unlock its parent's live guard.

The stdio MCP integration and prior runtime/profile-lock changes passed the
complete [CI run for 2f2d61b](https://github.com/ShadowfetchLinux/ShadowCode/actions/runs/35523898335),
including clean-runner compilation, both package formats, offline runtime source
rebuild, runtime lifecycle stress, and the packaged CLI/window. HTTP transport
was added afterward; this green run does not certify that later change.

OS dialog interaction, notification delivery, broader stress/accessibility
coverage, corresponding sources for remaining redistributed components,
and the final installed release still need their release-gate checks.

The MCP window update passes 20 scripted model requests and ten Axe views. It
registers stdio and HTTP servers through Settings, confirms registration and
activation are inert, displays exact call arguments, approves actual subprocess
and authenticated HTTP/SSE requests, and checks that redacted results reach the
model. It waits for cleanup and disables/removes the registrations. Visual
review uses `artifacts/native/mcp.png` and `artifacts/native/mcp-http.png`; the
complete run is recorded in `artifacts/native/result.json`. The test exposed
notifications covering Close and Send controls; dialogs now stay above transient
messages and notification bodies no longer intercept clicks. Registration forms
collapse after adding a server so reviewed connection details stay in view.
The HTTP extension initially used an incorrect test button label; the probe now
uses the same approval control as the existing stdio workflow.

The application notice inventory now covers 538 dependencies. `sse-stream` 0.2.6
is pinned to the SDK's tested version; its omitted upstream license texts are
retained at the exact source commit, with digests verified by the notice builder.

## Real local models

`native/core/examples/probe_background.rs` asks an installed model to start a
disposable Node HTTP server through the native background tool, then read its
retained log and report a unique readiness marker. The harness grants only the
exact requested start command, independently verifies the HTTP response after
the coding task completes, and verifies that shutdown closes the listener.

Both **gpt-oss:20b** and **qwen3:14b** passed: one scoped start approval, the
correct live-log marker, a running server after task completion, matching HTTP
content, and stopped server on shutdown. GPT-OSS used four model steps and 6,632
reported tokens; Qwen used three steps and 6,702 tokens. These isolated results
verify tool interoperability, not general model reliability or performance.

```sh
cargo run -p shadowcode-core --example probe_background --locked -- gpt-oss:20b background-probe.json
```

Node is infrastructure for this disposable test server and is not bundled with
the application. The optional JSON report records approvals, tool events,
actual model/usage, independent HTTP verification and shutdown outcome.

`native/core/examples/probe_sqlite.rs` creates an isolated profile and disposable
invoice database. The model must discover its schema and use the native SQLite
query tool to calculate the sum for paid invoices. The probe checks the recorded
query result, final task status and answer, and unchanged database bytes.

Both **gpt-oss:20b** and **qwen3:14b** completed this read-only task in three
model steps, with the correct total of 1,000 cents. Their reported token totals
were 2,829 and 3,456 respectively. Neither final run needed an inspection retry.
Earlier runs exposed missing schema-PRAGMA support and an inspection check that
failed to recognize successful SQLite tools. That check caused unnecessary
repair prompts, followed by empty model answers. Both defects are fixed and
covered by regression tests, including rejection of a failed query as evidence.
These are individual tool-interoperability probes, not performance benchmarks.

With either model already available in local Ollama, reproduce the probe with:

```sh
cargo run -p shadowcode-core --example probe_sqlite --locked -- gpt-oss:20b sqlite-probe.json
```

The optional report path records the actual model, results, usage and tool/
verification events. The disposable profile and project are removed on exit.

`scripts/probe-native-hooks.mjs` tests an installed Ollama model through the
headless executable with a disposable project/profile and an explicitly enabled
completion check. It independently reads the two required output files, verifies
that the hook definition did not change, and records hook exit codes and whether
repair was observed. Arbitrary shell requests still require approval. Run it
after building the debug executable:

```sh
node scripts/probe-native-hooks.mjs
SHADOW_HOOK_PROBE_MODEL=qwen3:14b node scripts/probe-native-hooks.mjs
```

An initial gpt-oss probe exhausted a 12-step budget after supplying empty file
hashes and malformed patches. File tools now describe `missing` for a new file
and return actionable errors for invalid hashes without weakening stale-write
checks. With this guidance and a 20-step cap, the subsequent gpt-oss run created
both files and passed its completion check with 11,142 reported tokens. It had
inspected the hook and met its requirements before completing, so this run did
not exercise a failed-check repair. Its prose incorrectly claimed a commit;
the evidence here establishes file contents and the hook result, not a Git
commit. Model summaries still need to be checked against recorded operations.
Detailed local reports are in `artifacts/native-hooks-local/`.

The final gpt-oss probe with shell approval enabled attempted to execute a copy
of the check itself, despite being asked to use file tools. The CLI returned
`needs_approval` with exit code 2 and did not run that model-requested command.
Enabling a lifecycle hook does not grant unrelated agent shell calls approval.
This later run was not counted as a passing completion probe; model behavior
remains variable even when command permissions and recorded outcomes are correct.

The Qwen probe initially repeated its answer through all failed-check retries.
Inspection of the installed Qwen3 Ollama template showed that it renders the
leading system block but skips later system-role messages. The transport now
includes those runtime notes in the leading block with their original positions
labelled, while keeping tool/user data in their original roles. The rerun passed
in four model steps with 6,247 reported tokens: the hook failed with exit 2,
Qwen created the missing file, and the hook passed with exit 0. Both files and
the unchanged hook definition were independently verified.

`native/core/examples/probe_task.rs` creates a disposable Rust package and an
isolated ShadowCode profile. Each model must inspect a broken addition function,
make the minimal edit without changing package metadata or tests, and run
`cargo test --offline --lib` through an approval restricted to that command in
the disposable project. The probe independently repeats the assertions, runs a
read-only follow-up, checks actual fresh-file inspection, and rewinds the edit.

Both installed models, **Ollama gpt-oss:20b** and **qwen3:14b**, completed the
coding workflow. Qwen initially produced an ambiguous replacement and then an
invalid Rust edit. The tools rejected the ambiguity, the compiler exposed the
invalid edit, and a stale hash forced a fresh read before the model repaired it.
That run passed in 12 model steps with 27,188 reported tokens.
The final gpt-oss run passed in six model steps with 8,292 reported tokens.

Earlier probe runs exposed two harness issues: an overly strict test-only
approval rule rejected equivalent project-directory arguments, and unrestricted
`cargo test` hit the host's `rustdoc` loader problem. The probe now accepts only
equivalent project directories and exercises library tests explicitly. Another
run exposed a model answering a follow-up from conversation history instead of
reading the current file. Named-file requests now attach current native reads;
the probe asserts that fresh inspection actually occurred.

These small coding probes establish tool interoperability, recovery from a real
compiler failure, continuation, and rewind. They do not establish performance
on large repositories or complete native-window usability.

The local optimized AppImage was also exercised through WebKit WebDriver using
`scripts/probe-native-model.mjs` with both **gpt-oss:20b** and **qwen3:14b**.
Both models made the exact one-expression fix without altering the fixture's
tests or manifest. The probe clicked the native approval card for the bounded
test command, independently reran those unchanged tests, reloaded the saved
conversation, and submitted a read-only follow-up that inspected the current
file. It then clicked Stop after observing a real streamed model response,
verified persisted cancellation and its visible transcript, restored the first
task's checkpoint byte for byte through IPC, and checked native-process exit.

In these individual window runs, gpt-oss used six steps and 8,272 reported tokens
for coding plus continuation; Qwen used seven steps and 13,267 reported tokens.
Observed Stop-to-visible-cancellation times were 93 ms and 234 ms respectively.
These are single disposable-project observations, not performance benchmarks.
Screenshots, the saved event history, independent test output, scoped approval
decisions, and machine-readable results are retained in `artifacts/native-model/`.

Broader desktop/UI tests, integrations, release artifact checks, and installation
remain mandatory before release.


## Desktop reviewed copy and Git identity fixtures

The desktop copy review displays separate staged and unstaged patches, untracked
file names/sizes and intent-to-add entries. All 26 UI tests and seven browser
scenarios pass, including exact review-hash submission, keyboard focus and stale
copy rejection. The actual Rust/WebKit window suite passes with 29 model requests;
its new copy flow verifies the destination files and staging against the source
and proves the original files/index remain unchanged. Light, dark and compact
copy-review accessibility checks report zero violations; the screenshot was
visually inspected. Clippy, the native desktop build and 20 CLI scenario groups
(31 model requests in this run) also pass.

CI runs 35545837440 and 35546241039 failed the worktree return tests before this
fixture correction. Fixture commits supplied identity only to individual test
Git commands, while application-driven merges use the repository identity.
Fixtures now configure their disposable repositories explicitly, rather than
depending on the developer's global identity. Thirteen worktree tests pass,
including a regression that deliberately clears repository identity, verifies
return preserves source HEAD/files/index with an actionable error, then configures
identity and successfully retries. Replacement CI is still pending; local checks
are not a claim that the final release or remote pipeline is complete.


The first full Rust rerun exposed a timeout in the existing client-disconnect
fixture. Its PID file existence check could read between shell redirection and
`echo`, probing `/proc/stat` with an empty PID. The fixture now waits for a parsed,
nonzero PID before disconnecting and checking child cleanup.

The full workspace rerun after these corrections passes all **234 native tests**;
Clippy with warnings denied also passes.

## Reviewed managed Git connection repair

Fifteen worktree integration tests pass, including restoration of both missing
connection files at the original managed checkout, byte-for-byte preservation of
the original index, retained staged/unstaged/untracked work, unchanged source,
stale-review rejection and active-workspace exclusion. Foreign connections,
symlinks, locked worktrees and missing indexes are refused. Repair journals remain
private and separate from active worktree records. This is connection repair, not
reconstruction of missing files, indexes or moved/damaged administrative metadata.

Clippy with warnings denied, the native desktop build, 27 UI tests and seven
browser scenarios pass. The executable CLI passes 21 scenario groups with 31
model requests, including reviewed repair with stale-hash rejection and original
index/file preservation. The expanded CLI arguments use a boxed argument group
without changing existing option names or action conflicts.

The native Rust/WebKit window suite passes with 29 model requests. Its repair
flow deletes the disposable checkout's connection, reviews and restores it via
Settings, then checks exact original index bytes and staged/unstaged content.
Light, dark and compact repair views report zero accessibility violations; the
screenshot was visually inspected. The first harness attempt queried a card
before the asynchronous inventory loaded; it now waits for that exact card before
clicking, matching the existing return-flow readiness check. Broader moved-path
and lost-metadata recovery, final package/release checks and installation remain
open.

CI run 35546885309 passed the Rust suite, CLI, sustained stress, terminal and MCP
checks, then failed a native-window click on `Remove python-expert`: the backend
installation check completed before the refreshed button appeared. The window
harness now waits for a visible, enabled button with the exact label before
issuing one WebDriver click. It does not retry mutations or relax assertions.
The full native-window rerun with this readiness helper passes with 29 model
requests, including plugin installation/removal, worktree copy and connection
repair. The replacement GitHub pipeline remains unverified until it completes.

## Bounded native desktop saved-history pages

Twelve service-command tests pass. The history regression traverses every event
across byte-limited Unicode pages, excludes another session's interleaved events,
keeps the exclusive cursor stable after live appends, rejects invalid cursors,
and verifies oversized preview notices leave original events and exports intact.
The default session API retains its existing full-history contract.

All 31 UI tests and seven browser scenarios pass. Hook tests cover older/newer
cursor navigation, live streaming while reading an old page, return to the latest
state, late responses after session changes, retryable errors and reattachment
of an unchanged running-job ID after a refreshed snapshot. The latter previously
changed the event generation without forcing the stream effect to reattach.
Queued-task snapshot refreshes preserve the historical page being read. Clippy
with warnings denied and the native desktop build also pass.

The saved-page limits are 128 events, 2 MiB aggregate event payload and 256 KiB
per ordinary preview event. Explicit omission notices point to export for larger
individual events; originals are never rewritten. Long-running live transcript
memory and Markdown virtualization remain separate release gates.

The first expanded native-window run caught a worktree-copy publication race:
checkout creation briefly saved `ready` before applying the reviewed edits, so
an inventory reader could observe the original index as if copying had finished.
Copy creation now remains in its incomplete state until copy application and
verification publish readiness. All 15 worktree regressions pass after the fix;
the desktop test continues to require the copied index as soon as `ready` appears.


The expanded native-window run passes with 29 model requests and a disposable
12,000-event conversation. It traverses all 94 pages back to the first message,
keeps no more than 128 message elements in the DOM, navigates forward and returns
to the latest snapshot. Light, dark and compact history checks report zero
accessibility violations; the rendered history screen was visually inspected.
Visual review also prompted the floating Latest activity control to return from
historical browsing to live messages, matching the top Latest messages action.

The full window rerun also verifies both Latest messages and the floating Latest
activity control return to the current snapshot. The large-event notice names
JSON export explicitly: the command palette now exposes **Export this task as
JSON**, while Markdown remains the readable summary. The browser regression
checks this action selects `format=json`; native OS save-dialog interaction
remains in the separate desktop integration gate.

## Native stream catch-up rendering

Adjacent text fragments from one response are compacted within a fetched page
before transcript rendering, without changing stored events. The regression
suite compares original and compacted replay, Unicode content, duplicate rows,
response/task/metadata boundaries, bounded merging and a two-page completed job.
All 34 UI tests and the rebuilt native window suite passed locally. The native
window run includes the 12,000-event, 94-page history fixture and its three
accessibility layouts. This is rendering/replay evidence, not a measurement of
constant memory during an indefinitely growing live conversation.

## Desktop attached to a persistent engine

The complete Rust workspace passes 241 tests after adding four private-control
regressions: independent persistent view selection and clean detach, four-view
limits with EOF cleanup, refusal of temporary foreground owners, and concurrent
status reads during a slow manual command. Views share the existing engine but
do not change the owner's selection or cancel its durable work on disconnect.
The TUI owner mode is covered at the control-transport level.

All 34 UI tests, seven browser scenarios and workspace Clippy checks passed.
The rebuilt real native window suite made 30 fixture-model requests, including
an attachment phase against a separately launched headless owner. It verifies
the shared engine PID, independent desktop PID, terminal execution, visible
shared-lifetime notice and running-task controls. After closing the window, the
headless engine and its agent task remain running; the CLI cancels that task
explicitly. A reopened window also closes successfully after the owner exits.
The attachment screenshot was inspected and its accessibility check reported no
violations. Existing owner-desktop shutdown, worktree, plugin, MCP, queue, goal
and 94-page history scenarios still pass in the same run.

Attached completion notifications and automatic reconnection to a restarted
owner are not covered or implemented by this change. Actual GUI attachment to
an interactive TUI process is not part of this window fixture; it uses `serve`.
