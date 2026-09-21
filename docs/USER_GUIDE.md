# ShadowCode user guide

This guide covers the supported 0.19 release. For the Python-free 0.20
development branch, use the [native desktop](NATIVE_DESKTOP.md) and
[native CLI](NATIVE_CLI.md) and [native hooks](NATIVE_HOOKS.md) guides, plus the documented
[migration status](NATIVE_MIGRATION.md). Native lifecycle and integration support
differ from the browser/API release described below.

## Start a project

Open ShadowCode, choose a project directory, and select a model. Local servers
are detected when available. Remote providers need an endpoint, model ID, and
API key. Test the connection before starting. Mock is a deterministic offline
demo; it can demonstrate a hello-world workflow without model credits.

The left sidebar groups tasks by project. **New task** starts a fresh conversation
in the current project. The project button or `Ctrl+P` opens another folder. New
folders show a trust prompt because project instructions and hooks can influence
or execute work. To keep inspecting without edits, choose read-only permissions.

## Describe, inspect, review

Write a task and press Enter. The composer clears after submission; Shift+Enter
adds a line. Build/Research/Review/Test select the routing purpose, not a separate
security boundary. Choose the permission level in Settings.

The transcript shows agent explanations and compact operations. Expand an
operation to inspect its output. File-changing operations offer review and
checkpoint rewind. The plan above the composer reflects observed actions and
verification; skipped steps were not needed for that task. A successful harness
verdict does not establish correctness beyond the checks actually performed.

Review opens the Git panel. Select a file, compare its unstaged or staged changes,
then stage a hunk or a whole new file. If a file changed since the preview, refresh
it before staging. Discard asks for confirmation. Commit uses the staged index.
Binary and large files are identified when a full text preview is unavailable.

The Files panel reads files within the active project. The Terminal panel runs
one command with a 60-second limit; it is not an interactive PTY. Agent tasks and
manual writes do not compete in the same workspace. Use Background processes for
services that should keep running.

## Continue and organize

Task search covers titles, workspace paths, and task prompts. Pin buttons keep
frequent tasks at the top. Manage tasks in the command palette provides rename,
branch, Markdown export, and deletion. Branches inherit the parent transcript;
the filesystem is shared, not a separate Git worktree.

Reloading restores the selected task, its transcript, and its unsent text draft.
An active task reconnects without duplicating output. Attachments are text/source
files up to 1 MB; they are copied into `.shadow/attachments/`. Attachment selections
are not retained through reloads. Drafts, pins, and sidebar state are browser-local;
use Export when you need a portable conversation artifact.

If the API restarts during a task, its job record becomes **interrupted**. Open
that task and choose Continue to compose a recovery request. Inspect the recorded
changes first. Closing the browser does not stop the API; terminating the server
does stop its workers. Stop requests can wait for an in-flight model request or
tool timeout before the worker exits.

## Models, settings, and goals

The model selector accepts detected or configured models and a custom model ID.
Settings covers provider connection, permissions, appearance, notifications,
hooks, MCP, and plugins. API keys remain separate from YAML. Browser notifications
need browser permission; Linux desktop notifications use the system notifier.

Goals create and track milestone tasks. Goals and ordinary tasks use the same
harness. If a milestone fails, review its evidence and resume after correcting the
problem. Background processes are separate from goal tasks.

## Troubleshooting

- **Blank/stale source UI:** run `shadow doctor --fix`, or rebuild with
  `npm --prefix ui ci && npm --prefix ui run build` from the checkout.
- **AppImage will not mount:** use `--appimage-extract-and-run`; FUSE is optional.
- **No model response:** inspect Workspace health and Test connection in Settings.
  Confirm the model server is listening and the model ID matches that server.
- **Reconnecting:** the task may still be running. Keep the app open; it retries
  and polls status. If the API was stopped, launch it again and inspect recovery.
- **Port already used after an upgrade:** stop the old ShadowCode API process
  after its tasks finish, then reopen the app. Do not run two APIs against one
  XDG profile. By default the address is `http://127.0.0.1:7430`.
- **Missing project:** restore the folder or open its new location as a project.
- **Need stronger isolation:** run the project in a container or separate account;
  shell commands use your Linux user's privileges.

The compatible XDG directories remain named `shadow-agent`. Back up both
`~/.config/shadow-agent` and `~/.local/state/shadow-agent` before moving machines.
