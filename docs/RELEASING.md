# Release procedure

Build on Ubuntu 24.04 / x86_64 with Python 3.12 and Node 22. `requirements-build.txt`
records the Python build/test dependency set used for 0.19.0; `ui/package-lock.json`
locks the frontend. This is a pinned dependency build, not a claim of bit-for-bit
reproducible binaries across machines.

1. Update `pyproject.toml`, `src/shadow_agent/__init__.py`, the desktop entry,
   README, changelog, canonical-version test, and release notes.
2. Create `.venv`, install `requirements-build.txt` and the editable project.
3. Download appimagetool **1.9.1 x86_64** from its official GitHub release and verify
   SHA-256 `ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0`.
4. Run `APPIMAGETOOL=/path/to/appimagetool ./scripts/build-linux.sh`.
5. Run `npm --prefix ui test`, then `cd ui && npx playwright install chromium &&
   npm run test:e2e`. Inspect light/dark/compact and completed-task screenshots.
6. Run `.venv/bin/python -m build` to produce the wheel and source archive.
7. Smoke the standalone and AppImage executables with `--version`, the API, an
   offline task, and a real local-model task when available. Test a clean wheel
   installation. Verify the installer preserves existing XDG data.
8. Recompute `dist/SHA256SUMS` for the AppImage, portable archive, wheel, and source
   archive. Commit, tag `vVERSION`, and push. The tag workflow repeats the checks
   on Ubuntu 24.04 and uploads the assets. Inspect both workflows before announcing.

The source installer uses a venv and builds from the npm lockfile. The AppImage
installer validates the executable before replacing the prior stable link and
launcher. Do not replace a running API midway through a task. AppImage extraction
must remain alive while serving assets; `desktop.py` deliberately keeps the
standalone process serving rather than exiting after spawning a detached worker.

Browser test artifacts are ignored by Git. Copy only intentional, sanitized
screenshots into `docs/images/`. Never include databases, secrets, model caches,
user browser profiles, or machine-specific paths in release artifacts.

The GitHub workflows pin their actions by commit. Keep action pins, build locks,
and the appimagetool checksum current as part of maintenance. Release assets are
checksummed but are not independently signed.
