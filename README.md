# Shadow Agent

Linux-native autonomous coding-agent **harness** with a Codex-style terminal
composer. The LLM is replaceable. The harness owns planning, context, tools,
sandbox, git, memory, permissions, verification, history, subagent hooks,
the desktop UI, and the terminal UI.

```
AGENT HARNESS → MODEL INTERFACE → LLM PROVIDER
```

## What's new in 0.5.0

- **Codex light-mode desktop UI** (now the default): white canvas, very light
  gray borders, lots of whitespace, rounded floating composer, user prompts as
  small pill bubbles top-right with a category tag, agent output as plain text
  blocks (no heavy bubbles), permission/action cards with `Allow ↵` /
  `Cancel Esc` buttons. Dark mode is still available as a toggle in Settings.
- **Enter submits the prompt** in the composer; Shift+Enter inserts a newline.
  Multiline is still supported. The placeholder reads "Ask for follow-up
  changes…".
- **Abilities (Computer Use / Custom) live in Settings**, not the composer.
  The composer stays clean: `+` attach icon, input, model dropdown, mode
  dropdown (coder/researcher/reviewer/tester), mic, solid black submit square.
  Below it: a "Work locally" checkbox line and a slim status line
  (model · workspace · level · tokens · working indicator).
- **Per-task mode** (purpose) wired through `/api/jobs` so the harness can
  route coder/researcher/reviewer/tester.

## What's new in 0.4.0

- **Codex-style terminal UI** (default landing experience when a tty is
  attached): centered composer (Enter sends, Ctrl+J newline), scrollback
  transcript with collapsible tool cards, Codex-style diff/proposal cards
  with Accept/Reject hints, inline approval cards with risk labels, compact
  status line (model · ctx% · tokens · step · working indicator), light/dark
  themes that respect the system scheme, keyboard-first navigation.
- **Slash command system**: built-ins (`/help /clear /new /model /config
  /compact /undo /diff /git /branch /sessions /resume /pin /cost /doctor
  /health /ui /quit`) plus user-defined commands loaded from
  `.shadow/commands/*.md` (YAML front-matter + alias supported).
- **Context meter + auto-compact** that summarizes older turns.
- **Session branching** (fork to try a path), **pin/bookmark** messages,
  per-session and per-task **cost/tokens**, resume any session into the TUI.
- **Smarter doctor with auto-fix**: `shadow doctor --fix` repairs
  secrets.env permissions, reinstalls wrapper/desktop entry/icons, and
  regenerates a broken config.yaml.
- **Fully general model picker**: any provider (mock, OpenAI-compatible,
  Grok/xAI, Ollama, LM Studio, llama.cpp, vLLM, custom) selectable in
  onboarding, settings, and per-task; free-text model id with presets +
  detected list; `shadow models --use <id> [--provider ...] [--endpoint ...]`
  works for any model. Default stays qwen3:14b but the user can switch to
  gpt-oss:20b, a Grok model, an OpenAI model, etc. with one click.
- **Codex light theme** added to the desktop UI alongside the dark theme.

## What was new in 0.3.0

- **First-class Ollama**: native `/api/chat` adapter with `think: false`
  (≈15× faster than the /v1 shim on thinking models like qwen3), real
  tool-calling, and token usage from `prompt_eval_count`/`eval_count`.
- **Auto-detect local servers**: Ollama, LM Studio, llama.cpp, and vLLM are
  probed on startup; installed models appear in the UI model picker with
  capabilities (tools/thinking), size, and context length.
- **One-click test-and-save**: every provider preset in onboarding and
  Settings has a live "Test connection" button; clicking a model makes it
  the default.
- **Per-task model override** in the composer, plus routing hints
  (`routing.planner/coder/...`) for purpose-based model selection.
- **Retry with backoff** on transient provider errors (429/5xx/connection),
  visible as `model.retry` events and UI toasts.
