# 0.26 simplification report

ShadowCode is a local desktop coding agent again: one window, one picker, real
backends. This supersedes the remote agent-management direction.

## What changed

- Composer has one searchable picker (Subscriptions / On this computer) plus
  Connect account and Add local model.
- Cursor uses official ACP (`cursor-agent acp`). Antigravity uses official `agy`
  `--print --output-format stream-json`. Codex and Claude Code adapters are
  unchanged in protocol. Grok ACP remains but is not featured.
- Usage never invents percents. Unknown is "Usage unavailable".
- Local GGUF catalog (user-selected files/folders only). llama.cpp is optional.
  Ollama stays compatibility-only and is not treated as GGUF.
- Home is a quiet welcome with four prompt chips. Activity is one timeline.
  Settings starts with Accounts and Local models.

## Subscription integrations (this machine)

| Adapter | Runtime | Discovery | Usage | Notes |
| --- | --- | --- | --- | --- |
| Codex | `codex` 0.155.0-alpha.16 | Doctor + optional `codex models` | Unavailable unless official JSON reports remaining | app-server unchanged |
| Claude Code | `claude` 2.1.278 | Doctor (`auth status`) | Unavailable | official stream-json; disable flag remains |
| Cursor | `cursor-agent` 2026.09.15-d2fe57e | `--list-models` when Ready | `about` shows plan tier only — not rendered as a percent | `cursor-agent status` for login |
| Antigravity | `agy` 1.2.8 | `agy models` when Ready | No `usage` command on 1.2.8 | GUI `antigravity` is not the CLI |
| Grok | kept | not featured | — | existing ACP adapter |

## Local models

- GGUF magic is required. Removing a catalog row does not delete the file.
- This machine has Ollama tags `qwen3:14b`, `gpt-oss:20b`,
  `huihui_ai/gemma-4-abliterated:12b`. Those blobs are not auto-imported as GGUF.
- No `llama-cli` / `llama-server` on PATH. Local GGUF rows stay Setup required
  until the user points at an official binary. Not proven against a live GGUF
  load on this box.
- Hardware probe can read CPU/RAM and `nvidia-smi` when present.

## Tests and packaging

See the release commit message and CI logs. Required local commands:
`cargo build -p shadowcode-desktop --locked`,
`cargo test -p shadowcode-core --tests --locked`,
UI vitest + tsc on Node 22.22.3, `pytest tests/` if the venv exists,
`scripts/test-install-appimage.sh`.

## Genuine limits

- No hosted ShadowCode account or cloud orchestration.
- Vendor image bytes are not forwarded yet; those routes reject attachments.
- Antigravity quota API is not in CLI 1.2.8.
- llama.cpp inference is catalog + setup, not a proven in-process engine.
