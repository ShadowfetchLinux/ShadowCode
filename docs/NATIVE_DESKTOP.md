# Native desktop development — 0.20

The `native-0.20` branch builds one Rust desktop executable with the React
interface embedded. Tauri hosts it in the system WebKit webview. The interface
calls the Rust engine through IPC; desktop operation needs no HTTP listener,
Python interpreter, Node runtime, or browser launcher.

This is a development build. The [full migration gates](NATIVE_MIGRATION.md)
still apply, including orchestration/integration parity, packaging, broader UI
stress tests, and installation. Keep the supported 0.19 installation until the
native release is verified.

## Build and run

On Ubuntu 24.04, install the native build dependencies:

```sh
sudo apt-get install build-essential pkg-config libgtk-3-dev \
  libwebkit2gtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev patchelf
```

Use Rust 1.95 and Node 22 or newer for development. From the repository root:

```sh
npm --prefix ui ci
npm --prefix ui run build
cargo build -p shadowcode-desktop --locked
./target/debug/shadowcode --version
./target/debug/shadowcode --profile /tmp/shadowcode-dev --workspace /path/to/project
```

`--profile` keeps config, secrets, SQLite history, and webview storage separate
from the installed application. Without it, the application uses the existing
`shadow-agent` XDG directories and backs up SQLite before migrating. Do not run
the legacy Python application against the same profile during migration.

Choose an installed local model or a compatible endpoint in onboarding. The
offline preview lets you inspect the workspace but cannot execute coding tasks.
Model inference remains in Ollama or the selected provider; it is not bundled.

The [native CLI](NATIVE_CLI.md) uses this same executable. Commands such as
`run`, `sessions`, and `health` run without a display and share an open desktop's
engine without changing its selected project. `serve` explicitly hosts the
engine headlessly for detached tasks and background servers.

To work on the live interface, run `./ui/node_modules/.bin/tauri dev` from the
repository root. The Tauri configuration starts Vite and builds the native app.
Build hooks explicitly use the `ui` directory, including when the CLI is invoked
from the repository root.

## Development packages

From the repository root, build both package formats with:

```sh
node scripts/build-native.mjs
node scripts/check-native-package.mjs \
  target/release/bundle/appimage/ShadowCode_0.20.0_amd64.AppImage \
  target/release/bundle/deb/ShadowCode_0.20.0_amd64.deb
```

The build script collects [dependency notices](../licenses/native/README.md),
restores the original executable before packaging each format because Tauri
modifies its bundle-type marker, and repacks the AppImage with notices for its
actual bundled system libraries. Build tools are cached in `target/.tauri/`.
Unknown dependencies or a changed AppImage runtime stop packaging until their
notices are supplied. The checker verifies FUSE-free
startup, the legacy `shadow ui` launch form, native ELF code, package versions,
dependency resolution on the build host, and absence of Python interpreters,
libraries, and sidecars. It also verifies the SHA-256 digest of every listed
notice in each extracted package. It produces checksums in `artifacts/native-package/`.

The first verified CI packages contain an approximately 17.5 MB executable, an
83 MB AppImage (including native GTK/WebKit libraries), and an 8.2 MB Debian
package (using system GTK/WebKit). CI builds, inspects, and exercises the actual
AppImage on Ubuntu 24.04, with development downloads retained for 14 days.
The downloaded packages also passed inspection and the AppImage window workflow
on the development machine. These remain development artifacts.
Corresponding-source release artifacts, complete feature migration, and final
release verification/installation are still required before publication as the
supported download.
Concurrent extraction-mode launches also need isolated temporary directories:
the pinned AppImage runtime otherwise shares a directory and can remove files
still needed by another instance. That lifetime issue remains a release blocker;
use the unbundled executable or a distinct `TMPDIR` per development invocation.

If linuxdeploy aborts while scanning an inaccessible symlink in a PATH directory,
remove that directory from PATH for the packaging command; the application does
not require that tool. No global PATH change is needed.

## Desktop integration

- Native folder selection and conversation export use OS dialogs. External
  HTTP/HTTPS links open through the OS; remote pages cannot replace the app view.
