# ShadowCode 0.28 implementation report

Date: 2026-09-23. Base: `main` at 0.27.0 (`d1b685c`). Machine: Linux, RTX 5060 Ti
16 GB (Vulkan), 62 GB RAM, 16 threads.

## What changed

- **One picker.** The composer has a single searchable dropdown with two groups,
  *Subscriptions* and *On this computer*. Rows are real execution targets with
  stable ids (`cli:codex:gpt-6-astra`, `cli:cursor:auto`, `local:gguf:<hash>`),
  availability (Ready / Sign in / Setup required / Unavailable), usage as
  reported, and vision / Chat-only badges only when verified. The choice is
  remembered per conversation and per project on the engine side.
- **Subscriptions through the official CLIs.** Codex (`codex app-server`),
  Claude Code (`claude -p … stream-json`), Cursor (`cursor-agent acp`),
  Antigravity (`agy --print= stream-json`, rewritten to the documented `event`
  protocol) and Grok (`grok agent stdio`, ACP). A shared vendor catalog asks
  each CLI for sign-in state, models and usage through documented commands; a
  binary on disk never makes a row Ready. Native session ids are stored per
  conversation and resumed. Provider API-key variables are removed from every
  vendor process so a subscription row can never bill an API key.
- **Accounts page.** Connect runs the official login in the user's browser;
  Disconnect runs the official logout after a confirmation that explains it
  signs out the shared CLI. No password forms.
- **Truthful usage.** Codex rate-limit pools (window, reset time, shared pool,
  credits only when reported) are persisted (SQLite `user_version` 25, backup
  before migration) and refreshed with backoff; a plan limit stops the job with
  `limit_reached`. Other vendors show "Usage unavailable · Open … usage" with the
  reason. Offline mode starts no vendor process.
- **Switching models.** Changing provider mid-conversation asks for consent and
  hands a bounded summary (≤ 12 000 characters) to the new runtime. A running
  turn keeps its target; the switch applies to the next message.
- **Built-in local engine.** A pinned llama.cpp built with loadable CPU variants
  and a Vulkan module ships in the AppImage and deb (`usr/lib/shadowcode`) and is
  installed to `~/.local/lib/shadowcode`. GGUF metadata (not file names) decides
  compatibility, context, tool support ("Chat only" otherwise), vision (paired
  projector) and a memory estimate against VRAM or RAM. Ollama store models are
  imported by reference, never copied. One model is loaded at a time, behind a
  lease, on loopback with a per-launch key, with no web UI and no orphans.
- **Local agent.** The native loop now runs on the local engine with matching
  context, tool calls in the model's own template, `view_image` for vision
  models, and `web_fetch` / `web_search` when web is on for the task.
- **Web tools.** Private-network and metadata addresses blocked after DNS
  resolution, redirects re-checked, size and time limits, page text framed as
  untrusted data, sources recorded. Network modes: online / web tools off /
  offline.
- **Permissions.** Two modes: *Ask before actions* (default for new installs)
  and *Allow project edits*; destructive, privileged and out-of-project actions
  always ask or are refused. Stored tool output is redacted.
- **Simpler interface.** Quiet welcome with three suggestions, onboarding
  without a model step, one activity timeline built from real events, a final
  summary of changed files and checks, Settings with Accounts, Local models,
  Permissions & network, Appearance, Advanced (goals, skills, background, MCP,
  plugins, worktrees and diagnostics moved here). Mode tabs, the provider chip
  stack, starter grid, open-weight hub and other dead controls were removed.
- **Removed.** The legacy 0.19 Python harness and its build/test plumbing.

## Subscription integrations (verified live on this machine)

| Runtime | State | Live turn | Resume | Images | Approvals reach ShadowCode | Usage data |
| --- | --- | --- | --- | --- | --- | --- |
| Codex 0.155.0-alpha.16 | Ready (ChatGPT Pro) | yes, 3.6 s | `thread/resume` | yes (`localImage`) | yes | weekly window, reset, shared pool, credits (live: 2% left) |
| Claude Code 2.1.278 | Sign in | not possible (not signed in) | `--resume` (fixture-tested) | yes (image blocks) | yes (host prompts) | not exposed |
| Cursor 2026.09.15 | Ready (Free) | yes, 8.8 s + 7.9 s | `session/load` | yes (ACP) | yes | plan tier only |
| Antigravity 1.2.9 | Ready | yes, 6.3 s + 6.8 s | `--conversation` | no (text only) | no (its own settings) | not exposed |
| Grok 1.0.41 | Ready | yes, 8.4 s + 2.6 s | `session/load` | no (ACP reports none) | yes | per-session tokens only |

A Grok → Cursor switch in one conversation first returned a consent request and
wrote nothing; after consent Cursor received a 314-character summary and
answered from the earlier turn.

## Local model capabilities (verified live)

| Model (Ollama store, by reference) | Runs | Speed | Tools | Vision |
| --- | --- | --- | --- | --- |
| qwen3:14b | Vulkan, 16k context | ~36 tok/s | fixed a failing test (8 steps); `web_fetch` task 11.9 s | no projector |
| gemma-4 12B + projector | Vulkan | ~37.6 tok/s | tool calls | answered "Red" for a red PNG (attachment and `view_image`) |
| gpt-oss:20b | refused: "unsupported architecture gptoss" | – | – | – |

Local runs worked with every proxy pointed at a dead port (no network). With
the Vulkan module removed, the runtime falls back to the CPU.

## Tests run

See the final numbers at the end of this report ("Final verification").

## Genuine external limits

- Claude Code is not signed in on this machine; its live path is covered only
  by protocol fixtures.
- Antigravity and Claude expose no machine-readable plan usage; Cursor reports
  only the plan tier; Grok reports per-session tokens. ShadowCode shows
  "Usage unavailable" for them.
- Antigravity applies its own permission settings in print mode, so its
  actions cannot be approved in ShadowCode; the UI says so at task start.
- DuckDuckGo answers automated requests from this machine with a bot check.
  `web_search` reports it as blocked and never invents results; a user-run
  SearXNG instance can be set as `network.searxng_url`. `web_fetch` works.
- The llama.cpp runtime links the system OpenSSL, libgomp and (optionally)
  Vulkan loader; the deb declares them.
- The system `rustdoc` on this machine cannot load its LLVM library; doctests
  were run with the rustup 1.95.0 toolchain instead.

## Built application

Filled in by the final verification below.
