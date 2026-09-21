# Native release procedure

This procedure publishes the Rust/Tauri 0.20 application. The tag workflow is
the release authority: it does not build, upload, or mention the legacy Python
wheel, source distribution, portable archive, or browser launcher.

## Prepare the release

1. Update the shared version in `Cargo.toml` and `src-tauri/tauri.conf.json`.
   They must match the tag exactly, for example `v0.20.0`.
2. Update `CHANGELOG.md`, `docs/RELEASE_NOTES.md`, README download commands and
   native migration/verification records with only evidence that is current for
   the tagged commit.
3. Run the focused local checks:

   ```sh
   npm --prefix ui ci
   npm --prefix ui run build
   npm --prefix ui test
   npm --prefix ui run test:e2e
   cargo +1.95.0 fmt --all --check
   cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings
   cargo +1.95.0 test --workspace --locked
   cargo build -p shadowcode-desktop --locked
   node scripts/test-native-cli.mjs
   node scripts/test-native-stress.mjs
   node scripts/test-native-tui.mjs
   ```

4. Verify the package path before tagging. On the pinned Ubuntu 24.04 build
   environment, run:

   ```sh
   node --test scripts/test-native-packaging-env.mjs
   node scripts/build-native.mjs
   node scripts/check-native-package.mjs \
     target/release/bundle/appimage/ShadowCode_VERSION_amd64.AppImage \
     target/release/bundle/deb/ShadowCode_VERSION_amd64.deb
   node scripts/test-native-runtime-sources.mjs
   node scripts/test-native-runtime.mjs
   bash scripts/test-install-appimage.sh
   ```

   `build-native.mjs` sanitizes PATH internally (see [desktop packaging](NATIVE_DESKTOP.md)).
   Do not require a hand-edited PATH to hide `/usr/local/bin` or Hermes.

   Package inspection verifies the absence of Python/Node sidecars, package
   notices and their digests. The installer test uses a disposable home to prove
   a checked AppImage replaces an old application, preserves profile data, and
   refuses a checksum mismatch before changing the installed target.

5. Push the reviewed commit, create and push the matching annotated tag:

   ```sh
   git tag -a vVERSION -m "ShadowCode VERSION"
   git push origin vVERSION
   ```

## GitHub release gate

`.github/workflows/release.yml` runs on the tag. It validates the native version,
builds the embedded UI, runs Rust format/clippy/tests, then exercises the native
CLI, sustained stress suite, terminal, MCP transports and real WebKit window.
It builds both packages, checks the AppImage and Debian contents, rebuilds the
bundled runtime sources without network access, verifies extraction behavior, and
runs the packaged CLI, TUI, MCP and window checks. It creates `SHA256SUMS` only
after those checks and uploads:

- `ShadowCode_VERSION_amd64.AppImage`
- `shadowcode_VERSION_amd64.deb`
- `ShadowCode_VERSION_appimage-runtime-sources.tar.gz`
- `SHA256SUMS`

Do not install or announce a tag while this job is incomplete or failed. Inspect
the exact uploaded checksums and release notes after success.

## Install verification

Download the AppImage and `SHA256SUMS` into the same directory, then verify and
install it:

```sh
sha256sum -c SHA256SUMS
./scripts/install-appimage.sh /path/to/ShadowCode_VERSION_amd64.AppImage
```

The installer performs a second matching-entry check when `SHA256SUMS` is beside
the AppImage. It starts the new executable for its version before replacing the
stable Applications link, then removes old versioned ShadowCode AppImages. It
does not delete XDG profile data. Finish active work in the previous application
before the final replacement, then reopen ShadowCode and verify the expected
sessions, settings and model connection.

The native release remains a user-level process runner rather than an operating
system sandbox. Review requested tool approvals and the task's recorded evidence.
