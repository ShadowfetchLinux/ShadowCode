#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN="${HOME}/.local/bin"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/shadow-agent"

# Drop a stale pip/site-packages copy so Python cannot import the old 0.1 package.
# Do this before writing ~/.local/bin/shadow — pip uninstall also removes that entry point.
python3 -m pip uninstall -y shadow-agent >/dev/null 2>&1 || \
  python3 -m pip uninstall -y shadow-agent --break-system-packages >/dev/null 2>&1 || true

mkdir -p "$BIN"
# Replace any previous wrapper (do not delete user config/sessions/memories).
rm -f "$BIN/shadow"
cat >"$BIN/shadow" <<EOF
#!/usr/bin/env bash
set -euo pipefail
SECRETS="\${XDG_CONFIG_HOME:-\$HOME/.config}/shadow-agent/secrets.env"
if [[ -f "\$SECRETS" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "\$SECRETS"
  set +a
fi
export PYTHONPATH="${ROOT}/src\${PYTHONPATH:+:\$PYTHONPATH}"
exec python3 -m shadow_agent "\$@"
EOF
chmod +x "$BIN/shadow"

mkdir -p "$CONFIG"
# Build the desktop UI so one-click launch serves this upgrade.
(cd "$ROOT/ui" && npm install --no-fund --no-audit && npm run build)

ICON_SRC="$ROOT/assets/icons/shadow-agent.svg"
cp "$ICON_SRC" "$ROOT/assets/shadow-agent.svg"
mkdir -p "$ROOT/assets/icons/hicolor/scalable/apps"
cp "$ICON_SRC" "$ROOT/assets/icons/hicolor/scalable/apps/shadow-agent.svg"
cp "$ICON_SRC" "$ROOT/ui/public/icon.svg"
for size in 16 22 24 32 48 64 128 256 512; do
  dest="$PREFIX/icons/hicolor/${size}x${size}/apps"
  proj="$ROOT/assets/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$dest" "$proj"
  if command -v rsvg-convert >/dev/null 2>&1; then
    rsvg-convert -w "$size" -h "$size" "$ICON_SRC" -o "$dest/shadow-agent.png"
    cp "$dest/shadow-agent.png" "$proj/shadow-agent.png"
  fi
done
mkdir -p "$PREFIX/icons/hicolor/scalable/apps"
cp "$ICON_SRC" "$PREFIX/icons/hicolor/scalable/apps/shadow-agent.svg"

mkdir -p "$PREFIX/applications"
sed "s|^Exec=.*|Exec=${BIN}/shadow ui|; s|^TryExec=.*|TryExec=${BIN}/shadow|" \
  "$ROOT/packaging/shadow-agent.desktop" > "$PREFIX/applications/shadow-agent.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$PREFIX/applications" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q "$PREFIX/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "Installed shadow → ${BIN}/shadow"
echo "Desktop entry → ${PREFIX}/applications/shadow-agent.desktop (Icon=shadow-agent)"
