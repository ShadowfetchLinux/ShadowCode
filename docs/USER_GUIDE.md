# ShadowCode user guide

This guide covers ShadowCode 0.28 and follows the usual workflow: open a
project, pick a model, describe the task, watch the agent work, then review
what changed. Installation and a feature overview are in the
[README](../README.md).

## Open a project

Use **Open project** (`Ctrl+P`) and choose a folder. The first time, ShadowCode
asks you to trust the folder: project instructions, skills, hooks and plugins
can influence or run work. Tasks do not start in an untrusted project, whether
they come from the desktop, the CLI, goals or MCP.

On first launch, onboarding asks for the project and a permission mode.
*Ask before actions* is the default. You can change it later in
**Settings › Permissions & network**.

The sidebar groups conversations by project. **New task** (`Ctrl+N`) starts a
new conversation in the current project.

## Pick a model

Open the picker (`Ctrl+M`, or the model button in the composer). It has two
groups:

- **Subscriptions**: Codex, Claude Code, Cursor, Antigravity and Grok, run
  through the vendor's official CLI. See [subscriptions](SUBSCRIPTIONS.md).
- **On this computer**: GGUF models run by the bundled llama.cpp. See
  [local models](LOCAL_MODELS.md).

Each row shows Local or Cloud, its availability, a *Vision* or *Chat only*
badge where it applies, and its usage line. Search filters rows. Vendors with
many models show their default, the selected row and recent rows first; the
rest are behind a *more* entry. Press the right arrow key or the info icon to
open a row's details: the reason it isn't ready, usage windows and reset times,
and a link to the vendor's own usage page.

A row that isn't ready still opens something useful: *Sign in* opens
**Settings › Accounts**, *Setup required* opens the right settings page, and
*Unavailable* explains why. Send stays disabled until a ready row is selected.

ShadowCode stores the choice for the conversation and uses it as the project's
default for new conversations.

## Describe the task

Type in the composer and press `Enter`. `Shift+Enter` adds a line.

- **Attachments.** Attach text files or up to four images per message. Images
  are accepted only when the selected row is marked Vision. Attachments are
  copied into `<project>/.shadow/attachments/`.
- **Web.** The **Web** chip appears only when a local model is selected, since
  vendor CLIs bring their own web tools. Turning it on lets the agent use
  `web_fetch` and `web_search` for the task. In *Web tools off* and *Offline*
  modes, a pill replaces the chip.
- **Permission mode.** The mode control in the composer shows the current mode
  and how the selected vendor applies it.
- **Slash commands.** Type `/` to browse them. `/plan` and `/review` start
  read-only tasks. `/model` opens the picker.

If a task is already running, pressing `Enter` queues the message as a
follow-up.

## Watch it work

