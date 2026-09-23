ShadowCode 0.27.0 keeps the desktop coding-agent picker and adds a real
managed local runtime. Local GGUF files you already have load through
`~/.local/lib/shadowcode/llama-server`. ShadowCode does not start Ollama or
LM Studio. Weights are never auto-downloaded or deleted when a catalog row is
removed. Codex, Claude Code, and Cursor receive official image bytes; Antigravity
still rejects attachments.

Downloads: `ShadowCode_0.27.0_amd64.AppImage`, `ShadowCode_0.27.0_amd64.deb`.

ShadowCode 0.26.0 is a desktop coding agent: open a project, pick one
execution target, describe the task, watch it work. The composer has a single
searchable picker for Codex, Claude Code, Cursor, Antigravity, and local
models. Usage is shown only when a provider actually reports it. Local GGUF
files you already have can be added to a catalog; weights are never deleted or
auto-downloaded.

Downloads: `ShadowCode_0.26.0_amd64.AppImage`, `ShadowCode_0.26.0_amd64.deb`.

ShadowCode 0.25.0 ("Supreme") delivers a major flagship UX upgrade designed
to simplify the onboarding flow and elevate workspace feel.

- **WelcomeBanner & Quickstarts:** 8 instant starter cards for building, bug fixing,
  codebase explanation, test authoring, change reviews, refactoring plans, security
  audits, and performance optimization. One click populates the composer and sets
  the optimal mode.
- **ModeTabs:** Accessible icon-tabs with hover descriptions replacing the plain
  dropdown mode selector.
- **Header & Status Bar Polish:** Minimalist icon controls, and a compact 48 px
  visual context meter that indicates usage at a glance and turns warning-red
  above 80%.
- **Design & Typography:** Subpixel font smoothing, typographic ligatures, focus
  glow rings, and seamless dark mode support.

Downloads: `ShadowCode_0.25.0_amd64.AppImage`, `ShadowCode_0.25.0_amd64.deb`,
the AppImage runtime source archive, and `SHA256SUMS`. Binaries target x86_64,
Ubuntu 24.04+ / glibc 2.39+.
