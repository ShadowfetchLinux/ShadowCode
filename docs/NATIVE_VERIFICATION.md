# Native engine verification — development branch

Recorded on 2026-09-20. This is evidence for the Rust engine under development,
not a claim that the native desktop release is complete. The full release gates
remain in [NATIVE_MIGRATION.md](NATIVE_MIGRATION.md).

## Automated checks

`cargo fmt --all --check`, Clippy with warnings denied, and all **182 native
integration tests** pass on the development machine. The suite covers:

- Config validation, private secrets, untrusted project overlays, profile locks,
  legacy SQLite backups and goal import, session branching, and durable replay.
- Private native profile directories and lock permissions, migration of permissive
  existing directories without changing their contents or parent permissions,
  relocated profile parents, and rejection of symlink leaves, symlink/hard-linked
  locks, directories and FIFOs without changing their targets or hanging startup.
- 2,000 events from eight concurrent writers; 32 concurrent workspaces plus 24
  immediate follow-ups, then restart and exact completion/usage checks.
- Native tool execution, stale edits, ambiguous/malformed multi-file patches,
  CRLF and missing final newlines, partial-write checkpoint recovery, and rewind
  conflict preflight that preserves unrelated edits.
- Approval isolation, expiry, cancellation, aborted futures, read-only tasks,
  queued cancellation, hung providers, and managed shutdown.
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

The host's distro `rustdoc` needs its LLVM library directory in the loader path
for doc tests. The full suite was run with:

```sh
LD_LIBRARY_PATH=/usr/lib/rustlib/x86_64-unknown-linux-gnu/lib cargo test --workspace --locked
```

The pinned CI toolchain does not require this host-specific workaround.

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
Seventeen interface unit tests cover ordered replay, pagination, stream finalization,
listener cleanup, interruption, native tool cards, routing/fallback replay,
workflow provenance, durable command/hook cards, completion checks between
answers and task results, and disambiguated model labels that omit URL credentials. The seven existing browser
tests continue to pass through the legacy transport.

The same native window workflow also passes when launched from the local optimized
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
