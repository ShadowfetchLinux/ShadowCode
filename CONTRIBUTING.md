# Contributing to ShadowCode

Native 0.20 work lives on `native-0.20`. Follow the
[Rust/Tauri development guide](docs/NATIVE_DESKTOP.md),
[native CLI guide](docs/NATIVE_CLI.md), and
[migration acceptance gates](docs/NATIVE_MIGRATION.md). Native behavior tests live
in `native/core/tests`; executable CLI and actual-window tests are in `scripts/`.
The Python workflow below remains for the supported 0.19 release and legacy
transport compatibility.

Use Python 3.12+ and Node.js 20.19+ or 22.12+.

```bash
python3 -m venv .venv
.venv/bin/pip install -e '.[dev]'
npm --prefix ui ci
npm --prefix ui run build
.venv/bin/python -m pytest
npm --prefix ui test
cd ui
npx playwright install chromium
npm run test:e2e
```

For frontend development, run the API with `shadow ui --no-browser`, then
`npm --prefix ui run dev`. Vite proxies `/api` to the local API. Use its proxy;
the API intentionally rejects cross-origin requests.

Keep harness logic provider-independent. Add behavioral regression tests when
changing runtime behavior. UI interactions belong in `ui/e2e`, event transformations
in Vitest, and backend invariants in pytest. Avoid tests that inspect spelling or
formatting in source files. Run type checking and build the UI before testing the
bundle or creating a release.

Tests must use throwaway workspaces and XDG directories. Do not commit API keys,
`secrets.env`, databases, browser profiles, or machine-specific home paths. The
browser test server creates its own data and never loads the user's secrets.
Optional provider integration checks may be skipped when their dependencies are
unavailable; report those separately from deterministic coverage.

Format frontend changes with `npm --prefix ui run format`. Use clear Python 3.12
code and explicit exception handling around external processes. Preserve saved
user data and compatibility with the `shadow-agent` XDG directories.

For bugs, include OS, release version, relevant redacted `shadow doctor` output,
and reproduction steps. See [SECURITY.md](SECURITY.md) for private vulnerability
reports and [docs/RELEASING.md](docs/RELEASING.md) for builds and publication.
