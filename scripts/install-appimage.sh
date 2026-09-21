#!/usr/bin/env bash
# Install a verified release; remove previous ShadowCode AppImages only after success.
set -euo pipefail
SOURCE="$(realpath "${1:?Usage: install-appimage.sh /path/to/ShadowCode-VERSION-x86_64.AppImage}")"
[[ -f "$SOURCE" ]] || { echo 'AppImage not found.' >&2; exit 1; }
SOURCE_DIR="$(dirname "$SOURCE")"
SOURCE_NAME="$(basename "$SOURCE")"
CHECKSUMS="${SHADOWCODE_SHA256SUMS:-$SOURCE_DIR/SHA256SUMS}"
if [[ -f "$CHECKSUMS" ]]; then
  EXPECTED="$(awk -v name="$SOURCE_NAME" '
    {
      file=$2
      sub(/^\*/, "", file)
      count=split(file, parts, "/")
      if (parts[count] == name) { print $1; exit }
    }
  ' "$CHECKSUMS")"
  [[ "$EXPECTED" =~ ^[[:xdigit:]]{64}$ ]] || {
    echo "No SHA-256 entry for $SOURCE_NAME in $CHECKSUMS" >&2
    exit 1
  }
  ACTUAL="$(sha256sum "$SOURCE" | awk '{print $1}')"
  [[ "$ACTUAL" == "$EXPECTED" ]] || {
    echo "Checksum mismatch for $SOURCE_NAME; refusing to install it." >&2
    exit 1
  }
  printf 'Verified SHA-256 from %s\n' "$CHECKSUMS"
else
  printf 'No SHA256SUMS file beside the AppImage; install is not checksum-verified.\n' >&2
fi
chmod +x "$SOURCE"
VERSION_LINE="$("$SOURCE" --appimage-extract-and-run --version)"
[[ "$VERSION_LINE" =~ ^ShadowCode\ ([0-9]+\.[0-9]+\.[0-9]+)$ ]] || { echo 'Not a supported ShadowCode release.' >&2; exit 1; }
VERSION="${BASH_REMATCH[1]}"
APPS="$HOME/Applications"
BIN="$HOME/.local/bin"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
DEST="$APPS/ShadowCode-${VERSION}-x86_64.AppImage"
mkdir -p "$APPS" "$BIN" "$DATA/applications" "$DATA/icons/hicolor/scalable/apps"
if [[ "$SOURCE" != "$DEST" ]]; then
  install -m 755 "$SOURCE" "$DEST.pending"
  mv -f "$DEST.pending" "$DEST"
fi
ln -sfn "$(basename "$DEST")" "$APPS/.ShadowCode.AppImage.pending"
mv -Tf "$APPS/.ShadowCode.AppImage.pending" "$APPS/ShadowCode.AppImage"
# Desktop launch uses extraction mode so libfuse2 is not required.
cat > "$BIN/.shadow-install" <<'LAUNCH'
#!/usr/bin/env bash
set -euo pipefail
exec "$HOME/Applications/ShadowCode.AppImage" --appimage-extract-and-run "$@"
LAUNCH
chmod +x "$BIN/.shadow-install"
mv -f "$BIN/.shadow-install" "$BIN/shadow"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cp "$ROOT/assets/icons/shadow-agent.svg" "$DATA/icons/hicolor/scalable/apps/shadow-agent.svg"
sed "s|^Exec=.*|Exec=\"${BIN}/shadow\" ui|; s|^TryExec=.*|TryExec=${BIN}/shadow|; s|^X-ShadowCode-Version=.*|X-ShadowCode-Version=${VERSION}|" "$ROOT/packaging/shadow-agent.desktop" > "$DATA/applications/shadow-agent.desktop"
for old in "$APPS"/ShadowCode-*-x86_64.AppImage; do
  [[ "$old" == "$DEST" || ! -f "$old" ]] || rm -- "$old"
done
command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" || true
printf 'Installed %s\nConfig and task history are preserved. Restart the app to use the new release.\n' "$DEST"
