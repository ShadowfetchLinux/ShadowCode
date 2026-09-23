#!/usr/bin/env bash
# Install a ShadowCode AppImage for the current user.
#
#   * The AppImage must match its entry in SHA256SUMS beside it (or in
#     SHADOWCODE_SHA256SUMS). Without a checksum file the install is refused
#     unless --unverified is given.
#   * Nothing is replaced until the new executable starts (--version) and the
#     llama.cpp runtime bundled inside it starts (llama-server --version with
#     LD_LIBRARY_PATH unset, printing the commit in its COMMIT file).
#   * The runtime is taken from the AppImage itself (usr/lib/shadowcode) and
#     installed into ~/.local/lib/shadowcode by rename; the previous runtime
#     stays as ~/.local/lib/shadowcode.previous until the install succeeds and
#     is restored if a later step fails.
#   * Settings and task history are never touched. Previous ShadowCode
#     AppImages are removed only after success.
set -euo pipefail
usage() {
  echo 'Usage: install-appimage.sh [--unverified] /path/to/ShadowCode_VERSION_amd64.AppImage' >&2
  exit 2
}
fail() { echo "$*" >&2; exit 1; }
UNVERIFIED=0
SOURCE_ARG=""
for arg in "$@"; do
  case "$arg" in
    --unverified) UNVERIFIED=1 ;;
    -*) usage ;;
    *) [[ -z "$SOURCE_ARG" ]] || usage; SOURCE_ARG="$arg" ;;
  esac
done
[[ -n "$SOURCE_ARG" ]] || usage
SOURCE="$(realpath -- "$SOURCE_ARG")"
[[ -f "$SOURCE" ]] || fail 'AppImage not found.'
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
  [[ "$EXPECTED" =~ ^[[:xdigit:]]{64}$ ]] || fail "No SHA-256 entry for $SOURCE_NAME in $CHECKSUMS"
  ACTUAL="$(sha256sum "$SOURCE" | awk '{print $1}')"
  [[ "$ACTUAL" == "$EXPECTED" ]] || fail "Checksum mismatch for $SOURCE_NAME; refusing to install it."
  printf 'Verified SHA-256 from %s\n' "$CHECKSUMS"
elif [[ "$UNVERIFIED" == "1" ]]; then
  printf 'No SHA256SUMS beside the AppImage; installing unverified as requested (--unverified).\n' >&2
else
  fail "No SHA256SUMS beside $SOURCE_NAME. Download SHA256SUMS from the same release, or pass --unverified to install without checking."
fi
chmod +x "$SOURCE"
VERSION_LINE="$("$SOURCE" --appimage-extract-and-run --version)" || fail 'The AppImage did not start.'
[[ "$VERSION_LINE" =~ ^ShadowCode\ ([0-9]+\.[0-9]+\.[0-9]+)$ ]] || fail 'Not a supported ShadowCode release.'
VERSION="${BASH_REMATCH[1]}"

APPS="$HOME/Applications"
BIN="$HOME/.local/bin"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
DEST="$APPS/ShadowCode-${VERSION}-x86_64.AppImage"
LIB="$HOME/.local/lib/shadowcode"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$APPS" "$BIN" "$(dirname "$LIB")" "$DATA/applications" "$DATA/icons/hicolor/scalable/apps"

# Stage next to the destination so the final step is a rename.
STAGE="$(mktemp -d "$(dirname "$LIB")/.shadowcode-install.XXXXXX")"
SWAPPED=0
PREVIOUS_LINK="$(readlink "$APPS/ShadowCode.AppImage" 2>/dev/null || true)"
finish() {
  local status=$?
  if [[ "$status" != 0 && "$SWAPPED" == 1 ]]; then
    # A later step failed: put the previous runtime and AppImage link back.
    rm -rf -- "$LIB"
    [[ -d "$LIB.previous" ]] && mv -T -- "$LIB.previous" "$LIB"
    if [[ -n "$PREVIOUS_LINK" ]]; then
      ln -sfn -- "$PREVIOUS_LINK" "$APPS/.ShadowCode.AppImage.pending"
      mv -Tf -- "$APPS/.ShadowCode.AppImage.pending" "$APPS/ShadowCode.AppImage"
    fi
    [[ "$SOURCE" == "$DEST" || "$(basename "$DEST")" == "$PREVIOUS_LINK" ]] || rm -f -- "$DEST"
    echo 'Install failed; the previous runtime and AppImage were restored.' >&2
  fi
  rm -rf -- "$STAGE"
  exit "$status"
}
trap finish EXIT

