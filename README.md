# ShadowCode

![version](https://img.shields.io/badge/version-0.21.0-386c51) ![Rust](https://img.shields.io/badge/runtime-Rust%201.95-orange) ![license](https://img.shields.io/badge/license-MIT-green)

**Your ideas. Your models. Your machine.**

ShadowCode is a Linux coding-agent workspace inspired by the focused workflow of
Codex. Bring a local model or an OpenAI-compatible provider. The harness owns
context, tools, permissions, checkpoints, plans, and verification.

![ShadowCode workspace](docs/images/workspace-light.png)

**Native 0.21 release:** the `main` branch builds a Rust/Tauri desktop window
with the interface embedded in the executable. It needs no Python runtime or
browser launcher. See the [native desktop guide](docs/NATIVE_DESKTOP.md)
and the [goals](docs/NATIVE_GOALS.md) and [model routing](docs/NATIVE_ROUTING.md)
workflows, [managed background processes and model tools](docs/NATIVE_BACKGROUND.md), and
[native slash commands and project skills](docs/NATIVE_WORKFLOWS.md),
[reviewed project plugins](docs/NATIVE_PLUGINS.md),
[reviewed lifecycle commands](docs/NATIVE_HOOKS.md),
[project inspection and diagnostics](docs/NATIVE_INSPECTION.md),
[project and task notes](docs/NATIVE_MEMORY.md),
[native SQLite inspection](docs/NATIVE_SQLITE.md),
[queued follow-ups](docs/NATIVE_QUEUE.md),
[approved MCP stdio and HTTP tools](docs/NATIVE_MCP.md), a
[native MCP stdio and authenticated HTTP server](docs/NATIVE_MCP.md#connect-another-coding-tool-to-shadowcode)
with Codex/Claude Code/Cursor registration output, and a
[native CLI](docs/NATIVE_CLI.md) and [terminal interface](docs/NATIVE_TUI.md)
that share the active desktop engine or run headlessly with their own profile.
The [native desktop can also attach](docs/NATIVE_DESKTOP.md#attaching-to-a-running-engine)
to a running headless/TUI engine and leave its work running when the window closes.
Release AppImage and Debian
packages include [dependency inventories and notices](licenses/native/README.md).
Integrations and release checks are tracked under the [release gates](docs/NATIVE_MIGRATION.md).

## Native AppImage

Download the `v0.21.0` **x86_64 AppImage**
and its `SHA256SUMS` file from [GitHub releases](https://github.com/ShadowfetchLinux/ShadowCode/releases/latest).
The native application embeds its interface; Python, Node.js and a browser
launcher are not runtime dependencies. The release targets **Ubuntu 24.04 or
newer / glibc 2.39+**. Git and a model provider such as Ollama remain external.

```bash
sha256sum -c SHA256SUMS
chmod +x ShadowCode_0.21.0_amd64.AppImage
./ShadowCode_0.21.0_amd64.AppImage --appimage-extract-and-run
```

Extraction mode works without FUSE. To install into `~/Applications`, add a
stable `shadow` command, and replace the desktop launcher:

```bash
git clone https://github.com/ShadowfetchLinux/ShadowCode.git
cd ShadowCode
./scripts/install-appimage.sh /path/to/ShadowCode_0.21.0_amd64.AppImage
```

Keep `SHA256SUMS` beside the download and the installer verifies its matching
entry automatically. It replaces older ShadowCode AppImages only after the new
executable starts, preserves settings, keys, memory and task history, and refuses
a changed download before it can alter the installed application.

## Native 0.21 highlights

- **A focused workspace.** Persistent projects and tasks, search, pinned tasks,
  a refined light/dark interface, and responsive layouts. Files, review, terminal,
  goals, and health stay beside the conversation.
- **A better conversation.** Markdown with code-copy controls, live tool cards,
  observable task plans, saved prompt drafts, and scroll position that respects
  reading older output. Keyboard-driven navigation and accessible dialogs.
- **Reliable continuation.** Reload reconnects to a running task. Stable event
  cursors prevent duplicate history and the old 800-event stream stall. Restarts
  mark unfinished jobs interrupted and expose a Continue action.
- **Context that follows the task.** Resuming switches the backend workspace;
  follow-ups and branches carry bounded prior conversation. Renamed tasks keep
  their names. Cancellation remains pending until the worker stops.
- **Review with evidence.** Untracked file previews, separate staged/unstaged
  views, literal filenames, and protection against stale hunk application.
- **A standalone native app.** Rust engine, embedded interface, AppImage and Debian
  packages, checksums, dependency notices, private local IPC, and native CLI/TUI.
- **Local coding workflows.** Durable queues, goals, model routing, approvals,
  worktrees, background processes, MCP, project skills and hooks all share one
  local task engine.

See [CHANGELOG.md](CHANGELOG.md) for the release history and
[the user guide](docs/USER_GUIDE.md) for workflows and recovery.

## Native development build

Requires Rust 1.95, Node 22+, and the packages listed in the [native desktop
guide](docs/NATIVE_DESKTOP.md#build-and-run).

```bash
git clone https://github.com/ShadowfetchLinux/ShadowCode.git
cd ShadowCode
npm --prefix ui ci
npm --prefix ui run build
cargo build -p shadowcode-desktop --locked
./target/debug/shadowcode --profile /tmp/shadowcode-dev --workspace /path/to/project
```

`--profile` keeps development data separate from the installed application. See
the [native migration guide](docs/NATIVE_MIGRATION.md) for validation and release
requirements.

## Choose a model

On first launch, select a detected local model and **Test & start**, or enter a
provider endpoint and model ID. Mock is an offline harness demonstration with
deterministic behaviors, not a general coding model.

```bash
shadow models
shadow models --use gpt-oss:20b
shadow models --use my-model --provider local --endpoint http://127.0.0.1:1234/v1
shadow models --use my-model --provider openai_compatible --endpoint https://your-provider.example/v1
```

Supported providers: Ollama, LM Studio/local, llama.cpp, vLLM, and any compatible
`/v1/chat/completions` endpoint. The composer can override the model per task.
Store API keys through Settings or the environment variable named by `api_key_env`.
Never put keys in YAML or Git.

## Workflows

- **Build:** describe a change, watch inspection and tool execution, review the
  resulting files, and inspect verification output.
- **Review:** open Review in the top bar. Stage a hunk or new file, inspect the
  staged diff, then commit with a message. Discarding a hunk asks first.
- **Continue:** select a task in the sidebar. Drafts and history return; running
  work reconnects. Branch, rename, export, or delete through Manage tasks in the
  command palette.
- **Goals:** create a milestone checklist and run/resume its tasks. Automatic
  completion reflects the harness verifier's checks, not a proof of correctness.
- **Inspect:** browse files or run a bounded command in the terminal panel.
  Long-running services belong in Background processes.

## Keyboard

`Ctrl+K` command palette · `Ctrl+B` sidebar · `Ctrl+N` new task · `Ctrl+P` open
project · `Ctrl+,` settings · `Ctrl+L` composer · `Ctrl+.` stop ·
`Ctrl+Shift+E` export · `?` help. `Enter` sends; `Shift+Enter` inserts a line.
Approval cards require their explicit Allow or Deny buttons.

## CLI and integrations

```bash
shadowcode tui                     # native terminal UI
shadowcode run "Explain this workspace"
shadowcode run --json "Review this workspace"
shadowcode ui                      # native desktop window
shadowcode sessions                # task history
shadowcode export --format md
shadowcode goal "Improve test coverage" --run
shadowcode goals
shadowcode doctor
shadowcode mcp serve               # MCP over stdio
shadowcode mcp serve --http 127.0.0.1:7431
shadowcode mcp register            # client configuration snippets
```

AppImage users install the next release with `install-appimage.sh`; it does not
rewrite the running image. The standalone executable opens the desktop with no
arguments; use `tui` explicitly for the terminal interface.

Slash commands include `/help`, `/model`, `/plan`, `/diff`, `/review`, `/test`,
`/git`, `/goal`, `/goals`, `/memory`, `/skills`, `/sessions`, `/new`, `/branch`,
`/doctor`, and `/settings`. Type `/` to browse the current command catalog.

## Data and permissions

Configuration: `~/.config/shadow-agent/config.yaml`
Secrets: `~/.config/shadow-agent/secrets.env` (mode 600)
Sessions, desktop jobs, and events: `~/.local/state/shadow-agent/shadow-agent.db`
Goals: `~/.local/state/shadow-agent/goals.db`
Project instructions and memory: `<project>/.shadow/`

Existing `shadow-agent` paths and desktop IDs are retained for compatibility.
See [config.example.yaml](config.example.yaml), [ARCHITECTURE.md](ARCHITECTURE.md),
and [SECURITY.md](SECURITY.md). Filesystem tools constrain paths to the workspace;
shell commands run as your user and are **not an operating-system sandbox**.

## Development and verification

```bash
python3 -m venv .venv
.venv/bin/pip install -e '.[dev]'
npm --prefix ui ci
npm --prefix ui run build
.venv/bin/python -m pytest
npm --prefix ui test
cd ui && npx playwright install chromium && npm run test:e2e
```

Tests use temporary workspaces and XDG directories. Browser tests exercise the
real API with the offline provider, including reconnects, review, and accessibility.
See [CONTRIBUTING.md](CONTRIBUTING.md) and [release instructions](docs/RELEASING.md).

[MIT](LICENSE) · Copyright 2026 Shadowfetch
