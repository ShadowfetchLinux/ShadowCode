# ShadowCode 0.22 qualification

Qualification date: 2026-09-22. Linux x86_64, Rust 1.95.0 and Node 24.19.0.
This release builds on the recent symbol-index, worktree, steering, fork and
Guardian changes and corrects their integration and reliability issues.

Local application qualification below used the 0.22.0 candidate. Clean GitHub
CI then exposed a workflow ordering error: process tests ran before the real
desktop executable existed. Candidate 0.22.1 corrected that order and passed
native source, CLI, stress, terminal, MCP and desktop checks on GitHub. Packaging
then exposed missing Rustup discovery in the sanitized PATH. Release 0.22.2
preserves the selected Cargo home, with six packaging-environment regression
checks. These patches leave application behavior unchanged. The tag workflow
repeats verification before publishing; failed candidate tags are retained
without release downloads.

Installation of the published 0.22.2 package exposed the generic AppImageKit
launcher changing the working directory and injecting nonexistent language-runtime
paths. Version 0.22.3 uses a native launcher that preserves the caller directory,
relative projects/profiles and external Python tooling. Packaged runtime tests
cover these cases in both extraction modes, including directory names with spaces.
The desktop resolves user paths before selecting the bundled WebKit resource
directory; CLI commands retain the caller directory. Desktop checks start without
an explicit workspace and also exercise a relative isolated profile. Both installed
command aliases resolve to the same verified release.

## Verified behavior

- 343 native Rust tests cover the engine, permissions, cancellation, history,
  context budgets, tools, background processes, IPC, MCP and worktree workflows.
  Formatting and Clippy with warnings denied pass.
- 49 UI tests and seven browser workflows cover transcript replay, settings,
  dialogs, keyboard navigation, drafts, reconnects and accessibility.
- The actual Tauri/WebKit window passes worktree recovery, approvals, forks,
  queues, goals, integrations, engine attachment and managed shutdown checks.
  A 12,000-event conversation traverses all 94 history pages with a bounded DOM.
  Accessibility checks cover light, dark and compact layouts, including Advanced.
- 316 legacy Python tests pass. Python remains a development/test dependency
  for the legacy browser transport; it is not part of the native packages.
- The native CLI passes 21 scenario groups. Terminal checks use a real PTY.
  MCP checks cover stdio, authenticated HTTP and independent official TypeScript
  v1/v2 clients on both transports.
- A sustained native run completes 200 tasks, five cancellations and ten
  subprocess-output floods. Sampled engine RSS stays between 92,808 and 93,308
  KiB, with 14 open descriptors at each checkpoint. This is a scripted-provider
  engine measurement, not the memory used by a loaded model or desktop WebKit.
- Live `qwen3:14b` through Ollama calls `system_info` to answer a display-count
  question. A separate coding probe performs a minimal real edit, obtains exact
  shell approval, runs tests, continues in read-only mode and restores a checkpoint.
  The coding probe uses seven model steps and 18,272 reported tokens; results are
  evidence for this fixture, not a general model-quality benchmark.
- Package inspection checks matching native code and versions, dependency
  notices, runtime provenance and absence of bundled Python/Node runtimes.
  The AppImage runtime rebuilds from its shipped sources without network access.
  Concurrent extraction, activation, shutdown and installer checksum refusal
  are covered by the packaging checks.
- The final AppImage repeats the desktop walkthrough (including default-profile
  activation), all 21 CLI scenario groups, the terminal PTY checks and both MCP
  transports with official TypeScript v1/v2 client interoperability.

## Regression fixes

Scratch cleanup is restricted to directories owned by the current process.
Bubblewrap availability is checked before executing a command, and execution
failures are not retried outside isolation. Project mounts take precedence over
read-only home mounts, including commands started in a project subdirectory.

Symbol reads use workspace confinement and a private bounded cache. Tests cover
outside symlinks, deleted files, Unicode signatures, exact-name ranking and call
sites. Parallel plans persist per project; combined checks catch worker-to-worker
conflicts. Cleanup preserves branches and refuses dirty or busy checkouts.

Forks validate event ownership, preserve completed tool context and exclude later
turns. Guardian approvals are bound to the original summary, profile and workspace.
Ollama residency is validated, retained in saved models and encoded correctly.
Checkpoint rewind waits for a confirmed pause boundary, serializes against
resume, and refuses competing workspace or background activity. Regression
tests reject rewind during an in-flight command and retain completed side effects.
Attached-window closing interrupts owner discovery. Replacement connections reuse
the original event channel and disconnect flag, with tests for fresh completion
events and a second owner shutdown after reconnection.

The audit also fixed a stale Ctrl+N handler, shortcut modifier overlap and text
contrast. Happy DOM was upgraded to 20.14.5; the UI npm audit reports zero known
vulnerabilities at qualification time. That is dependency-audit coverage, not a
claim that the application is free of security defects.

Test maintenance preserves the behaviors being checked: namespace PIDs are
resolved to actual host children before asserting cleanup; descriptor-leak checks
run in an isolated process with a tighter allowance; the 4,096-token boundary
fixture accounts for the expanded tool catalog without raising the limit. Native
and browser accessibility checks wait for theme transitions to finish. The
browser fixture was aligned after CI captured an intermediate animated color;
the contrast assertions and application styling remain unchanged.

## Scope

The release targets Ubuntu 24.04+ / glibc 2.39+ on x86_64. Other distributions and
architectures are not qualified by this run. Model runtimes and Git remain external.

Parallel workspace preparation does not dispatch models or merge branches.
Guardian diagnostics do not execute tests or generate patches. AST call sites are
syntactic matches from bounded source scans. Optional bubblewrap isolation and
file checkpoints do not provide whole-machine rollback or a complete OS sandbox.
