# Native project and task notes

> **Advanced.** This is reached through the `/memory` command, the CLI and MCP. For the everyday workflow see the [user guide](USER_GUIDE.md), [subscriptions](SUBSCRIPTIONS.md) and [local models](LOCAL_MODELS.md).

ShadowCode keeps project notes in
`.shadow/memory/project.md` and task notes in the profile's SQLite history.
These are explicit notes, distinct from model reasoning and recorded verification
results. No Python process is involved.

## Read, append and edit

```sh
shadowcode memory
shadowcode memory "Run the offline fixtures before changing the parser."
shadowcode --json memory --task TASK_ID
shadowcode memory --task TASK_ID "Keep the original regression case."
```

Use the exact `task_id` returned by `run --json` or an MCP job. It is distinct
from a job ID or conversation ID. The task must already exist in the selected
project; unknown IDs, path fragments and tasks from another project are rejected.

In the desktop, `/memory` shows project notes and the selected conversation's
latest task notes. `/memory <note>` appends project notes, while
`/memory --task <TASK_ID> <note>` appends task notes. Omitting the note reads it.
The command result stays in the conversation. The CLI also accepts these forms
through `command memory`; use `--` before an argument string beginning with
`--task` so the shell argument is treated as command text.

MCP's `shadow_memory` accepts `action: read|append|replace`,
`scope: project|task`, `task_id`, `note`, and `expected_hash`. Scope defaults to
project; task scope requires an explicit existing task ID. A read returns
`project`, `task`, `project_hash`, `task_hash`, and `task_id`. Changes require
`--allow-write`, project trust and configured write permission. Read-only clients
can inspect notes in their fixed project but cannot change them.

Each note store is limited to 16,000 UTF-8 bytes. To shorten, edit or clear notes,
first read the current hash, then explicitly replace that version:

```sh
shadowcode --json memory --task TASK_ID
shadowcode memory --task TASK_ID --replace --expected-hash TASK_HASH "Revised note"
```

Use an empty string to clear notes. Omitting `--task` targets project notes and
requires their `project_hash`. A stale or missing hash fails without changing the
notes. Appends merge the current value; database appends and their audit event
commit together. Manual writes reserve the same workspace as tasks and terminal
operations, so they cannot race an active task's project changes. Project-file
writes also check the observed file hash before replacement.

## Continuation, branches and export

Future runs in a conversation receive recent task notes as labelled historical
data. The engine inspects the latest eight tasks, prioritizes recent notes, limits
each excerpt to 4 KB and the combined context to 16 KB, and labels omissions.
Inherited branch notes also have a labelled context excerpt. Notes cannot grant
permissions or count as proof that tests passed.

A branch preserves the full notes from its source conversation, independently
of future source edits or deletion. Its archive includes inherited notes and
supports up to 10,000 tasks and 32 MB of note text; exceeding those bounds rejects
the branch instead of silently dropping notes. The transcript/context uses
excerpts, while JSON and Markdown exports retain the archived text and current
task notes within the existing 32 MB conversation export limit. Deleting a source
conversation removes its native task-note records but leaves a branch's copy.

## Existing profiles

The history schema moves from version 23 to 24 with an automatic SQLite backup
before migration. Legacy `state/tasks/<TASK_ID>/memory.md` files remain untouched.
For a known task in the selected project, the native engine reads that legacy
file until the first native edit saves its content in SQLite. A cleared native
note remains empty rather than restoring the old file on the next read.

Legacy reads are confined to the profile, limited to 16 KB, and reject
symlinks in every path component and special files. Oversized or unreadable legacy notes are reported
instead of discarded. Unattributed legacy folders such as `tasks/mcp` are
preserved; the native server does not guess which project or task owns them.

Notes are included in conversation exports and database backups. Manual note
edits are not file-tool checkpoints. The wider [native release gates](archive/NATIVE_MIGRATION.md)
still apply.
