# ShadowCode

Linux-native, model-agnostic coding-agent **harness** — a Claude Code / Codex
alternative that you run on your machine. The LLM is replaceable. The harness
owns tools, permissions, context, memory, checkpoints, hooks, skills, MCP,
verification, and routing.

**Brand:** ShadowCode · **CLI:** `shadow` · **Desktop id:** `shadow-agent`

```
AGENT HARNESS → MODEL INTERFACE → LLM PROVIDER
```

Providers: Ollama, OpenAI-compatible (OpenAI, xAI/Grok, …), LM Studio,
llama.cpp, vLLM, or mock (tests, no credits).

## Install (Linux)

Requires Python 3.12+ and, for the desktop UI, Node.js.

```bash
git clone https://github.com/ShadowfetchLinux/ShadowCode.git
cd ShadowCode
./scripts/install-linux.sh
```

That installs the `shadow` CLI to `~/.local/bin`, builds the desktop UI, and
writes `~/.local/share/applications/shadow-agent.desktop`.

Manual:

```bash
pip3 install -e . --user
cd ui && npm install && npm run build
```

First run needs no API key. The default model is **qwen3:14b on Ollama** when
Ollama is detected, otherwise mock. The desktop UI opens an onboarding wizard
(folder, provider, optional key, permissions).

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
shadow models --use qwen3:14b
shadow models --use gpt-4.1 --provider openai_compatible --endpoint https://api.openai.com/v1
shadow config
shadow config model.default ollama
shadow health
shadow doctor --fix
shadow sessions
shadow export --format md
shadow ui                      # desktop UI
shadow ui --no-browser
shadow mcp serve               # MCP over stdio
shadow mcp serve --http 127.0.0.1:7431
shadow mcp register            # JSON blocks for Claude Code / Cursor / Codex
```

Other commands: `background`, `plugin`, `rewind`, `skill`, `goal`, `goals`,
`status`, `jobs`, `tools`, `profile`, `understand`, `why`, `vision`, `docs`,
`rollback`, `checkpoints`, `team`.

## Terminal UI

```bash
shadow            # in a tty
shadow tui        # explicit
```

Keybindings: `Enter` send · `Ctrl+J` newline · `Esc` cancel/overlay ·
`Ctrl+R` rerun · `Ctrl+P` sessions · `F2` models · `↑/↓` history · `?` help.

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

- URL: http://127.0.0.1:7430
- Health: `curl -s http://127.0.0.1:7430/api/health`
- Settings with provider presets, detected models, command palette (`Ctrl+K`),
  session resume + branch, Stop, diffs, git, skills editor, approvals,
  token usage, transcript export, light/dark themes.

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
| `~/.local/state/shadow-agent/logs/events.jsonl` | Audit log |

Copy [config.example.yaml](config.example.yaml). Name the environment
variable instead of embedding a key (`api_key_env`).

Project overlay: `<workspace>/.shadow/config/config.yaml`  
Skills / memory / commands: `<workspace>/.shadow/{instructions.md,skills/,memory/,commands/}`

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

Default tests use the mock provider (no credits, no network).

## License

[MIT](LICENSE) · Copyright 2026 Shadowfetch
