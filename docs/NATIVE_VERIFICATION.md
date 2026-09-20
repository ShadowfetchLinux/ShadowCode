# Native engine verification — development branch

Recorded on 2026-09-20. This is evidence for the Rust engine under development,
not a claim that the native desktop release is complete. The full release gates
remain in [NATIVE_MIGRATION.md](NATIVE_MIGRATION.md).

## Automated checks

`cargo fmt --all --check`, Clippy with warnings denied, and all **69 native
integration tests** pass on the development machine. The suite covers:

- Config validation, private secrets, untrusted project overlays, profile locks,
  legacy SQLite backups and goal import, session branching, and durable replay.
- 2,000 events from eight concurrent writers; 32 concurrent workspaces plus 24
  immediate follow-ups, then restart and exact completion/usage checks.
- Native tool execution, stale edits, ambiguous/malformed multi-file patches,
  CRLF and missing final newlines, partial-write checkpoint recovery, and rewind
  conflict preflight that preserves unrelated edits.
- Approval isolation, expiry, cancellation, aborted futures, read-only tasks,
  queued cancellation, hung providers, and managed shutdown.
- Tool-call/result pairing across compaction and recovery, live file attachments,
  verification retries, token limits, and labelled estimates for absent usage.
- Streaming UTF-8/SSE/NDJSON, malformed/truncated provider responses, large frame
  batches, bounded process output, concurrent commands, and process-group cleanup.
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

The host's distro `rustdoc` needs its LLVM library directory in the loader path
for doc tests. The full suite was run with:

```sh
LD_LIBRARY_PATH=/usr/lib/rustlib/x86_64-unknown-linux-gnu/lib cargo test --workspace --locked
```

The pinned CI toolchain does not require this host-specific workaround.

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
Axe WCAG 2 A/AA and 2.1 AA checks pass in light, dark, compact, and Goals workspace
views. The compact check caught the Review button losing its accessible name
when its text was hidden; the control now retains an explicit label.
The native test also creates a goal through the drawer, completes all three
milestones, approves its verification command, checks the resulting file and
live transcript, and pauses a second goal during a stalled model request.
It also resumes that goal after deleting its old conversation, checking that a
fresh conversation is created and remains cancellable.
Eleven interface unit tests cover ordered replay, pagination, stream finalization,
listener cleanup, interruption, and native tool cards. The seven existing browser
tests continue to pass through the legacy transport.

The same native window workflow also passes when launched from the local release
AppImage in FUSE-free extraction mode with the legacy `ui` argument. The package
checker confirms that AppImage and Debian packages contain native ELF application
code with matching versions and no Python runtime or sidecars. Packaging now
collects notices for 523 application dependencies, including the resolved Cargo
build/test graph and production npm graph. The local AppImage inventory records
156 system packages; that number depends on the build host's libraries and GTK
data. Unattributed system files stop packaging, and the extracted-package check
verifies the SHA-256 digest of every listed notice. Exact source package versions
are retained for corresponding-source release preparation.

OS dialog interaction, notification delivery, default-profile single-instance
behavior, broader stress/accessibility coverage, real models through the native
window, corresponding-source artifacts, clean-runner package builds, and the final installed
release still need their release-gate checks.

## Real local models

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
on large repositories or complete native-window usability. Broader desktop/UI tests, integrations,
release artifact checks, and installation remain mandatory before release.