# The llama.cpp runtime shipped inside this AppImage.
(cd "$STAGE" && "$SOURCE" --appimage-extract usr/lib/shadowcode >/dev/null) \
  || fail 'Could not extract usr/lib/shadowcode from the AppImage.'
NEW_RUNTIME="$STAGE/squashfs-root/usr/lib/shadowcode"
[[ -x "$NEW_RUNTIME/llama-server" && -f "$NEW_RUNTIME/COMMIT" ]] \
  || fail 'This AppImage does not contain the managed llama.cpp runtime (usr/lib/shadowcode).'
[[ -s "$NEW_RUNTIME/architectures.txt" && -s "$NEW_RUNTIME/NOTICES/llama.cpp-LICENSE" ]] \
  || fail 'The bundled llama.cpp runtime is incomplete (architectures.txt or NOTICES missing).'
if [[ -n "$(find "$NEW_RUNTIME" -type l -lname '/*' -print -quit)" ]]; then
  fail 'The bundled llama.cpp runtime contains absolute symlinks; refusing to install it.'
fi
if [[ -n "$(find "$NEW_RUNTIME" -xtype l -print -quit)" ]]; then
  fail 'The bundled llama.cpp runtime contains dangling symlinks; refusing to install it.'
fi
RUNTIME_COMMIT="$(awk -F= '$1 == "commit" { print $2; exit }' "$NEW_RUNTIME/COMMIT")"
[[ "$RUNTIME_COMMIT" =~ ^[[:xdigit:]]{40}$ ]] || fail 'The bundled runtime COMMIT file has no commit.'
RUNTIME_VERSION="$(cd / && env -u LD_LIBRARY_PATH "$NEW_RUNTIME/llama-server" --version 2>&1)" \
  || fail "The bundled llama-server did not start: $RUNTIME_VERSION"
[[ "$RUNTIME_VERSION" == *"commit ${RUNTIME_COMMIT:0:7}"* ]] \
  || fail "The bundled llama-server does not report commit ${RUNTIME_COMMIT:0:7}."

# Everything verified: replace.
if [[ "$SOURCE" != "$DEST" ]]; then
  install -m 755 "$SOURCE" "$DEST.pending"
  mv -f "$DEST.pending" "$DEST"
fi
rm -rf -- "$LIB.previous"
[[ -e "$LIB" ]] && mv -T -- "$LIB" "$LIB.previous"
mv -T -- "$NEW_RUNTIME" "$LIB"
SWAPPED=1
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
ln -sfn shadow "$BIN/.shadowcode-install"
mv -Tf "$BIN/.shadowcode-install" "$BIN/shadowcode"
cp "$ROOT/assets/icons/shadow-agent.svg" "$DATA/icons/hicolor/scalable/apps/shadow-agent.svg"
sed "s|^Exec=.*|Exec=\"${BIN}/shadow\" ui|; s|^TryExec=.*|TryExec=${BIN}/shadow|; s|^X-ShadowCode-Version=.*|X-ShadowCode-Version=${VERSION}|; /^X-ShadowCode-GitSha=/d" "$ROOT/packaging/shadow-agent.desktop" > "$DATA/applications/shadow-agent.desktop.pending"
# Record the source commit when known: SHADOWCODE_GIT_SHA wins, then the
# checkout this script runs from. Omitted rather than guessed otherwise.
GIT_SHA="${SHADOWCODE_GIT_SHA:-$(git -C "$ROOT" rev-parse --verify HEAD 2>/dev/null || true)}"
if [[ "$GIT_SHA" =~ ^[[:xdigit:]]{40}$ ]]; then
  printf 'X-ShadowCode-GitSha=%s\n' "$GIT_SHA" >> "$DATA/applications/shadow-agent.desktop.pending"
fi
mv -f "$DATA/applications/shadow-agent.desktop.pending" "$DATA/applications/shadow-agent.desktop"
# Success: drop the previous runtime and older AppImages.
SWAPPED=0
rm -rf -- "$LIB.previous"
for old in "$APPS"/ShadowCode-*-x86_64.AppImage; do
  [[ "$old" == "$DEST" || ! -f "$old" ]] || rm -- "$old"
done
command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" || true
printf 'Installed %s\nInstalled llama.cpp %s to %s\nConfig and task history are preserved. Restart the app to use the new release.\n' \
  "$DEST" "${RUNTIME_COMMIT:0:9}" "$LIB"
