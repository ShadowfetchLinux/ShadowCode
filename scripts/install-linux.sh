#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN="${HOME}/.local/bin"

# Editable pip install is optional (Pop!_OS blocks system-wide pip).
python3 -m pip install -e "$ROOT" --user --break-system-packages --quiet >/dev/null 2>&1 || true
mkdir -p "$BIN"
cat >"$BIN/shadow" <<EOF
#!/usr/bin/env bash
export PYTHONPATH="${ROOT}/src\${PYTHONPATH:+:\$PYTHONPATH}"
exec python3 -m shadow_agent "\$@"
EOF
chmod +x "$BIN/shadow"

if [[ ! -d "$ROOT/ui/dist" ]]; then
  (cd "$ROOT/ui" && npm install --no-fund --no-audit && npm run build)
fi

ICON_SRC="$ROOT/assets/shadow-agent.svg"
for size in 16 22 24 32 48 64 96 128 256 512; do
  dest="$PREFIX/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$dest"
  if command -v rsvg-convert >/dev/null 2>&1; then
    rsvg-convert -w "$size" -h "$size" "$ICON_SRC" -o "$dest/shadow-agent.png"
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
echo "Desktop entry → ${PREFIX}/applications/shadow-agent.desktop (not pinned)"
