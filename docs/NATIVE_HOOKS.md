# Native lifecycle hooks

> **Advanced.** This is reached through Settings › Advanced › Hooks. For the everyday workflow see the [user guide](USER_GUIDE.md), [subscriptions](SUBSCRIPTIONS.md) and [local models](LOCAL_MODELS.md).

Native hooks run explicitly enabled project commands at fixed points in an
agent task. They use the Rust process runner and need no Python interpreter or
callback loader. The archived
[release gates](archive/NATIVE_MIGRATION.md) record its earlier verification.

[Project plugins](NATIVE_PLUGINS.md) can supply native hook definitions.
Installation leaves them disabled; review their exact commands here before
activation. Removing a plugin revokes its project hook grants and preserves any
locally edited definition files.

## Define and enable a check

Create `.shadowcode/hooks/check.yaml` in your project:

```yaml
name: check
description: Run the Rust test suite before reporting success.
events: [on_complete]
command: cargo test --locked
timeout_sec: 60
```

Open **Settings › Advanced › Hooks**, select **Refresh hooks**, inspect the command, and
choose **Enable check**. Project trust is required. Discovery only reads data;
opening a repository or listing its hooks does not execute them.

The CLI exposes the same catalog and activation:

```sh
shadowcode --workspace /path/to/project --json hooks
shadowcode --workspace /path/to/project hooks \
  --enable .shadowcode/hooks/check.yaml --hash HASH_FROM_THE_CATALOG
shadowcode --workspace /path/to/project hooks --disable .shadowcode/hooks/check.yaml
```

Use your development `--profile` consistently. The path and SHA-256 hash identify
the definition you reviewed. If its contents change, refresh and review it again.
An enabled definition that changes or disappears blocks new Build tasks until
you approve the current contents or disable the old registration. Settings keeps
unavailable registrations visible so they can be removed. Plan and Review stay
usable with these hooks inactive.

## Definition format and limits

Definitions are direct `.yaml`, `.yml`, or `.json` files in `.shadowcode/hooks`
or `.shadow/hooks`. Both the filename stem and `name` accept 1–80 ASCII letters,
digits, underscores and hyphens. Symlink escapes and unsupported fields are
rejected. Required fields are `name`, `events`, and a nonempty `command`.

Optional `description` describes the check. `timeout_sec` defaults to 30 and
accepts 1–120 seconds, capped further by the task's configured tool timeout.
`path_suffix` restricts execution to events with a matching affected path;
it is useful for an `after_edit` formatter:

```yaml
name: format-rust
events: [after_edit]
path_suffix: .rs
command: rustfmt -- "$SHADOW_HOOK_PATH"
timeout_sec: 10
```

Definitions are limited to 32 KB, commands to 8 KB, descriptions to 1 KB, and
path suffixes to 80 bytes. The catalog displays at most 64 definitions/issues;
at most 16 hooks can be enabled per project and 256 registrations per profile.
Commands run sequentially in definition-path order. Output capture is limited
to 16 KB per invocation while both streams continue draining.

## Events and failure behavior

- `before_command`: before an agent `exec` or `background_start` action, after
  its normal permission decision. A failed check blocks the action and later
  checks for that event. An `exec`-only tool filter does not match background starts.
- `before_commit`: before the agent's `git_commit` tool, with the same blocking
  behavior. It does not intercept arbitrary Git commands in a shell.
- `after_edit`: after a successful `write_file`, `edit_file`, or `apply_patch`.
  A patch fires once for each changed path, after all patch changes are applied.
  Failure reports a failed tool result; it does not undo the changes.
- `after_test`: after an agent `exec` that resembles a test command, including
  a failed test. Detection recognizes common test runner words and commands such
  as `cargo test` and `npm test`; it is a lexical heuristic, not a shell parser.
  It can miss aliases or match words in an `echo` command. Use `on_complete` for
  a required check rather than depending on test detection.
- `on_error`: after a failed agent tool or a final provider failure after its
  retries. Hook failures do not recursively invoke error hooks. This event does
  not promise to catch every validation, startup, storage, or cancellation error.
