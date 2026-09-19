# ShadowCode

![version](https://img.shields.io/badge/version-0.18.0-black) ![python](https://img.shields.io/badge/python-3.12%2B-blue) ![license](https://img.shields.io/badge/license-MIT-green)

Linux-native, model-agnostic coding-agent **harness** — a Claude Code / Codex
alternative that you run on your machine. The LLM is replaceable. The harness
owns tools, permissions, context, memory, checkpoints, hooks, skills, MCP,
verification, and routing.

**Brand:** ShadowCode · **CLI:** `shadow` · **Desktop id:** `shadow-agent` · **Version:** 0.18.0

```
AGENT HARNESS → MODEL INTERFACE → LLM PROVIDER
```

Providers: Ollama, OpenAI-compatible (OpenAI, xAI/Grok, any `/v1` host),
LM Studio, llama.cpp, vLLM, or mock (tests, no credits). Any free-text model id
works with any provider.

## What's new in 0.18.0

- **Clean desktop UI.** The default view is just the top bar, the transcript,
  the floating composer, and a thin status line. Sessions · Files · Changes ·
  Skills · Goals · Health (doctor + router) · Background all live in one
  right-hand drawer (`Ctrl+B`, closed by default) or the command palette
  (`Ctrl+K`). Light by default, dark toggle, one font stack, no heavy borders.
- **One-click first run.** Detected local models show as tiles; pick one,
  *Test & start*. Under 60 seconds to the first task.
- **Model picker for everything.** Every provider is a group in the composer
  dropdown, detected models are marked, and *Custom model…* accepts any id for
  any provider. Per-task override; the routing table is visible in Health.
- **Goal Mode.** Milestone checklist with progress %, run / resume / abandon
  from the drawer; each milestone is a verified agent task.
- **Rewind from a card.** Expand any file-changing op card → *Rewind* undoes
  exactly that task's edits; *Review diff* jumps to per-hunk accept / reject.
- **Sessions:** search, rename, delete, branch, export as Markdown
  (`shadow sessions`, `shadow export`).
- **`shadow update`** self-updater from the GitHub release tag (or `main`),
  and `shadow doctor --fix` now covers the install *and* the UI build.
- Desktop + browser notification when a task finishes in the background.
- Keyboard cheat sheet (`?`), trimmed TUI status line
  (model · ctx% · tokens · stage).

## Install (Linux)

Requires Python 3.12+ and, for the desktop UI, Node.js.

```bash
git clone https://github.com/ShadowfetchLinux/ShadowCode.git
cd ShadowCode
./scripts/install-linux.sh
```

That installs the `shadow` CLI to `~/.local/bin`, builds the desktop UI, and
writes `~/.local/share/applications/shadow-agent.desktop` with the
`shadow-agent` hicolor icon. Re-running the script upgrades in place; config,
secrets, and the sessions DB are preserved.

Manual:

```bash
pip3 install -e . --user
cd ui && npm install && npm run build
```

First run needs no API key. When Ollama is detected the first installed model
is preselected (this release was built and verified against **gpt-oss:20b**);
otherwise the offline mock is used.

## CLI

```bash
shadow                         # Codex-style TUI (tty) or desktop UI (headless)
shadow /path/to/project        # attach to a project
shadow tui                     # terminal UI
shadow tui -s <session-id>     # resume a session
shadow run "Create a Python hello-world project"
shadow run --json "review this pull request"
shadow run --agent security "audit this workspace"
shadow models                  # configured + detected models
shadow models --use gpt-oss:20b
shadow models --use gpt-4.1 --provider openai_compatible --endpoint https://api.openai.com/v1
shadow config
shadow health
shadow doctor --fix            # repairs wrapper, desktop entry, icons, secrets perms, UI build
shadow update --check          # newer release on GitHub?
shadow update                  # fetch the release tag, reinstall, keep config
shadow sessions [query] [--rename T | --delete]
shadow export --format md
shadow goal "one-line goal" --run
shadow goals --resume <id>
shadow ui                      # desktop UI
shadow ui --no-browser
shadow mcp serve               # MCP over stdio
shadow mcp serve --http 127.0.0.1:7431
shadow mcp register            # JSON blocks for Claude Code / Cursor / Codex
```

Other commands: `background`, `plugin`, `rewind`, `skill`, `status`, `jobs`,
`tools`, `profile`, `understand`, `why`, `vision`, `docs`, `rollback`,
`checkpoints`, `team`.

## Terminal UI

```bash
shadow            # in a tty
shadow tui        # explicit
```

Keybindings: `Enter` send · `Ctrl+J` newline · `Esc` cancel/overlay ·
`Ctrl+R` rerun · `Ctrl+P` sessions · `F2` models · `↑/↓` history · `?` help.
Status line: `model · ctx% · tokens · stage`.

## Slash commands

Built-ins (plus user commands from `.shadow/commands/*.md`):

| Command | Purpose |
| --- | --- |
| `/help` `/shadowcode` `/status` | Orientation and health |
| `/model` `/models` `/router` | Model picker and routing table |
| `/plan` `/compact` `/expand` `/context` | Plan and context meter |
| `/diff` `/review` `/test` `/run` `/git` `/commit` `/undo` | Workspace and git |
| `/agents` `/team` `/mcp` `/memory` `/tools` | Subagents, MCP, memory |
| `/understand` `/goal` `/goals` `/why` | Repo map, goals, explanations |
| `/vision` `/docs` `/profile` | Screenshots, docs research, permission profile |
| `/rollback` `/checkpoints` | Named restore points |
| `/clear` `/new` `/branch` `/sessions` `/resume` `/pin` | Session UX |
| `/cost` `/doctor` `/health` `/settings` `/ui` `/quit` | Diagnostics and exit |

## Desktop UI

Launch from the ShadowCode app icon (`Icon=shadow-agent`) or `shadow ui`.

- URL: http://127.0.0.1:7430 · Health: `curl -s http://127.0.0.1:7430/api/health`
- Default view: top bar · transcript · composer (`+` attach, input, model,
  mode, mic, send) · status line (`model · ctx% · tokens · stage`).
- `Ctrl+B` drawer (Sessions, Files, Changes, Skills, Goals, Health, Background)
  · `Ctrl+K` palette · `Ctrl+,` settings (Model, Permissions, Appearance,
  Hooks, MCP, Plugins) · `?` keys.
- `Enter` sends and clears, `Shift+Enter` newline. Approval cards: `↵` allow,
  `Esc` cancel.

## MCP server

Expose the same harness to Claude Code, Cursor, Codex, or any MCP client:

```bash
shadow mcp serve                          # stdio
shadow mcp serve --http 127.0.0.1:7431    # HTTP/SSE (loopback)
shadow mcp register                       # print client config blocks
```

Optional bearer token lives in `~/.config/shadow-agent/mcp-token` and is
never stored in this repo.

## Config (XDG)

Created on first run. **Never put API keys in YAML or in git.**

| Path | Role |
| --- | --- |
| `~/.config/shadow-agent/config.yaml` | Model, permissions, git, UI, routing |
| `~/.config/shadow-agent/secrets.env` | API keys only (chmod 600) |
| `~/.local/share/shadow-agent/` | Shared data |
| `~/.local/state/shadow-agent/shadow-agent.db` | Sessions, tasks, events |
| `~/.local/state/shadow-agent/goals.db` | Goals and milestones |
| `~/.local/state/shadow-agent/logs/events.jsonl` | Audit log |

Copy [config.example.yaml](config.example.yaml). Name the environment
variable instead of embedding a key (`api_key_env`).

Project overlay: `<workspace>/.shadow/config/config.yaml`  
Skills / memory / commands: `<workspace>/.shadow/{instructions.md,skills/,memory/,commands/}`  
Hooks: `<workspace>/.shadowcode/hooks/*.py`

## Point at a real model

```bash
# Any installed Ollama model (auto-detected)
shadow models --use gpt-oss:20b

# Grok (xAI)
export XAI_API_KEY=...
shadow models --use grok-4 --provider openai_compatible --endpoint https://api.x.ai/v1

# Any OpenAI-compatible host
export OPENAI_API_KEY=...
shadow models --use gpt-4.1 --provider openai_compatible --endpoint https://api.openai.com/v1

# llama.cpp / vLLM / LM Studio
shadow models --use my-model --provider llamacpp --endpoint http://127.0.0.1:8080/v1
shadow models --use my-model --provider vllm --endpoint http://127.0.0.1:8000/v1
shadow models --use my-model --provider local --endpoint http://127.0.0.1:1234/v1
```

## Architecture

The harness owns control flow. Models reason and select tools; they do not
own the filesystem, git, permissions, or “we are done.”

- **Loop:** understand → plan → inspect → reason → tool → observe → verify
- **Success is observed:** tests and command output decide completion
- **Workspace sandbox:** paths resolve under the project root
- **Security levels:** `read_only` · `workspace` (default) · `elevated` (opt-in)

See [ARCHITECTURE.md](ARCHITECTURE.md) for the module map.

## Tests

```bash
python3 -m pytest
```

Default tests use the mock provider (no credits, no network) and run against
throwaway XDG directories — the suite never touches your real config or
sessions.

## License

[MIT](LICENSE) · Copyright 2026 Shadowfetch
