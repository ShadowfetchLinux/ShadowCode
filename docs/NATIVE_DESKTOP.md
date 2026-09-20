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

To work on the live interface, run `./ui/node_modules/.bin/tauri dev` from the
repository root. The Tauri configuration starts Vite and builds the native app.

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

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
npm --prefix ui test
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
reload, cancellation, compact layout, light/dark/compact accessibility, and
managed shutdown. It also verifies
that the executable embeds the current compiled interface and does not load
`libpython`.

See [verification evidence](NATIVE_VERIFICATION.md) for engine stress tests and
separate real-model coding probes. The scripted window test establishes UI and
engine integration; the real-model probes establish provider/tool behavior.
