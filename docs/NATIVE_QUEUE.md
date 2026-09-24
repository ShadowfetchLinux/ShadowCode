# Queued follow-ups

> **Reference.** This covers the composer (press Enter while a task runs). For the everyday workflow see the [user guide](USER_GUIDE.md), [subscriptions](SUBSCRIPTIONS.md) and [local models](LOCAL_MODELS.md).

Keep working on the next instruction while ShadowCode runs a task. In the native
desktop, enter a message and press Enter or **Queue follow-up**. The running
response continues to stream; **Up next** shows the project's waiting messages
in execution order. Expand a message to read it in full, or use its cancel button
to remove it from the queue.

Model and mode choices apply to the new follow-up. The queued task retains those
choices and uses the completed conversation when it starts, including the
previous task's actual tool results. Plan and Review keep their read-only
permissions. Queuing does not grant approvals or run two tasks in one project
at once.

## Conversations and navigation

The queue belongs to the project. A new conversation in the same project waits
for its earlier tasks; conversations in other projects can run independently,
within the engine's four-project concurrency limit. Queue rows from another
conversation include a link to it. The sidebar distinguishes waiting tasks from
running tasks and does not label completed history as active.

Switching conversations or reloading the window retains the queue. Returning to
a conversation selects its running task first, then its oldest waiting task.
When a task finishes, the desktop loads the next task and its intervening
history. Cancelling a waiting message does not replace the active task's plan,
usage or progress state. Prompts appear in execution order when their tasks
start, without duplicate user messages.

The queue cancel button only cancels work that is still queued. If the task has
started since the button was displayed, ShadowCode reports that change and
leaves it running. Open its conversation and use **Stop task** to stop it.
Stopping a running task leaves other queued tasks eligible to run; cancel those
messages too if they are no longer wanted.

## Limits and recovery

- Up to 64 tasks can be running or queued across the application. Excess
  submissions return an error and restore the draft.
- Slash commands and new file uploads wait until the project is idle. Ordinary
  follow-up messages can refer to files already in the project.
- The active-job listing includes old running/queued tasks even when another
  project has produced many newer completed tasks.
- Reloading a window does not stop jobs. Closing the native application performs
  managed shutdown and cancels its active and queued work. After an abrupt exit,
  unfinished tasks are marked interrupted for review; they are not replayed
  automatically.
- This desktop queue uses the native engine. The native CLI also supports queued
  submissions; see [NATIVE_CLI.md](NATIVE_CLI.md).

See [the 0.2x verification record](archive/NATIVE_VERIFICATION.md) for the engine and real-window
checks and [the 0.2x release gates](archive/NATIVE_MIGRATION.md).

## Polling and long prompts

The native desktop polls `/api/jobs?view=summary&limit=100`: at most 100 recent
records plus all active records, including older running/queued work. Polling
omits full results and summaries and limits prompt previews to 512 Unicode
characters. A truncated preview is marked explicitly. Expanding a queued prompt
loads the complete job once, with a visible error and retry if loading fails.
Opening a conversation fetches its full job separately, preserving its result,
original prompt and continuation behavior. The existing full job endpoint and
ordinary CLI listing remain available; this change targets repeated UI polling.
