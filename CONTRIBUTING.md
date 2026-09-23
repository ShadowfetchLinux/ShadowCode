# Contributing to ShadowCode

ShadowCode is a Rust engine (`native/core`), a Tauri desktop shell
(`src-tauri`) and a React interface (`ui/`) embedded in one executable. Read
[ARCHITECTURE.md](ARCHITECTURE.md) and the UI contract in
[docs/API_CONTRACT_0.28.md](docs/API_CONTRACT_0.28.md) before changing a route
or an event.

## Setup

You need Rust 1.95, Node.js 22.12 or newer, and the system packages listed in
the [README](README.md#build-from-source). Local-model work also needs the
managed llama.cpp runtime (`scripts/build-llama.cpp.sh`).

```bash
npm --prefix ui ci
npm --prefix ui run build
cargo build -p shadowcode-desktop --locked
./target/debug/shadowcode --profile /tmp/shadowcode-dev --workspace /path/to/project
```

Always use `--profile` during development, so your real settings and history
stay untouched.

## Checks

```bash
cargo +1.95.0 fmt --all --check
cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.95.0 build -p shadowcode-desktop --locked
cargo +1.95.0 test --workspace --locked
npm --prefix ui run typecheck
npm --prefix ui test
(cd ui && npx playwright install chromium && npm run test:e2e)
npm --prefix ui run format
```

Where tests go:

- **Engine behaviour:** `native/core/tests`. Vendor protocols are tested with
  scripted frames and fake CLIs (`tests/vendor_support`), and the local engine
  with a fake `llama-server`. Tests must not need a real account, a GPU or a
  network.
- **Event reducers and components:** Vitest.
- **Window flows:** Playwright (`ui/e2e`) against the fake engine in
  `ui/e2e/fakeBackend.ts`.
- **Packaging and the installer:** `scripts/` (`test-llama-runtime.mjs`,
  `test-install-appimage.sh`).

Live checks against real vendor CLIs or a GPU are useful, but report them
separately from the deterministic suite. Mark tests that need them `#[ignore]`.

## Rules

- **Facts come from the vendor.** Show only what a vendor reports through its
  documented interface. Never invent usage, models or capabilities, and never
  read vendor credential files.
- **Protocols stay in adapters.** Keep provider protocols inside
  `native/core/src/cli_agent/`, and let the rest of the app use the runtime
  facade (`runtime.rs`).
- **Use throwaway data in tests.** Tests use temporary workspaces and profiles.
  Never commit API keys, `secrets.env`, databases or machine-specific home
  paths.
- **Keep the `shadow-agent` directories.** Don't break the XDG profile
  directories or existing configs: add migrations instead.

## Reporting

For bugs, include:

- your OS and the ShadowCode version,
- the relevant, redacted output of `shadowcode doctor`,
- the steps to reproduce the problem.

Report vulnerabilities privately, as described in [SECURITY.md](SECURITY.md).
Releases follow [docs/RELEASING.md](docs/RELEASING.md).
