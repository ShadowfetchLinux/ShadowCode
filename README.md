# Shadow Agent

Linux-native autonomous coding-agent **harness**. The LLM is replaceable.
The harness owns planning, context, tools, sandbox, git, memory, permissions,
verification, history, subagent hooks, and the UI.

```
AGENT HARNESS → MODEL INTERFACE → LLM PROVIDER
```

## What's new in 0.3.0

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
shadow                         # attach to cwd, interactive loop
shadow /path/to/project        # attach to a project
shadow run "Create a Python hello-world project"
shadow run "analyze, find failing tests, fix, rerun, summarize"
shadow models
shadow config
shadow config model.default ollama
shadow health                  # provider ping + git/python/docker
shadow sessions
shadow export --format md
shadow ui                      # desktop (same Agent API)
shadow ui --no-browser
```

First run needs no API key. The default model is **mock**. The desktop
opens an onboarding wizard (folder, provider, optional key, permissions)
so a new user can run a task in under a minute.

## Desktop UI

One-click from the Shadow Agent app icon (`Icon=shadow-agent`).

- URL: http://127.0.0.1:7430
- Health: `curl -s http://127.0.0.1:7430/api/health`
- Settings GUI, command palette (`Ctrl+K`), session resume, Stop, diffs,
  git, skills editor, approvals, token usage, transcript export.

## Config (XDG)

Created on first run:

| Path | Role |
| --- | --- |
| `~/.config/shadow-agent/config.yaml` | Model, permissions, git, UI, routing |
| `~/.config/shadow-agent/secrets.env` | API keys only (chmod 600, never YAML) |
| `~/.local/share/shadow-agent/` | Shared data |
| `~/.local/state/shadow-agent/shadow-agent.db` | Sessions, tasks, events, models |
| `~/.local/state/shadow-agent/logs/events.jsonl` | Audit log |

Copy [config.example.yaml](config.example.yaml). **Never put API keys in YAML.**
Name the environment variable instead (`api_key_env`).

Project overlay: `<workspace>/.shadow/config/config.yaml`  
Skills / memory: `<workspace>/.shadow/{instructions.md,skills/,memory/}`

## Point at a real model later

```bash
# Grok (xAI)
export XAI_API_KEY=...
shadow config model.default grok
shadow config model.provider openai_compatible
shadow config model.endpoint https://api.x.ai/v1
shadow config model.api_key_env XAI_API_KEY
shadow config model.name grok-4

# Any OpenAI-compatible host
export OPENAI_API_KEY=...
shadow config model.default openai
shadow config model.endpoint https://api.openai.com/v1

# Ollama (this machine already speaks /v1)
shadow config model.default ollama
shadow config model.provider ollama
shadow config model.endpoint http://127.0.0.1:11434/v1
shadow config model.name gpt-oss:20b

# llama.cpp / vLLM / LM Studio
shadow config model.provider llamacpp   # :8080/v1
shadow config model.provider vllm       # :8000/v1
shadow config model.provider local      # :1234/v1
```

## Tests

```bash
cd ~/src/ShadowAgent
python3 -m pytest
```

All default tests use the mock provider (no credits).

## Security levels

`read_only` · `workspace` (default) · `elevated` (opt-in).  
Dangerous commands, `sudo`, network, and history-destroying git stay blocked
unless policy allows them. Paths cannot leave the workspace.

## License

MIT
