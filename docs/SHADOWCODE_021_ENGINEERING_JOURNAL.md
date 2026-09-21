# ShadowCode 0.21 engineering journal

Evidence-first autonomy work on `feature/shadowcode-0.21-autonomy`.
Dates are 2026-09-21. No secrets, PII, or machine inventory.

## Phase 0 — rebase (completed before this branch)

- Rebase of `audit/independent-hardening-20260921` onto `origin/main`
  `ab5e7c8fb578d969ff1f41454d7ff7f946be20c7` was required and clean
  (zero conflicts). Main’s 0.20.0 version canonicalization and `/new`
  session-switch timing were preserved.
- Audit HEAD after journal + push:
  `5107f9d8e43bd955c098aeb9c01beaa9b174b6be`
  (`git ls-remote` matched). `origin/main` stayed `ab5e7c8`. Not merged.
- Post-rebase: Rust `--lib --tests --bins` pass; UI 36 tests + production
  build pass; `shadowcode-desktop` offline build pass.
- Clippy/rustfmt **not run**: host has `rustc` 1.95.0 without those
  binaries. Apt (no sudo used): `sudo apt install rust-clippy rustfmt`
  (candidates `1.95.0~1777070268~24.04~a85c377`).
- This feature branch starts at that rebased audit commit.

## Hypotheses (starting)

H1. Existing `context::compact` already keeps the current user request and
    tool-call groups; the gap is inspectable layered budgets and a
    structured keep-list (intent/decisions/failures), not a new summarizer.
H2. Replay of shell/Git/mutations after crash is the highest remaining
    safety risk; file-tool checkpoints already exist.
H3. Event indexes `(session_id,id)` and `(task_id,id)` are sufficient
    through ~100k rows; extra indexes need measurement.
H4. Transcript virtualization is unnecessary below a few thousand items
    if reconstruct is cheap; measure before changing React.
H5. Capability-based tool hiding will confuse local models more than it
    saves tokens unless catalog cost is a large fraction of the window.
H6. Expanding the lexical shell blacklist will raise false positives
    without becoming a sandbox.

## Decisions

- Do not bump crate/UI version to 0.21.0 until a release gate. Package
  remains 0.20.0; 0.21 is the branch name and journal scope.
- Do not replace `context::compact`, `Store::recover_jobs`, stream
  decoder, Markdown href policy, attach protocol, or permission checker.
  Extend them.
- Failed experiments stay in this file.

## Measurements and results

### H1 compaction
- Confirmed: keep-list + layered `autonomy::account` is enough. Replacing
  compact with a summarizer was not done.
- Failed experiment: pinning the first user group so original intent
  could never be dropped. On a 4096-token tester route this left 4110
  estimated tokens and blocked goal verification
  (`goal_verification_uses_the_tester_route_and_executes_its_acceptance_command`).
  Reverted the pin. Intent now lives in the keep-list / bounded note.
- Failed experiment: 10k-message compact in debug. Each removal
  reserializes the remaining tape; the test did not finish in five
  minutes. Not rewritten this pass. Lab size is 80 complete groups.
- Keep-list JSON in the in-band note is truncated to 1200 bytes so small
  windows still fit. Full keep-list is on the `context.compacted` event.

### H2 replay
- Classification table implemented. Shell/Git history/MCP call =
  RequiresConfirmation. File mutations and `git_reset`/`git_clean` =
  NeverAutoReplay. Existing `repair_incomplete` already refuses blind
  mutation replay. No change to checkpoint internals.

### H3 SQLite
- 10k and 100k event inserts on a temp DB: `recent_events(20)` and
  `events_after` stayed under 2s at 100k with existing
  `(session_id,id)` indexes. No extra index added.
- 1M not run (would be ~10× the ~60–80s 100k insert in debug). Practical
  ceiling recorded as 100k in this lab.

### H4 UI transcripts
- 10_000 `model.delta` replays in Vitest finished under 4s. No
  virtualization added. Existing 128-event history pages remain the
  desktop bound.

### H5 tool catalog
- Catalog token cost is already reserved in `response_budget`. Hiding
  tools by capability was not implemented (no measured confusion
  benefit; risk of models calling missing names).

### H6 shell policy
- Corpus: `ls`/`cargo test`/`git status` stay Allow when
  `approve_shell=false`. `sudo`/`curl`/`rm` are not silently Allow.
  Ambiguous cases (`echo curl`, `python -c ... rm`, `./scripts/npm`)
  show lexical FP/FN. Blacklist not expanded. `sandbox: false`.

### Other phase outcomes
- P7: Desktop reconnect/catch-up already exists (`useConversation`,
  event cursors). Automatic reattach after an *owner engine process*
  restart remains unimplemented (same as the 0.20 audit remaining
  concern). Not faked.
- P8: Malformed JSON is an error, not invented text. Unindexed argument
  deltas still join via the audit decoder.
- P11: warn at 3, replan at 4 (calls not executed), pause at 5 (same
  bail string as before).
- P12: named profiles never raise configured caps; last allowed step
  is not treated as token exhaustion (that bug broke MCP `max_steps=1`).
- P13: `verification.summary` now includes claim/observed/verified.
  Model prose alone is never `verified`.
- P17: no allocator/FD leak hunt this pass (no baseline tooling on host).
- P20: `bwrap` probed as optional; Docker not required; no runtime
  containment enabled.
- P22: statusline `aria-live="polite"`. Full keyboard/contrast audit
  not completed.
- P25–P27: not run (long chaos mix, disposable real repo, self-host
  worktree).
- P29: Clippy/rustfmt/Python/AppImage/deb/install smoke not run.

### Commands (0.21 branch)
- `cargo test --workspace --offline --lib --tests --bins`: pass
  (includes audit stream/recovery cases + 10 new `autonomy_021` tests).
- `npm --prefix ui test -- --run`: 38 passed (8 files).
- `npm --prefix ui run build`: pass.
- `cargo build -p shadowcode-desktop --offline`: pass.
- Clippy/rustfmt: **not run** (same apt gap as Phase 0).

## Remaining risks
- Compact is still O(n) removals with full JSON estimate each time.
- Engine-process restart auto-reattach is still missing.
- Shell policy remains a word list.
- 1M-event and leak-hunt baselines are not collected.
- Doctor/local stats are local only; no product telemetry.

## Rejected changes
- First-user pin during compact (broke 4096-token goal verification).
- 10k-message compact rewrite.
- Vector retrieval.
- Capability-based tool hiding.
- Expanding the shell blacklist.
- Version bump to 0.21.0.
- Merge to main.
