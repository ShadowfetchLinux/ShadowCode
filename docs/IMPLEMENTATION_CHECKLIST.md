# ShadowCode 0.28 implementation checklist

One desktop window, one picker, real backends. This checklist replaces the
0.27 simplification checklist. Every box is ticked only after the code path
was exercised (unit test, integration test, or a recorded live run on this
machine); "written" is not "done".

Baseline audited on 2026-09-23 from `main` (0.27.0). The 0.27 picker, vendor
adapters, managed llama.cpp and safety layers are kept where they were real;
the items below are what was missing, fake, or unreachable.

## 0. Ground truth on this machine (audit)

- [x] Codex 0.155.0-alpha.16 signed in (ChatGPT Pro). `account/rateLimits/read`
      reports two pools (`codex` weekly 98% used; `base_model_inference`
      "gpt-reserve" for gpt-5.6-luna). `model/list` returns 7 models with
      image input. `codex models` is not a subcommand.
- [x] Claude Code 2.1.278 installed, **not signed in** (`claude auth status`
      → loggedIn:false). Live Claude turns cannot be verified here.
- [x] Cursor 2026.09.15 signed in (Free tier). ACP handshake advertises
      image support, 40 models (exact ids with bracketed parameters accepted by
      `session/set_model`; bare names are rejected), modes agent/plan/ask.
      `--model` and `session/set_model` both persist as the CLI default.
- [x] Antigravity 1.2.9 signed in; `agy models` lists 14 models; no `usage`
      subcommand (only the interactive `/usage` panel). Headless input is
      `{event:"user"}` NDJSON; `--print` must be given as `--print=`.
- [x] Grok 1.0.41 signed in (grok.com); `grok models` and ACP `initialize`
      list 4 models; ACP `promptCapabilities.image` is **false**.
- [x] GPU: RTX 5060 Ti 16 GB (driver 580, Vulkan ICD). No nvcc/glslc/cmake
      on PATH; `glslc` available from the user flatpak SDK.
- [x] Local weights: Ollama store `~/models/ollama` holds GGUF blobs for
      qwen3:14b (loadable), gemma-4 12B + vision projector (loadable, vision
      verified), gpt-oss:20b (architecture `gptoss` unknown to llama.cpp).

## 1. Managed local runtime (GPU)

- [x] Rewrite `scripts/build-llama.cpp.sh`: pinned commit, `GGML_BACKEND_DL`,
      all CPU variants, Vulkan module via `tools/glslc-flatpak.sh`, local
      SPIRV-Headers, `$ORIGIN` rpath, atomic install, `architectures.txt`.
- [x] Live: Qwen3 14B tool calls at ~35 tok/s and Gemma 4 12B vision on Vulkan;
      CPU fallback verified with the Vulkan module removed.
- [ ] Ship llama.cpp MIT notice with the runtime; bundle into AppImage and deb
      with relative symlinks; package checks assert the runtime loads.

## 2. Local engine

- [x] `gguf.rs`: header reader (architecture, context, template, tensors,
      projector detection) with memory estimate.
- [ ] `local_engine.rs`: inspect via GGUF metadata; compatibility from
      `architectures.txt` + token embedding tensor; projector pairing by exact
      stem or a single projector in the same folder; `tools` from the chat
      template; memory estimate vs VRAM/RAM; hardware from
      `llama-server --list-devices` (cached).
- [ ] Ollama store import: manifest → model + projector blobs, verified, never
      copied; unsupported architectures listed as incompatible with the reason.
- [ ] `local_runtime.rs`: `--jinja`, `--mmproj`, context from metadata capped
      by memory, `--api-key` per launch, `--no-webui`, stderr drain, cancel
      token, clean stop; one model at a time.
- [ ] Load / unload / status endpoints and Settings controls.
- [ ] Vision gate from the projector, not the file name; `view_image` tool.

## 3. Vendor accounts (subscriptions)

- [x] `codex_probe.rs`: account/read, rateLimits/read, model/list (live).
- [x] `acp_probe.rs`: initialize/authenticate/session/new for Cursor and Grok
      (live), `grok models` parser.
- [ ] Doctor: Codex via probe/`login status`; Grok via `grok models`; Cursor
      via ACP authenticate; Claude via `auth status`; Antigravity via `models`.
      Never Ready because a binary or a file exists.
- [ ] Discovery: Codex model/list; Cursor ACP models (exact ids); Grok ACP;
      Antigravity `models`; Claude default + CLI-documented aliases.
- [ ] Usage: multi-window snapshots (Codex pools, plan, reset, credits only
      when reported); stale/unavailable states; cache with backoff; persisted;
      cleared on disconnect; plan-limit pause.
- [ ] Antigravity adapter rewritten to the documented `event` protocol.
- [ ] Cursor adapter: authenticate, `session/set_model` with exact ids,
      `session/load` resume. Grok rows: no images.
- [ ] Codex adapter: per-turn token usage, `thread/resume`, limit-reached
      handling; exec fallback only before a turn starts.
- [ ] Claude adapter: `--resume`, session id capture, alias models.
- [ ] Native session ids persisted per conversation; explicit handoff when the
      provider changes; consent before local content goes to a cloud route.
- [ ] Accounts page: Connect (official login in the user's browser),
      status, models, usage, Disconnect (with shared-CLI logout warning).

## 4. Native agent (local models)

- [ ] Context window and server context are one number from the GGUF header.
- [ ] `web_fetch` / `web_search` tools with URL validation, private-network
      block, redirects, size and time limits; sources in the activity log.
- [ ] Network mode: online / web tools off / offline (no quota refresh).
- [ ] Two honest permission modes: Ask before actions / Allow project edits;
      vendor mapping explained; read-only auto-denies vendor prompts.
- [ ] Durable tool output redacted; trust gate for goals; checkpoint restore
      writes a transcript event.

## 5. Interface

- [ ] Composer picker fed from `/api/picker` (vendor + local rows), no
      fallback labels, Send disabled without a target; selection stored per
      conversation server-side.
- [ ] Quiet welcome with three suggestions; onboarding without a model step.
- [ ] Activity timeline from real events; final summary card (files, tests).
- [ ] Remove dead controls: ModeTabs, VendorAgentChip, ModelChooser,
      ThinkingCard, CustomModelDialog, OpenWeightHub, FlowGuide, hidden mode
      state; Skills/Goals/Background/Health move under Settings › Advanced.
- [ ] Accounts and Local models pages as above; compact permissions and web
      indicators only when relevant.

## 6. Persistence

- [ ] SQLite `user_version` 25: `execution_targets`, `usage_snapshots`,
      `native_sessions`; backup before migration (existing mechanism).

## 7. Prove it

- [ ] Rust: fmt, clippy `-D warnings`, unit + integration tests (fake
      llama-server, fake vendor binaries for every protocol, usage mapping,
      picker composition, migration).
- [ ] UI: typecheck, vitest, e2e.
- [ ] Live on this machine: Codex probe, Cursor/Grok ACP, Antigravity
      models, local GPU inference + vision, web fetch, offline local run.
- [ ] AppImage built, installed with `scripts/install-appimage.sh`, launched
      from the desktop entry, screenshots reviewed.
- [ ] `docs/IMPLEMENTATION_REPORT.md`, README, user guide, CHANGELOG 0.28.0;
      push to `origin/main`.