- Default-profile launches focus the existing instance. Isolated profiles have
  separate webview storage and still enforce their own profile locks.
- Window geometry is retained for the default profile. Completion notifications
  follow the saved notification setting when the window is not focused.
- Close requests and termination signals cancel agent and manual terminal work,
  wait for cleanup, and retain a visible error if shutdown needs attention.
- Live events are wakeups for ordered SQLite replay. A periodic read recovers
  missed notifications; streamed messages are replaced by their final text.
- [Goals](NATIVE_GOALS.md) run persisted milestones through the same task engine,
  with visible results, command verification, pause/resume, and safe recovery.
- [Model routing](NATIVE_ROUTING.md) selects a registered model for Plan, Build,
  Review, and Test. Provider-scoped IDs keep identical model names on different
  servers distinct; conversations retain the actual selection and fallback notices.
- [Commands and project skills](NATIVE_WORKFLOWS.md) run selected workflows through
  the native task engine and retain command results and source provenance.
- [Background processes](NATIVE_BACKGROUND.md) run project servers and watchers
  alongside coding tasks, with live bounded logs, retained history, stop controls,
  and process-group cleanup during application shutdown.
- A private Unix socket connects native CLI clients to the desktop's engine.
  Each client keeps independent navigation state; managed shutdown closes the
  connection and cancels its in-flight manual commands before engine cleanup.

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
npm --prefix ui test
node scripts/test-native-cli.mjs
```

The actual window test uses Tauri's WebDriver bridge and a scripted compatible
model, with a disposable profile and project. Install `webkit2gtk-driver`, `xvfb`,
and `tauri-driver` 2.0.6, then run:

```sh
cargo install tauri-driver --version 2.0.6 --locked
xvfb-run -a -s '-screen 0 1440x1100x24' dbus-run-session -- \
  node scripts/test-native-desktop.mjs
```

Optional environment variables `SHADOW_DESKTOP_BINARY`, `SHADOW_TAURI_DRIVER`,
and `SHADOW_WEBKIT_DRIVER` select explicit executable paths. Logs, screenshots,
and a machine-readable result are written to `artifacts/native/`. This test
checks the real embedded window, Rust IPC, approval, file and terminal tools,
reload, cancellation, compact layout, model routing/fallback notices, goals and
their live transcript, pause, managed background processes,
CLI coexistence and project isolation,
light/dark/compact/goals/routing/background accessibility,
and managed shutdown. It also verifies
that the executable embeds the current compiled interface and does not load
`libpython`.

To run this same workflow against the actual AppImage, set
`SHADOW_DESKTOP_BINARY` to its absolute path,
`SHADOW_DESKTOP_ARGS='["--appimage-extract-and-run","ui"]'`, and
`SHADOW_NATIVE_ARTIFACTS` to a separate output directory. Its test profile and
workspace remain disposable, so it does not migrate the installed app's data.

See [verification evidence](NATIVE_VERIFICATION.md) for engine stress tests and
separate real-model coding probes. The scripted window test establishes UI and
engine integration; the real-model probes establish provider/tool behavior.

To exercise an installed Ollama model through the actual native window, run:

```sh
xvfb-run -a -s '-screen 0 1440x1100x24' dbus-run-session -- \
  node scripts/probe-native-model.mjs gpt-oss:20b
# Repeat with another installed model:
xvfb-run -a -s '-screen 0 1440x1100x24' dbus-run-session -- \
  node scripts/probe-native-model.mjs qwen3:14b
```

This manual probe defaults to `target/release/shadowcode` and accepts the same
binary, argument, driver, and artifact environment variables as the scripted
window test. It uses a temporary Rust project and isolated profile, approves
only `cargo test --offline --lib` in that project, checks the minimal edit and
unchanged tests independently, reloads the conversation, submits a read-only
follow-up, clicks Stop during actual model streaming, restores the checkpoint,
and verifies native-process shutdown. Results and screenshots are saved under
`artifacts/native-model/<model>/`. Ollama must already have the selected model;
the probe does not download models or change the installed app's profile.
