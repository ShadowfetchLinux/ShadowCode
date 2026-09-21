# Native project inspection, diagnostics and test jobs

These workflows run in Rust on the `native-0.20` development branch. The desktop,
CLI and MCP server use the same project and task services. Python is needed only
when the user's project itself requires a Python test runner.

## Understand a project

```sh
shadowcode understand
shadowcode --json understand
shadowcode understand --save
```

The desktop commands are `/understand` and `/understand --save`. MCP exposes
`shadow_understand` with an optional `save` boolean, and the read-only
`shadow://project` resource. Inspection alone does not run a model, import project
code, execute a build script, install dependencies, or write files.

The map reports detected languages, manifest/framework signals, top-level source
modules, possible test commands, documentation, large source files and source
markers such as TODO/FIXME. These are heuristic findings, not proof of working
tests, installed dependencies, correctness, or an author's intent. Nested package
manifests are included, with commands rooted in their actual directories.

Inspection respects applicable ignore files, skips generated/dependency/state
directories, and does not traverse symlinks. Limits are 20,000 entries, depth 16,
64 manifests/modules, 128 KB per manifest, 64 KB per source-file prefix, and
8 MB of total content reads. The report includes counts, warnings and a partial
scan flag. Special files such as FIFOs are skipped; capability-relative,
nonblocking inspection reads open each path component without following symlinks.
The map is a live inspection, not an atomic snapshot of a changing repository.

Saving requires project trust and write permission, and reserves the workspace
against concurrent agent/manual writes. It updates a marked generated section
inside `.shadow/memory/project.md`, preserving surrounding notes. Repeated saves
replace that section. Stale contents, malformed markers, or a result exceeding
the 16 KB notes limit cause an error before writing. MCP additionally requires
`--allow-write`; registration remains read-only by default.

## Native diagnostics

```sh
shadowcode doctor
shadowcode doctor --test-model
```

Use `/doctor` or `/doctor --test-model` in the desktop. MCP exposes
`shadow_doctor`, with `test_model` disabled by default.
`/health` opens the Health panel; `/doctor` keeps its report in the conversation.
The panel displays the same checks with full details and repair suggestions.

Checks cover the native runtime, configuration, profile/credential-file metadata,
SQLite `quick_check`, Git availability, project inspection and detected test
toolchains. The report distinguishes passes, warnings, failures, informational
findings and checks that were not run. CLI output is readable text by default;
`--json` retains the structured report. Failed checks produce exit status 1;
warnings alone do not.

Model connectivity is **not inferred from configuration**. The optional model
test sends a small diagnostic prompt to the selected provider with a 15-second
deadline. It reports success/latency without including the model's reply or a
remote error body. It can consume provider usage. Ordinary diagnostics contact
no model and execute no project commands. Toolchain availability checks inspect
PATH entries rather than executing their binaries.

Python versions, browser launchers and local UI-server ports are not native
application requirements. A detected Python project can still need `python3`
to run its own tests. Diagnostic repair suggestions do not automatically install
software or edit permissions/configuration. Native `doctor --fix` and the updater
remain part of the wider migration.

## Recorded change history

```sh
shadowcode why
shadowcode why src/example.rs --count 12
```

The desktop command is `/why [path]`; MCP exposes `shadow_why` with optional
`path` and `count` (1–50). It returns recent commit subjects/dates plus current
and staged diffs. Paths stay within the selected workspace and are literal Git
pathspecs. External diff helpers, text conversion, signature display and hooks
are disabled. Process output and runtime use the native runner's bounds. The
result records evidence; it does not invent a rationale for a change.
Linked worktrees are supported. A repository without its first commit reports
an empty history while still showing staged and working changes.

## Test jobs without a model

MCP's `shadow_test` submits an exact command to the native engine and returns
an owned job ID. Use `shadow_jobs` for pending approvals, progress and final
`result.command` fields: command, stdout, stderr, exit code, timeout/truncation,
success and error. A job ID means submitted, not completed. No coding-model
request is needed, and a test job works when no model has been selected.

An omitted command is selected only when one unambiguous candidate is found:
`cargo test`, `go test ./...`, `python3 -m pytest -q`, or `npm test` when a package
declares a test script. Root candidates take priority over nested packages. If
several candidates remain, inspect `shadow_understand`'s `test_commands` and
provide an explicit command. Detection never runs those scripts.

Test submission requires `--allow-write`, project trust and non-read-only
permissions. Every command receives an exact approval even if ordinary shell
auto-approval is configured; the task's permission level is capped at Workspace.
The saved configuration is unchanged. Approve in
ShadowCode, or explicitly delegate owned-task decisions with `--allow-approvals`.
Existing root/network restrictions still apply. In particular, package-manager
commands may require network permission even when their test script is local.

Test jobs share the workspace queue, hooks, durable history, approvals and
cancellation with model tasks. An unsuccessful exit, denied action, timeout or
failed completion check cannot be reported as success. The requested timeout is
1–3,600 seconds and is also capped by the configured tool timeout; the approval
shows the effective timeout. EOF or gateway death cancels owned running/queued
jobs and cleans up command children. Shell-created changes are not file-tool
checkpoints. This is user-level command execution, not an OS sandbox.

The existing desktop `/test <command>` and CLI `exec` remain direct commands
explicitly entered by the user. An external MCP request uses the agent approval
path instead of treating its command text as human approval.
