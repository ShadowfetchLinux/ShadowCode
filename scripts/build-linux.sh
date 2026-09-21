#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Do not inherit /usr/local/bin/node → Hermes; linuxdeploy/appimagetool scan PATH.
# shellcheck source=native-packaging-env.sh
source "$ROOT/scripts/native-packaging-env.sh"
shadowcode_apply_packaging_path "$ROOT"
cd "$ROOT"
PYTHON="${SHADOW_BUILD_PYTHON:-$ROOT/.venv/bin/python}"
VERSION="$("$PYTHON" -c 'from shadow_agent import __version__; print(__version__)')"
npm --prefix ui ci --no-fund --no-audit
npm --prefix ui run build
"$PYTHON" -m pytest
"$PYTHON" -m PyInstaller --noconfirm --clean packaging/shadowcode.spec
APPDIR="$ROOT/build/ShadowCode.AppDir"
mkdir -p "$APPDIR/usr/bin"
cp -a dist/ShadowCode/. "$APPDIR/usr/bin/"
cp assets/icons/shadow-agent.svg "$APPDIR/shadow-agent.svg"
cp packaging/shadow-agent.desktop "$APPDIR/shadow-agent.desktop"
sed -i 's/^Exec=.*/Exec=shadowcode ui/; s/^TryExec=.*/TryExec=shadowcode/' "$APPDIR/shadow-agent.desktop"
cat > "$APPDIR/AppRun" <<'LAUNCH'
#!/usr/bin/env bash
set -euo pipefail
APP_ROOT="$(cd "$(dirname "$0")" && pwd)"
exec "$APP_ROOT/usr/bin/shadowcode" "$@"
LAUNCH
chmod +x "$APPDIR/AppRun"
if [[ -z "${APPIMAGETOOL:-}" ]]; then
  echo "Set APPIMAGETOOL to a local appimagetool executable to create the AppImage." >&2
  exit 1
fi
ARCH=x86_64 "$APPIMAGETOOL" --appimage-extract-and-run "$APPDIR" "$ROOT/dist/ShadowCode-${VERSION}-x86_64.AppImage"
tar -C dist -czf "dist/ShadowCode-${VERSION}-linux-x86_64.tar.gz" ShadowCode
(cd dist && sha256sum "ShadowCode-${VERSION}-x86_64.AppImage" "ShadowCode-${VERSION}-linux-x86_64.tar.gz" > SHA256SUMS)
