# Shadow Agent Architecture

Linux-native autonomous coding-agent **harness**. The LLM is replaceable.
The harness is the product.

```
CLI / Desktop UI
       │
   Agent API  (same runtime)
       │
   Agent Loop
       │
 Context │ Tools │ Permissions │ Memory │ Plan │ Verify
       │
 Model Interface  (generate / stream / chat / tool_call)
       │
 Providers: mock | openai-compatible | local | ollama | llama.cpp | vLLM
```

## Stack

| Layer | Choice |
| --- | --- |
| Runtime | Python 3.12 |
| Config / schema | pydantic + PyYAML |
| CLI | Typer (`shadow`) |
| Persistence | sqlite3 + Markdown memory |
| HTTP API | FastAPI + uvicorn (loopback) |
| Desktop | Vite + React, served by the same API |
| Tests | pytest |

XDG only: `~/.config/shadow-agent/`, `~/.local/share/shadow-agent/`,
`~/.local/state/shadow-agent/`. API keys come from environment variables
named in config — never from files in this repo.

## Principles

1. **Harness owns control flow.** Models reason and select tools. They do
   not own filesystem, git, permissions, or “we are done.”
2. **Loop, not prompt→response.** Understand → plan → inspect → reason →
   tool → observe → update context → verify → continue or finish.
3. **Success is observed.** Tests and command output decide completion.
4. **Workspace sandbox.** Paths resolve under the project root. Terminal
   cwd is the workspace. Destructive / network / root actions need
   ELEVATED and user-enabled policy.
5. **Linux-first.** No Windows/macOS path assumptions.

## Packages (`src/shadow_agent/`)

| Module | Responsibility |
| --- | --- |
| `paths`, `config` | XDG, YAML, defaults, overlays |
| `events`, `store` | Event bus + SQLite (sessions, tasks, events, models, projects) |
| `permissions` | READ_ONLY / WORKSPACE / ELEVATED + command policy |
| `models/` | Provider ABC, registry, adapters |
| `tools/` | FS, terminal, git, search + sandbox |
| `context/` | Prioritize / compress / truncate / memory |
| `planning/` | Observable, updatable plans |
| `verification/` | Test loop + error recovery |
| `agent/` | Main loop + subagent hooks + routing |
| `api/` | HTTP/SSE used by CLI and UI |
| `plugins/` | Tool/provider/event hooks; MCP reserved |

## Model interface

Every provider implements `generate`, `stream`, `chat`, `tool_call`,
`get_capabilities`, `get_context_limit`. Internal types are
`ToolCall` and `ToolResult`. Adapters map vendor protocols.

V1 ships **mock** (CI, no credits) and **openai-compatible** (Grok, OpenAI,
any `/v1/chat/completions` host). Local / Ollama / llama.cpp / vLLM are
thin adapters over the same protocol with different default endpoints.

## Memory (provider-agnostic)

| Kind | Location |
| --- | --- |
| Project | `<workspace>/.shadow/memory/` + `instructions.md` / `skills/` |
| Task | state dir `tasks/<id>/memory.md` |
| Session | SQLite + JSONL event log |

## Security levels

- **READ_ONLY** — list/read/search/git inspect
- **WORKSPACE** — write/edit/exec (non-dangerous)/git add+commit
- **ELEVATED** — network, destructive git, dangerous commands (opt-in)

Audit events are always recorded.

## UI

The desktop is a client of the Agent API, not a second agent.
Layout: Sessions/Projects/Models | Conversation/Plan/Tools | Files/Diff/Git/Skills/Health.
Jobs run in a background thread. The UI streams events over SSE, can cancel
the loop, and can approve or deny dangerous commands.

## Milestones

M1 foundation → M2 tools → M3 git/search/edit → M4 context/memory →
M5 plan/verify → M6 remote OpenAI-compatible → M7 desktop → M8 subagents →
M9 local server → M10 Ollama/llama.cpp/vLLM → M11 routing → M12 events UI.
