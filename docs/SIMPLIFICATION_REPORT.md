# 0.27 simplification report

ShadowCode is a local desktop coding agent: one window, one picker, real
backends. This supersedes the remote agent-management direction. 0.26.0 shipped
the picker. 0.27.0 makes local inference real without an Ollama or LM Studio
daemon.

## What changed

- Composer has one searchable picker (Subscriptions / On this computer) plus
  Connect account and Add local model.
- Cursor uses official ACP (`cursor-agent acp`). Antigravity uses official `agy`
  `--print --output-format stream-json`. Codex and Claude Code adapters are
  unchanged in protocol. Grok ACP remains but is not featured.
- Usage never invents percents. Unknown is "Usage unavailable".
- Local GGUF catalog (user-selected files/folders only). Removing a catalog row
  does not delete the file. Ollama stays compatibility-only and is not treated
  as GGUF.
- A pinned llama.cpp is compiled from source and installed to
  `~/.local/lib/shadowcode/llama-server`. The engine resolves that managed
  binary before PATH.
- Home is a quiet welcome with four prompt chips. Activity is one timeline.
  Settings starts with Accounts and Local models.

## Subscription integrations (this machine)

| Adapter | Runtime | Discovery | Usage | Notes |
| --- | --- | --- | --- | --- |
| Codex | `codex` 0.155.0-alpha.16 | Doctor + optional `codex models` | Unavailable unless official JSON reports remaining | app-server image input is `localImage` |
| Claude Code | `claude` 2.1.278 | Doctor (`auth status`) | Unavailable | official stream-json image source blocks |
| Cursor | `cursor-agent` 2026.09.15-d2fe57e | `--list-models` when Ready | `about` shows plan tier only — not rendered as a percent | ACP `image` content blocks |
| Antigravity | `agy` 1.2.8 | `agy models` when Ready | No `usage` command on 1.2.8 | images still rejected |
| Grok | kept | not featured | — | existing ACP adapter, same image blocks as Cursor |

## Local models

- GGUF magic is required. Removing a catalog row does not delete the file.
- This machine has Ollama tags `qwen3:14b`, `gpt-oss:20b`,
  `huihui_ai/gemma-4-abliterated:12b`. Those blobs are not auto-imported as GGUF.
- Vocab-only and embedding GGUFs found under `~/.unsloth` and LM Studio were
  not added to the catalog. They are not chat weights.
- Managed llama.cpp is a CPU build (no nvcc; Vulkan lacks glslc/shaderc).
  Upstream commit `18f9f7bef960b76b693d8dcbb33cbbd6148c1631`
  (`version: 0.4.1-dev`, reported as `18f9f7bef`). Pin: `tools/llama.cpp.pin`.
  Install path: `~/.local/lib/shadowcode/llama-server` with `$ORIGIN` rpath.
- One model is loaded at a time on 127.0.0.1. Local vision is true only when an
  mmproj companion file is present next to the GGUF.
- Live CPU generation smoke (2026-09-23): official
  `Qwen/Qwen2.5-0.5B-Instruct-GGUF` `qwen2.5-0.5b-instruct-q4_k_m.gguf`
  (491400032 bytes, sha256
  `74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db`,
  URL
  `https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf`)
  at `~/.local/share/shadowcode/models/`. Added through
  `POST /api/local-models/add` / `inspect_gguf`. `GET /api/picker` listed it
  under group `local` as `qwen2.5-0.5b-instruct-q4_k_m · This computer` with
  subtitle `Runs on this computer · No subscription quota` (UI section
  "On this computer"). Vision false (no mmproj). Managed
  `~/.local/lib/shadowcode/llama-server` (CPU build, commit `18f9f7bef`)
  loaded the GGUF on 127.0.0.1 and returned: "Python's zip() function
  returns an iterator that yields tuples containing elements from each of
  the iterables passed to it." `llama-cli` on the same file answered the
  same question. GPU was visible to `nvidia-smi` (RTX 5060 Ti) but this
  runtime has no CUDA/Vulkan; tokens were CPU. No AppImage rebuild.

## Tests and packaging

Quoted local runs on this machine (not claimed without running):

- `cargo build -p shadowcode-desktop --locked --offline`: finished `dev` profile
  for `shadowcode-desktop v0.27.0`.
- `cargo test -p shadowcode-core --tests --locked --offline`: 372 passed, 0
  failed across the crate's integration test binaries.
- `cargo test -p shadowcode-core --lib local_engine --locked --offline`: 2
  passed (catalog remove does not delete weights; managed binary wins over PATH).
- UI on nvm Node v22.22.3 (not `/usr/bin/node`): vitest `18` files / `59`
  tests passed; `npx tsc --noEmit` exit 0.
- `.venv/bin/pytest tests/ -q --tb=no`: 316 passed, 0 failed.
- `scripts/test-install-appimage.sh`: passed after the installer copies the
  managed llama-server when `packaging/llama.cpp/bin` is present.

AppImage build and install happen after this commit. Binaries under
`packaging/llama.cpp/bin/` and the llama.cpp source clone are not in git.

## Genuine limits

- No hosted ShadowCode account or cloud orchestration.
- Antigravity quota API is not in CLI 1.2.8. Antigravity images stay rejected.
- The 0.5B Q4_K_M smoke proves load + text generation only. It is not a
  coding-quality model and was not used to claim agent/tool-calling quality.
- GPU llama.cpp was not built: CUDA toolkit is absent and Vulkan shader tools
  are missing. CPU inference is what ships.
