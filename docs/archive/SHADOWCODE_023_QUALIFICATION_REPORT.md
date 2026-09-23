# ShadowCode 0.23 qualification

Qualification date: 2026-09-22. Linux x86_64, Rust 1.95.0 (system Cargo; Clippy
and rustfmt were not installed on the qualifying host, so those checks are left
to CI), Node 22.22.3 for the UI, Python 3.12 for the legacy test suite.

This release audits the flagship feature set that shipped in 0.22 against the
code and deepens it where behaviour, tests or documentation disagreed.

## Verified behavior

- 351 native Rust tests pass across 34 test binaries (`cargo test -p
  shadowcode-core --tests --locked`), including the real-desktop process tests
  against a freshly built 0.23.0 executable.
- 51 UI tests pass (Vitest) and `tsc --noEmit` is clean.
- 316 legacy Python tests pass (`pytest tests/`), including the canonical
  version check across the Python package, README and desktop entry.
- The AppImage installer check passes in an isolated home, including the new
  `X-ShadowCode-GitSha` line from both the checkout and an explicit override.
- `shadowcode --version` prints `ShadowCode 0.23.0`; `shadowcode --json doctor`
  reports 21 checks with no failures on the qualifying host, where bubblewrap
  is installed and its namespace probe succeeds.

## Feature audit

- **AST symbol tools** — tree-sitter definitions, references, signatures and
  bounded callers for Rust and TypeScript/TSX in a private SQLite cache; no
  embeddings or vector store. `touch` now deletes by the normalized path.
- **Shell isolation** — bubblewrap profile with read-only home, ephemeral
  scratch, network off unless permitted, probe before use, no replay after a
  sandbox failure. Doctor reports honest fallback. Scratch cleanup failures no
  longer discard a command's result. Not kernel-proof; no OverlayFS/Landlock.
- **Parallel worktrees** — at most two workers plus the lead; unclean merges
  reported, never hidden; disabled outside Git. Cleanup now recovers from a
  checkout deleted by hand while retaining every branch.
- **Live steering** — pause, steer, resume, rewind; steering text enters the
  next model context; manual-edit hashes are noticed; rewind never replays
  shell. A rejected pause on a queued task no longer parks it later.
- **Ollama residency** — `keep_alive` over HTTP with validated values. No
  in-process inference.
- **Verification gate** — prose never verifies. The last test/build/lint
  command decides, so a deliberately failing test followed by a passing run
  (the bug-fix policy) now counts as verified and reports `red_green`.
  Bug-fix wording is matched as whole words.
- **Session fork** — copies events up to the chosen id with completed tool
  context; the original session is intact.
- **Review UI** — per-hunk Stage / Discard / Ask agent; advanced controls in
  Settings. No Monaco. Failed diff loads clear both hunk views.
- **Secret redaction** — secret paths refused, tokens replaced with
  `[redacted secret]` at the tool-result boundary; `.env.example`-style
  templates are readable.
- **Guardian** — default off; read-only diagnostics; proposal drafts need an
  approval bound to the workspace; no push, PR or merge.

## Scope

Ubuntu 24.04+ / glibc 2.39+ on x86_64. Redaction is pattern-based and narrow.
AST call sites are syntactic. Bubblewrap isolation is optional and not a
complete OS sandbox. Formatting and Clippy gates run in CI, not on this host.
