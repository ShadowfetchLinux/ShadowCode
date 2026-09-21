#!/usr/bin/env bash
# Exercises installer replacement and checksum refusal in an isolated home.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRATCH="$(mktemp -d)"
cleanup() { rm -rf -- "$SCRATCH"; }
trap cleanup EXIT
HOME="$SCRATCH/home"
export HOME
export XDG_DATA_HOME="$HOME/.local/share"
mkdir -p "$HOME/Applications" "$SCRATCH/release" "$XDG_DATA_HOME/shadow-agent"
printf 'keep this profile data' > "$XDG_DATA_HOME/shadow-agent/profile.txt"
printf 'old application' > "$HOME/Applications/ShadowCode-0.19.0-x86_64.AppImage"
ln -s ShadowCode-0.19.0-x86_64.AppImage "$HOME/Applications/ShadowCode.AppImage"
APPIMAGE="$SCRATCH/release/ShadowCode_0.21.0_amd64.AppImage"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'set -euo pipefail' \
  'if [[ "$1" == "--appimage-extract-and-run" && "$2" == "--version" ]]; then' \
  '  printf "ShadowCode 0.21.0\\n"' \
  '  exit 0' \
  'fi' \
  'exit 1' > "$APPIMAGE"
chmod +x "$APPIMAGE"
(cd "$SCRATCH/release" && sha256sum "$(basename "$APPIMAGE")" > SHA256SUMS)
"$ROOT/scripts/install-appimage.sh" "$APPIMAGE" > "$SCRATCH/installed.txt"
test -f "$HOME/Applications/ShadowCode-0.21.0-x86_64.AppImage"
test "$(readlink "$HOME/Applications/ShadowCode.AppImage")" = ShadowCode-0.21.0-x86_64.AppImage
test ! -e "$HOME/Applications/ShadowCode-0.19.0-x86_64.AppImage"
test "$(cat "$XDG_DATA_HOME/shadow-agent/profile.txt")" = 'keep this profile data'
"$HOME/.local/bin/shadow" --version | grep -Fx 'ShadowCode 0.21.0'
printf '# modified after checksum\n' >> "$APPIMAGE"
if "$ROOT/scripts/install-appimage.sh" "$APPIMAGE" > /dev/null 2>&1; then
  echo 'Installer accepted a changed AppImage' >&2
  exit 1
fi
test "$(readlink "$HOME/Applications/ShadowCode.AppImage")" = ShadowCode-0.21.0-x86_64.AppImage
test "$(cat "$XDG_DATA_HOME/shadow-agent/profile.txt")" = 'keep this profile data'
printf 'AppImage installer checks passed.\n'
