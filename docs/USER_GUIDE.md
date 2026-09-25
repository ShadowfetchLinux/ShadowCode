# ShadowCode user guide

This guide covers ShadowCode 0.30 and follows the usual workflow: open a
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

Open the picker (`Ctrl+M`, or the model button in the composer). It has three
groups:

- **Subscriptions**: Codex, Claude Code, Cursor, Antigravity and Grok, run
  through the vendor's official CLI. See [subscriptions](SUBSCRIPTIONS.md).
- **On this computer**: GGUF models run by the bundled llama.cpp. See
  [local models](LOCAL_MODELS.md).
- **API keys**: OpenRouter models, billed per token to your OpenRouter
  account. Add a key in **Settings › Accounts › OpenRouter**; until then the
  group offers *Add an OpenRouter API key…*. See [OpenRouter](OPENROUTER.md).

Each row shows Local or Cloud, its availability, a *Vision* or *Chat only*
badge where it applies, and its usage line. Search filters rows. Vendors with
many models show their default, the selected row and recent rows first; the
rest are behind a *Show all* entry. Press the right arrow key or the info icon to
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
- **Web.** The **Web** chip appears when a local or OpenRouter model is
  selected, since vendor CLIs bring their own web tools. Turning it on lets the agent use `web_fetch` and
  `web_search` for the task. In *Web tools off* and *Offline* modes, a pill
  replaces the chip.
- **Permission mode.** The mode control in the composer shows the current mode
  and how the selected vendor applies it.
- **Slash commands.** Type `/` to browse them. `/plan` and `/review` start
  read-only tasks. `/model` opens the picker. Claude Code commands in
  `.claude/commands/` appear here too.
- **Agents.** Start a message with `@explore`, `@plan`, `@review`, `@general`
  or a project agent's name to run that subagent first. Type `@` to list them.

If a task is already running, pressing `Enter` queues the message as a
follow-up.

## Watch it work

A note above the answer names the model that ran, for example
*Using Cursor · Auto · Cloud*. It appears again only when the model changes.