- **Workspace trust dialog** on first open of a new folder.
- **Desktop notification** (`notify-send`) when a long task finishes.
- **Diff hunk accept/reject** (stage or revert one hunk via `git apply`).
- **Click-to-rerun** any `exec` command from the tools panel.
- **`shadow doctor`**: deep install/config checks with auto-fix suggestions.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the control-flow and module map.

## Install (Pop!_OS / Ubuntu / Debian)

```bash
cd ~/src/ShadowAgent
./scripts/install-linux.sh
```

That installs the `shadow` CLI to `~/.local/bin`, builds the desktop UI, and
writes `~/.local/share/applications/shadow-agent.desktop`. It does **not** pin
the app to the dock.

Manual:

```bash
pip3 install -e ~/src/ShadowAgent --user
cd ~/src/ShadowAgent/ui && npm install && npm run build
```

## CLI

```bash
shadow                         # Codex-style TUI (tty) or desktop UI (headless)
shadow /path/to/project        # attach to a project
shadow tui                     # open the Codex-style terminal UI
shadow tui -s <session-id>     # resume a session into the TUI
shadow run "Create a Python hello-world project"
shadow run "analyze, find failing tests, fix, rerun, summarize"
shadow models                  # list configured + detected models
shadow models --use qwen3:14b  # set default to any model
shadow models --use gpt-4.1 --provider openai_compatible --endpoint https://api.openai.com/v1
shadow config
shadow config model.default ollama
shadow health                  # provider ping + git/python/docker
shadow doctor --fix            # deep checks + apply safe auto-fixes
shadow sessions
shadow export --format md
shadow ui                      # desktop 4-panel UI (same Agent API)
shadow ui --no-browser
```

First run needs no API key. The default model is **qwen3:14b on Ollama** when
Ollama is detected, else mock. The desktop UI opens an onboarding wizard
(folder, provider, optional key, permissions) so a new user can run a task
in under a minute.

## Desktop UI

One-click from the Shadow Agent app icon (`Icon=shadow-agent`).

- URL: http://127.0.0.1:7430
- Health: `curl -s http://127.0.0.1:7430/api/health`
- Settings GUI with provider presets + detected models + free-text custom
  model registration, command palette (`Ctrl+K`), session resume + branch,
  Stop, diffs, git, skills editor, approvals, token usage, transcript
  export, Codex light/dark themes.

## Terminal UI (Codex-style)

```bash
shadow            # in a tty, opens the Codex-style composer
shadow tui        # explicit
```

Keybindings: `Enter` send · `Ctrl+J` newline · `Esc` cancel/overlay ·
`Ctrl+R` rerun · `Ctrl+P` sessions · `F2` models · `↑/↓` history · `?` help.
Slash commands listed above; custom commands live in `.shadow/commands/`.

## Config (XDG)

Created on first run:

| Path | Role |
| --- | --- |
| `~/.config/shadow-agent/config.yaml` | Model, permissions, git, UI, routing |
| `~/.config/shadow-agent/secrets.env` | API keys only (chmod 600, never YAML) |
| `~/.local/share/shadow-agent/` | Shared data |
| `~/.local/state/shadow-agent/shadow-agent.db` | Sessions, tasks, events, models, pins |
| `~/.local/state/shadow-agent/logs/events.jsonl` | Audit log |

Copy [config.example.yaml](config.example.yaml). **Never put API keys in YAML.**
Name the environment variable instead (`api_key_env`).

Project overlay: `<workspace>/.shadow/config/config.yaml`  
Skills / memory / commands: `<workspace>/.shadow/{instructions.md,skills/,memory/,commands/}`

## Point at a real model later

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

## Tests

```bash
cd ~/src/ShadowAgent
python3 -m pytest
```

All default tests use the mock provider (no credits). 80 tests green.

## Security levels

`read_only` · `workspace` (default) · `elevated` (opt-in).  
Dangerous commands, `sudo`, network, and history-destroying git stay blocked
unless policy allows them. Paths cannot leave the workspace.

## License

MIT