The activity timeline under the answer is built from recorded events: reading,
searching, editing, running commands, waiting for approval, web sources and
verification. Expand an item to see its output. Vendor tool names (for example
Codex command executions, or Claude's `Bash` and `Edit`) are grouped the same
way as ShadowCode's own tools.

### Approvals

When the agent wants to do something that needs permission, an approval card
shows what it is, for example `Edit src/main.rs`, `Apply a patch to …`, or a
shell command. Choose **Allow** or **Deny**. Keyboard shortcuts never approve
anything.

- **Local models.** ShadowCode enforces the permission mode for every tool
  call.
- **Codex, Claude Code, Cursor, Grok.** These vendors send their own approval
  requests, which ShadowCode shows. Each vendor decides which of its actions
  need approval. In Plan/Review tasks, ShadowCode denies vendor requests
  automatically and records a warning.
- **Antigravity.** It never asks ShadowCode. It applies its own settings
  (`~/.gemini/antigravity-cli/settings.json`) and reports refused actions. A
  warning at task start reminds you.

An unanswered vendor approval is denied after 10 minutes
(`cli_agents.approval_timeout_sec`).

### Stop, pause and steer

- **Stop** (`Ctrl+.`) cancels the task. For a vendor CLI, ShadowCode ends the
  vendor's whole process group. A local model stops at the next cancellation
  point.
- **Pause** waits for a safe boundary (a running command finishes first).
  Add a steering note, then **Resume** to continue without replaying finished
  commands. With a vendor CLI, pausing interrupts the vendor's turn and the note
  is sent as a follow-up.

## Review the result

When the task ends, a summary lists the changed files with line counts and any
test or build commands with their results. A task counts as verified only from
recorded command results. An answer that claims success without a recorded
check is marked as unverified.

- **Review changes** opens the Changes drawer (`Ctrl+Shift+B`). Pick a file,
  compare unstaged and staged hunks, stage a hunk or a whole new file, discard a
  hunk (after confirming), and commit with a message. If a file changed since
  you previewed it, refresh before staging.
- **Rewind** undoes every file change the task made with ShadowCode's own
  tools. Stop the task first. Rewind doesn't undo shell commands or Git
  history. It isn't offered for vendor CLI tasks, because the vendor writes
  files with its own tools. Use the Changes drawer or Git to undo those. After
  a rewind, the next turn is told that those edits are no longer on disk.

## Switch models mid-conversation

You can pick another row at any time. If a task is running, it keeps its model
and the new choice applies from the next turn.

- **Same vendor, different model.** The vendor's session resumes with the new
  model, and a *model switched* note appears. No handoff is needed.
- **Different provider.** The new provider hasn't seen the earlier turns, so
  ShadowCode passes it a handoff block: your requests, the final answers and
  the changed files, up to 12,000 characters, marked as earlier context and not
  as instructions. Vendor CLIs receive it before your message. Local models
  read the same turns from the conversation history.
- **Consent.** Before content goes to a cloud provider, a dialog shows what
  would be sent. This happens when the previous turn ran on this computer, when
  the conversation moves to another provider, or when you first attach images
  to a cloud row in this conversation. Nothing is sent or recorded until you
  choose **Send**. **Cancel** leaves the conversation unchanged.

When you return to a vendor you used earlier in the conversation, ShadowCode
resumes that vendor's own session: Codex `thread/resume`, Claude `--resume`,
Cursor and Grok ACP `session/load`, Antigravity `--conversation`.

## When a plan limit is reached

If a vendor reports that your plan limit is reached, the task stops with the
status *Plan limit reached*. A banner offers **Choose model**, and that vendor's
affected rows show as unavailable until the limit resets. ShadowCode doesn't
retry, buy credits or turn on overages.

## Offline and web-off

In **Settings › Permissions & network**:

- **Web tools off**: local models get no web tools. Subscriptions still work.
- **Offline**: only rows under *On this computer* run. Cloud rows show as
  unavailable. ShadowCode starts no vendor process for sign-in status, models
  or usage, and a cloud job is refused with
  "Offline mode: choose a model that runs on this computer". Shell commands
  that reach the network (for example `curl`, `npm` or `pip`) are denied.

## Continue, organize and recover

- **Reloading** the window keeps the selected conversation, the transcript and
  your unsent draft. A running task reconnects without repeating output.
- **Older messages** pages back through long conversations. **Fork from here**
  starts a new conversation at a response.
- **Interrupted tasks.** If ShadowCode exits during a task, the task is marked
  *interrupted*. **Continue task** writes a recovery request for you. Shell
  commands and file edits are never replayed automatically.
- **Command palette** (`Ctrl+K`): rename, branch, export (`Ctrl+Shift+E`) and
  delete conversations.

## Troubleshooting

- **A subscription row says Sign in.** Use **Settings › Accounts › Connect**,
  or run the vendor's login command in a terminal and choose **Refresh**.
  Antigravity sign-in happens only inside `agy`.
- **A local row says Setup required.** The llama.cpp runtime is missing. Run
  `scripts/install-appimage.sh` again, or build it with
  `scripts/build-llama.cpp.sh`.
- **A local model loaded on the CPU.** The row shows *CPU fallback (GPU load
  failed)*. Check that `libvulkan1` and a Vulkan driver are installed, and read
  the error on **Settings › Local models**.
- **The AppImage won't mount.** Run it with `--appimage-extract-and-run`. FUSE
  is optional.
- **You need stronger isolation.** Use a container or a separate Linux account.
  Shell commands run with your user's privileges.

Settings and history live in `~/.config/shadow-agent` and
`~/.local/state/shadow-agent`. Back up both before moving to another machine.