The activity timeline under the answer is built from recorded events: reading,
searching, editing, running commands, waiting for approval, web sources and
verification. Expand an item to see its output. Vendor tool names (for example
Codex command executions, or Claude's `Bash` and `Edit`) are grouped the same
way as ShadowCode's own tools.

### Approvals

When the agent wants to do something that needs permission, an approval card
shows what it is, for example `Edit src/main.rs`, `Apply a patch to …`, or a
shell command. Choose **Allow** or **Deny**. Keyboard shortcuts never approve
anything. The card appears in the conversation as soon as the agent asks.

- **Local and OpenRouter models.** ShadowCode enforces the permission mode
  for every tool call.
- **Codex, Claude Code, Cursor, Grok.** These vendors send their own approval
  requests, which ShadowCode shows. Each vendor decides which of its actions
  need approval. In Plan/Review tasks, ShadowCode denies vendor requests
  automatically and records a warning.
- **Antigravity.** It asks ShadowCode through its agent server, like Cursor
  and Grok. If it asks you a question instead of for permission, ShadowCode
  can't show the choices yet: it skips the question with a note, and you can
  answer in your next message.

An unanswered vendor approval is denied after 10 minutes
(`cli_agents.approval_timeout_sec`).

### Subagents

With a local or OpenRouter model, the agent can hand focused work to
subagents: `explore` searches, `plan` plans, `review` reviews (all
read-only), and `general` edits files in its own Git worktree. Several can run
at once. Each run shows as a card in the conversation; click it for the
result and changed files, or **Open transcript** for its own conversation.
A write subagent's changes reach your project only when the main agent
applies its diff, with your usual edit approval. Approvals a subagent needs
appear in this conversation, labelled with its name.

Projects can add their own agents in `.shadow/agents/`, `.claude/agents/` or
`.opencode/agent/`, and ShadowCode reads `CLAUDE.md`, nested `AGENTS.md` files,
Cursor rules and Claude Code skills. See [Subagents](SUBAGENTS.md).

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
check is marked as unverified. A task that finished or was stopped without
changing files or running checks shows one quiet line instead, for example
*Finished · 7s · No files were changed.* A stopped task keeps its partial
reply.

- **Review changes** appears only when files changed. It opens the Changes
  drawer (`Ctrl+Shift+B`). Pick a file, compare unstaged and staged hunks,
  stage a hunk or a whole new file, discard a hunk (after confirming), and
  commit with a message. If a file changed since
  you previewed it, refresh before staging. The drawer's tabs keep your work
  while you switch between them or close the drawer: terminal output, the
  file open in Files and an unsent commit message stay until you open
  another project.
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
  as instructions. Vendor CLIs receive it before your message. Local and
  OpenRouter models read the same turns from the conversation history.
- **Consent.** Before content goes to a cloud provider, a dialog shows what
  would be sent. This happens when the previous turn ran on this computer, when
  the conversation moves to another provider, or when you first attach images
  to a cloud row in this conversation. Nothing is sent or recorded until you
  choose **Send**. **Cancel** leaves the conversation unchanged.

When you return to a vendor you used earlier in the conversation, ShadowCode
resumes that vendor's own session: Codex `thread/resume`, Claude `--resume`,
Cursor, Grok and Antigravity ACP `session/load`.

## When a plan limit is reached

If a vendor reports that your plan limit is reached, the task stops with the
status *Plan limit reached*, and that vendor's affected rows show as
unavailable until the limit resets. ShadowCode doesn't retry on the same plan,
buy credits or turn on overages.

What happens next is set under **When a plan runs out**, in the Allowance
panel and at the top of **Settings › Accounts**:

- **Continue on a local model** (the default). The same conversation carries
  on right away on a model on this computer, with the usual summary of the
  earlier turns. The conversation shows "Codex reached its plan limit.
  Continuing on qwen3:14b on this computer." and the follow-up message is
  labelled *Continued automatically*. Nothing leaves your computer and there is
  no quota. The model is the one you pick in the setting, or else the last
  local model you used in this project, or else the first ready local model
  with tool support.
- **Ask me.** The conversation shows a card with **Continue on <local model>**
  and **Choose another model**.

If no local model is ready, the conversation says so and offers **Open Local
models**.

## Allowance

The **Allowance** button in the status bar opens one list of everything you
can run and how much of it is left, as each source reports it:

- **Subscriptions**: the reported usage windows with reset times (Codex), the
  plan, *Plan limit reached*, or *Usage not reported* for vendors that expose
  none. Signed-out or missing tools link to **Settings › Accounts**.
- **OpenRouter**: credits left of your key's limit, or what has been spent.
- **On this computer**: how many local models are ready. There is no quota,
  and the **When a plan runs out** setting lives here.

The dot on the button turns amber when a source you can run is low or has
reached its limit. **Refresh** checks the vendor accounts again.

## Compare models

Not sure which model suits a task? Type it, then press **Compare** next to
**Send** and choose 2 or 3 models (at most one on this computer). Each works
in its own copy of the project, starting from your latest commit plus any
uncommitted work, so your files stay as they are until you choose.

The **Comparisons** view (top bar, or *Comparisons in this project* in the
command palette) shows every lane side by side: what it changed, which checks
passed, how long it took and what it used. Open a file to see that lane's
change, or open its conversation to follow up with that model. A lane that
asks for permission says so, and approvals work as usual.

- **Keep** applies that model's changes to your project's working tree. Review
  them in **Changes** as usual; nothing is committed. If your project changed
  since the comparison started and the changes no longer apply, ShadowCode
  lists the conflicting files and changes nothing.
- **Discard all** throws every lane away. **Stop** cancels lanes that are
  still working and keeps what they did so far.

After keep or discard, every lane's copy and branch is removed. **Wins in this
project** counts which model you kept. Every lane is a full task: subscription
lanes use your plan, OpenRouter lanes are billed per token. Details:
[compare](COMPARE.md).

## Offline and web-off

In **Settings › Permissions & network**:

- **Web tools off**: ShadowCode's own agent loop (local and OpenRouter models)
  gets no web tools. Subscriptions still work.
- **Offline**: only rows under *On this computer* run. Cloud rows show as
  unavailable. ShadowCode starts no vendor process for sign-in status, models
  or usage, and a cloud job is refused with
  "Offline mode: choose a model that runs on this computer". Shell commands
  that reach the network (for example `curl`, `npm` or `pip`) are denied.

## Continue, organize and recover

- **Reloading** the window keeps the selected conversation, the transcript and
  your unsent draft. A running task reconnects without repeating output.
- **Older messages** pages back through long conversations. **Fork from here**
  starts a new conversation at a response. A very long page shows its latest
  150 items first; **Show … earlier items** adds more without moving what
  you are reading.
- **Interrupted tasks.** If ShadowCode exits during a task, the task is marked
  *interrupted*. **Continue task** writes a recovery request for you. Shell
  commands and file edits are never replayed automatically.
- **Command palette** (`Ctrl+K`): rename, branch, export (`Ctrl+Shift+E`) and
  delete conversations.

## Troubleshooting

- **A subscription row says Sign in.** Use **Settings › Accounts › Connect**,
  or run the vendor's login command in a terminal and choose **Refresh**.
  Antigravity's sign-in is ShadowCode's own, so use **Connect** for it.
- **Antigravity says Setup required.** Choose **Install** on its Accounts card
  (a one-time 334 MB download from Google), then **Connect**.
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
