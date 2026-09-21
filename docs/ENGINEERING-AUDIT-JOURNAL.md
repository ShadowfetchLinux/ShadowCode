# Engineering audit journal

Independent review of the native 0.20 tree after the prior hardening pass.
Dates are 2026-09-21. No secrets or machine inventory.

## Scope

Reviewed architecture/security/native docs, CI workflows, `native/core`,
`src-tauri`, React UI, Python 0.19 leftovers, MCP, CLI/TUI, hooks, plugins,
background processes, review/packaging, and the agent harness loop
(context, planning, tools, verification, routing, retry, memory, continuation).

Prior work already covers most concurrency, recovery, confinement, MCP trust,
and desktop attach/detach cases. This pass looked for remaining confirmed
gaps rather than rewriting working systems.

## Confirmed problems

### 1. Unsafe Markdown link schemes

- **Evidence:** `Markdown.tsx` forwarded every `href` into `<a>`.
  `javascript:`, `data:`, `file:`, and relative paths were clickable. Native
  click interception plus `openExternal` already rejected non-http(s) URLs;
  the browser path did not. Keyboard/middle-click still had a live href.
- **Fix:** `safeMarkdownHref` allowlists `#` fragments and absolute
  credential-free `http`/`https` URLs. Other hrefs render as text.
- **Tests:** `ui/src/components/Markdown.test.tsx`.

### 2. Compatible tool streams that omit `index`

- **Evidence:** `StreamDecoder` used the delta array position when `index`
  was missing. Local OpenAI-compatible servers often send later argument
  fragments as a one-element `tool_calls` array with no index, so the second
  fragment started a new call and `finish()` rejected incomplete JSON.
- **Fix:** Resolve by explicit index, then matching call id, then continue
  one incomplete same-name call. A new name or a completed same-name call
  opens another slot. Ollama still treats each unindexed frame as a new call
  unless an id matches. Repeated identical `id`/`name` fields are ignored
  instead of concatenated (`call_acall_a`).
- **Tests:** `native/core/tests/model_streams.rs`
  (`compatible_stream_continues_unindexed_argument_deltas`,
  `compatible_stream_ignores_repeated_call_identity`). Existing
  indexed/interleaved and Ollama fixtures remain.

### 3. Restart recovery left no durable completion event

- **Evidence:** `Store::recover_jobs` marked jobs/tasks interrupted and
  updated summaries, but inserted no `agent.completed` row and did not set
  `event_cursor`/`result`. History pages, export, and cursor catch-up could
  omit the interruption. The desktop only surfaced the summary when it
  loaded the job record.
- **Fix:** Recovery writes the same completion event shape used for normal
  finishes, with `interrupted: true`, and stores `event_cursor` plus `result`
  on the job. A second recover still reports zero remaining active jobs.
- **Tests:** `native/core/tests/foundations.rs`
  (`job_recovery_is_durable_and_usage_is_idempotent`).

### 4. Architecture doc described only 0.19

- **Evidence:** `ARCHITECTURE.md` documented the Python loop, FastAPI, and
  SSE as the current system on the native-0.20 branch.
- **Fix:** Document the native engine as the 0.20 architecture and keep a
  short 0.19 section. `SECURITY.md` now matches the Markdown link rule.

## Commands executed

- `cargo test --offline --test model_streams --test foundations` (new cases): pass
- `cargo test --workspace --offline --lib --tests --bins`: pass (246 listed tests
  after the two stream cases; 247 after the 64-job recovery case)
- `cargo test --workspace --offline` also attempted doctests; `rustdoc` failed
  with a missing `libLLVM.so` on this toolchain. Not a product test failure.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: pass
- `rustfmt --edition 2021` on touched Rust files: applied one formatting change
- `npm --prefix ui test -- --run`: 36 passed (8 files)
- `npm --prefix ui run build`: pass
- `cargo build -p shadowcode-desktop --offline`: pass
- `.venv/bin/python -m pytest`: `test_version_is_canonical_everywhere` still
  expects README `0.19.0` on this native branch (pre-existing). After the UI
  rebuild, `test_built_bundle_assets_exist_and_are_fresh` passed. No Python
  harness behavior was changed.
- AppImage/Debian packaging and native-window WebDriver were not rerun.

## Remaining concerns

- Shell/network/root checks remain lexical word lists, not a sandbox.
  Documented in `SECURITY.md`.
- Installer warns and continues if `SHA256SUMS` is absent. Checksum mismatch
  still refuses. Changing that to mandatory would be a product decision.
- Automatic reattachment after an owner engine restart is still unimplemented.
- Live transcript memory and Markdown virtualization remain release gates.
- Worktree recovery still refuses moved paths and lost metadata.
- Python 0.19 remains in-tree for the supported release. Dual maintenance is
  accepted until native replaces it; no silent stubbing.

## Deliberately unchanged

- Engine queue/drain locking, workspace capabilities, MCP transports,
  hook/plugin install rules, profile lock, and desktop attach protocol.
  Existing tests and comments already encode the hard cases.
- Default UI layout, brand, and command names.
- Heuristic shell policy. Tightening it without evidence would break
  legitimate project commands.
- Python harness behavior on this branch, except where docs referenced it
  as if it were the native architecture.