- `on_complete`: before an agent task can finish successfully. A failed check
  returns its recorded output to the model for repair, bounded by
  `agent.max_fix_retries` and the task's normal step/token limits. Exhaustion
  leaves the task failed. A repaired completion runs the checks again.
  Repair context uses an 8 KB diagnostic excerpt across failed checks so noisy
  commands do not fill the model's next request; full captured output stays in
  task history.
  Ollama requests include runtime guidance in the leading system block because
  some model templates ignore later system messages; notes retain their original
  conversation positions and persisted history is unchanged.
- `on_compaction`: after context history is compacted. Failure stops the task.

Native [MCP test jobs](NATIVE_INSPECTION.md#test-jobs-without-a-model) also use
these command and completion checks. They call no model: a failed completion
check leaves the job failed with the original command output and exit status
retained, without attempting an automatic repair.

Hooks belong to agent and native test tasks. Manual terminal commands, background
starts from the panel/CLI, UI Git actions, and commands launched inside another
hook do not invoke them. A managed background process finishing later does not
trigger the originating task's `after_test` or `on_complete` hooks.
They are not an operating-system policy or repository-wide Git hook mechanism.

Each invocation records `hook.started` and `hook.completed` events with its
definition path/hash, command, status and process result. The conversation shows
a durable hook card; the CLI reports hook outcomes and `run --events` includes
the saved events. A running card means execution has started; output is saved
at completion, rather than streamed into that card.

## Context, cancellation and trust

Commands run under `/bin/sh -c` with the project as working directory. The
environment includes `SHADOW_HOOK_EVENT`, `SHADOW_HOOK_PATH` (one affected file),
`SHADOW_HOOK_COMMAND` (the original agent command), `SHADOW_HOOK_TASK_ID`, and
`SHADOW_HOOK_SESSION_ID`. `SHADOW_HOOK_CONTEXT` contains JSON with `event`, `tool`,
`paths`, `command`, `exit_code`, `detail`, and `detail_truncated`.
Detail is capped at 4 KB. Operational fields remain exact; context exceeding
64 KB fails the check instead of supplying a shortened command to a gate.

Context is passed as environment data. Quote its variables and never `eval`
them. It can contain untrusted filenames, commands or model output. A filename
containing shell syntax is a literal path when used as `"$SHADOW_HOOK_PATH"`.

Enabling a hook is standing approval to run that command in this project.
The SHA-256 approval pins the definition, **not the contents of scripts,
executables or dependencies it invokes**. Review those dependencies as part of
trusting the command. Hooks inherit the user's environment and filesystem
access, can change files beyond the project, and are not sandboxed. Existing
root/network command restrictions are still checked, but those lexical checks
cannot constrain arbitrary script behavior. No hook runs in an untrusted
project or a read-only task.

Settings changes apply to new tasks. Active and queued tasks retain their
configuration snapshot; disable does not interrupt one already in progress.
Cancel the task to stop it. The manifest hash is rechecked before every
invocation, including queued work, so an edited definition is never silently
executed with old approval. Cancellation, timeout and engine shutdown stop the
owned process group and wait for cleanup before the task releases its workspace.

Hook shell edits are not native file-tool checkpoints. A formatter can therefore
cause a later rewind conflict; checkpoint recovery preserves those changes
rather than overwriting them. File observations are invalidated after a hook
runs so subsequent full-file writes must read current contents again. Agent
reads run serially while hooks are active because even an error check can change
the workspace.

## Migrating Python callbacks

Legacy `*.py` files exporting `register(registry)` appear as inactive migration
issues. Native ShadowCode does not import them or automatically translate them.
Move formatting/check commands into reviewed YAML definitions. There is no
automatic built-in formatter or Python test callback in the native engine.
External tools such as `rustfmt`, `npm`, or `pytest` must be installed by the
project owner; choosing a Python command requires that external interpreter,
but the application itself still has no Python runtime dependency.
