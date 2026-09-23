# ShadowCode 0.27 product simplification

Visual target: the two six-panel desktop boards (home, unified picker, accounts, activity, review, local models). Names, percents, and pass counts in those boards are layout examples only.

## Status

- [x] Inspect origin/main (0.26.0 already shipped) and keep reliable vendor CLI code
- [x] One searchable composer picker: Subscriptions + On this computer
- [x] Cursor ACP adapter (`cursor-agent acp`) and Antigravity `agy` stream-json adapter
- [x] Keep Grok adapter; do not feature it in the primary picker
- [x] Truthful usage snapshots (unknown/stale/shared; never invent percents)
- [x] Built-in GGUF catalog + managed llama.cpp runtime (no auto-download)
- [x] Image attach only when the selected route actually forwards vision bytes
- [x] Compact activity timeline; Mode tabs / chip stack off the default composer
- [x] Accounts + Local models settings (official login commands, no password forms)
- [x] Deterministic tests for picker, usage, image forwarding, cancel, new protocols
- [x] Version 0.27.0, docs, AppImage install, push origin/main

## Honest limits

- Antigravity 1.2.8 has no `usage` command; quota stays "Usage unavailable"
- Cursor `about` reports plan tier, not remaining allowance — do not render a percent
- llama.cpp is bundled as a managed CPU binary; Ready after that binary is installed and a user-owned GGUF is cataloged
- Ollama tags stay optional compatibility rows, never treated as GGUF
- Vendor CLIs own their tool loops. Codex/Claude/Cursor receive official image bytes. Antigravity rejects images.
- No chat GGUF existed on this machine; live generation against user weights was not proven
