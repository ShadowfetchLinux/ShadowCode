# ShadowCode 0.26 product simplification

Visual target: the two six-panel desktop boards (home, unified picker, accounts, activity, review, local models). Names, percents, and pass counts in those boards are layout examples only.

## Status

- [x] Inspect origin/main (0.25.0 already shipped) and keep reliable vendor CLI code
- [x] One searchable composer picker: Subscriptions + On this computer
- [x] Cursor ACP adapter (`cursor-agent acp`) and Antigravity `agy` stream-json adapter
- [x] Keep Grok adapter; do not feature it in the primary picker
- [x] Truthful usage snapshots (unknown/stale/shared; never invent percents)
- [x] Built-in GGUF catalog + optional llama.cpp spawn (no auto-download)
- [x] Image attach only when the selected route actually supports vision
- [x] Compact activity timeline; Mode tabs / chip stack off the default composer
- [x] Accounts + Local models settings (official login commands, no password forms)
- [x] Deterministic tests for picker, usage, image rejection, cancel, new protocols
- [x] Version 0.26.0, docs, AppImage install, push origin/main

## Honest limits

- Antigravity 1.2.8 has no `usage` command; quota stays "Usage unavailable"
- Cursor `about` reports plan tier, not remaining allowance — do not render a percent
- llama.cpp is not bundled; Ready only after the user points at a verified binary and a GGUF they already have
- Ollama tags stay optional compatibility rows, never treated as GGUF
- Vendor CLIs own their tool loops; images on those routes are rejected until the adapter passes real image bytes
