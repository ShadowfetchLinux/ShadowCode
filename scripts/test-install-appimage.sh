#!/usr/bin/env bash
# Exercises scripts/install-appimage.sh with fake AppImages in an isolated HOME:
# checksum refusal, runtime installed from inside the AppImage with relative
# links, refusal of broken runtimes, rollback, and preserved profile data.
# Never touches the real HOME.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
INSTALLER="$ROOT/scripts/install-appimage.sh"
SCRATCH="$(mktemp -d)"
cleanup() { chmod -R u+w -- "$SCRATCH" 2>/dev/null || true; rm -rf -- "$SCRATCH"; }
trap cleanup EXIT
HOME="$SCRATCH/home"
export HOME
export XDG_DATA_HOME="$HOME/.local/share"
unset SHADOWCODE_SHA256SUMS SHADOWCODE_GIT_SHA
COMMIT_A="$(printf 'a1%.0s' {1..20})"
COMMIT_B="$(printf 'b2%.0s' {1..20})"
LIB="$HOME/.local/lib/shadowcode"
mkdir -p "$HOME/Applications" "$SCRATCH/release" "$XDG_DATA_HOME/shadow-agent" "$LIB"
printf 'keep this profile data' > "$XDG_DATA_HOME/shadow-agent/profile.txt"
printf 'old application' > "$HOME/Applications/ShadowCode-0.27.0-x86_64.AppImage"
ln -s ShadowCode-0.27.0-x86_64.AppImage "$HOME/Applications/ShadowCode.AppImage"
printf 'old runtime' > "$LIB/old-runtime-marker"

# fake_appimage FILE VERSION COMMIT MODE
# MODE: good | absolute-link | dangling-link | broken-server | wrong-commit | no-runtime
fake_appimage() {
  local file="$1" version="$2" commit="$3" mode="$4"
  cat > "$file" <<FAKE
#!/usr/bin/env bash
set -euo pipefail
if [[ "\${1:-}" == "--appimage-extract-and-run" && "\${2:-}" == "--version" ]]; then
  printf 'ShadowCode $version\\n'
  exit 0
fi
if [[ "\${1:-}" == "--appimage-extract" && "\${2:-}" == "usr/lib/shadowcode" ]]; then
  [[ "$mode" == "no-runtime" ]] && exit 0
  dir=squashfs-root/usr/lib/shadowcode
  mkdir -p "\$dir/NOTICES"
  if [[ "$mode" == "real" ]]; then
    cp -a "$ROOT/packaging/llama.cpp/bin/." "\$dir/"
    exit 0
  fi
  reported="${commit:0:9}"
  [[ "$mode" == "wrong-commit" ]] && reported="0000000ff"
  exit_code=0
  [[ "$mode" == "broken-server" ]] && exit_code=127
  printf '#!/bin/sh\\nif [ -n "\${LD_LIBRARY_PATH:-}" ]; then echo "LD_LIBRARY_PATH leaked" >&2; exit 3; fi\\necho "version: 0.4.1-dev (build 1, commit %s)" >&2\\nexit %s\\n' "\$reported" "\$exit_code" > "\$dir/llama-server"
  chmod 755 "\$dir/llama-server"
  printf 'lib' > "\$dir/libfake.so.0.1"
  ln -s libfake.so.0.1 "\$dir/libfake.so.0"
  [[ "$mode" == "absolute-link" ]] && ln -sfn "\$PWD/\$dir/libfake.so.0.1" "\$dir/libfake.so.0"
  [[ "$mode" == "dangling-link" ]] && ln -sfn libmissing.so.0 "\$dir/libfake.so.0"
  printf 'url=https://github.com/ggml-org/llama.cpp.git\\ncommit=$commit\\nbackend=vulkan+cpu\\n' > "\$dir/COMMIT"
  printf 'llama\\nqwen3\\n' > "\$dir/architectures.txt"
  printf 'MIT License\\n' > "\$dir/NOTICES/llama.cpp-LICENSE"
  exit 0
fi
exit 1
FAKE
  chmod +x "$file"
}
checksum() { (cd "$(dirname "$1")" && sha256sum "$(basename "$1")" >> SHA256SUMS); }
expect_refusal() {
  local label="$1"; shift
  if "$@" > "$SCRATCH/refused.txt" 2>&1; then
    echo "Installer accepted: $label" >&2
    cat "$SCRATCH/refused.txt" >&2
    exit 1
  fi
}
assert_state_unchanged() {
  test "$(readlink "$HOME/Applications/ShadowCode.AppImage")" = "$1"
  grep -Fxq "commit=$2" "$LIB/COMMIT"
  test ! -e "$LIB.previous"
  test "$(cat "$XDG_DATA_HOME/shadow-agent/profile.txt")" = 'keep this profile data'
  test -z "$(find "$HOME/.local/lib" -maxdepth 1 -name '.shadowcode-install.*' -print -quit)"
}

APPIMAGE="$SCRATCH/release/ShadowCode_0.28.0_amd64.AppImage"
fake_appimage "$APPIMAGE" 0.28.0 "$COMMIT_A" good

# 1. No SHA256SUMS: refused unless --unverified; nothing changes.
expect_refusal 'missing SHA256SUMS' "$INSTALLER" "$APPIMAGE"
grep -Fq 'pass --unverified' "$SCRATCH/refused.txt"
test "$(readlink "$HOME/Applications/ShadowCode.AppImage")" = ShadowCode-0.27.0-x86_64.AppImage
test -f "$LIB/old-runtime-marker"

# 2. Verified install: AppImage, runtime from inside it, launchers, entry.
checksum "$APPIMAGE"
"$INSTALLER" "$APPIMAGE" > "$SCRATCH/installed.txt"
grep -Fq 'Verified SHA-256' "$SCRATCH/installed.txt"
test -f "$HOME/Applications/ShadowCode-0.28.0-x86_64.AppImage"
test "$(readlink "$HOME/Applications/ShadowCode.AppImage")" = ShadowCode-0.28.0-x86_64.AppImage
test ! -e "$HOME/Applications/ShadowCode-0.27.0-x86_64.AppImage"
test ! -e "$LIB/old-runtime-marker"
grep -Fxq "commit=$COMMIT_A" "$LIB/COMMIT"
test "$(readlink "$LIB/libfake.so.0")" = libfake.so.0.1
test -s "$LIB/NOTICES/llama.cpp-LICENSE"
test -s "$LIB/architectures.txt"
env -u LD_LIBRARY_PATH "$LIB/llama-server" --version 2>&1 | grep -Fq "commit ${COMMIT_A:0:9}"
test ! -e "$LIB.previous"
test -z "$(find "$HOME/.local/lib" -maxdepth 1 -name '.shadowcode-install.*' -print -quit)"
test "$(cat "$XDG_DATA_HOME/shadow-agent/profile.txt")" = 'keep this profile data'
"$HOME/.local/bin/shadow" --version | grep -Fx 'ShadowCode 0.28.0'
test "$(readlink "$HOME/.local/bin/shadowcode")" = shadow
"$HOME/.local/bin/shadowcode" --version | grep -Fx 'ShadowCode 0.28.0'
DESKTOP="$XDG_DATA_HOME/applications/shadow-agent.desktop"
grep -Fxq 'X-ShadowCode-Version=0.28.0' "$DESKTOP"
grep -Fxq 'StartupWMClass=shadowcode' "$DESKTOP"
grep -Fxq "Exec=\"$HOME/.local/bin/shadow\" ui" "$DESKTOP"
test -f "$XDG_DATA_HOME/icons/hicolor/scalable/apps/shadow-agent.svg"
# The source commit is recorded once, from the checkout or an explicit override.
test "$(grep -c '^X-ShadowCode-GitSha=' "$DESKTOP")" -le 1
if EXPECTED_SHA="$(git -C "$ROOT" rev-parse --verify HEAD 2>/dev/null)"; then
  grep -Fxq "X-ShadowCode-GitSha=$EXPECTED_SHA" "$DESKTOP"
fi
SHADOWCODE_GIT_SHA="$(printf 'a%.0s' {1..40})" "$INSTALLER" "$APPIMAGE" > /dev/null
grep -Fxq "X-ShadowCode-GitSha=$(printf 'a%.0s' {1..40})" "$DESKTOP"
test "$(grep -c '^X-ShadowCode-GitSha=' "$DESKTOP")" -eq 1
assert_state_unchanged ShadowCode-0.28.0-x86_64.AppImage "$COMMIT_A"

# 3. Broken runtimes inside a correctly checksummed AppImage are refused
#    before anything is replaced.
for mode in absolute-link dangling-link broken-server wrong-commit no-runtime; do
  BAD="$SCRATCH/release/ShadowCode_0.28.1_amd64.AppImage"
  rm -f "$BAD" "$SCRATCH/release/SHA256SUMS"
  fake_appimage "$BAD" 0.28.1 "$COMMIT_B" "$mode"
  checksum "$BAD"
  expect_refusal "runtime mode $mode" "$INSTALLER" "$BAD"
  case "$mode" in
    absolute-link) reason='contains absolute symlinks' ;;
    dangling-link) reason='contains dangling symlinks' ;;
    broken-server) reason='llama-server did not start' ;;
    wrong-commit) reason="does not report commit ${COMMIT_B:0:7}" ;;
    no-runtime) reason='does not contain the managed llama.cpp runtime' ;;
  esac
  grep -Fq "$reason" "$SCRATCH/refused.txt" || { cat "$SCRATCH/refused.txt" >&2; exit 1; }
  test ! -e "$HOME/Applications/ShadowCode-0.28.1-x86_64.AppImage"
  assert_state_unchanged ShadowCode-0.28.0-x86_64.AppImage "$COMMIT_A"
done

# 4. --unverified installs without SHA256SUMS (explicit opt-in).
UNVERIFIED="$SCRATCH/unverified/ShadowCode_0.28.1_amd64.AppImage"
mkdir -p "$(dirname "$UNVERIFIED")"
fake_appimage "$UNVERIFIED" 0.28.1 "$COMMIT_B" good
"$INSTALLER" --unverified "$UNVERIFIED" > /dev/null 2> "$SCRATCH/unverified.txt"
grep -Fq 'installing unverified' "$SCRATCH/unverified.txt"
assert_state_unchanged ShadowCode-0.28.1-x86_64.AppImage "$COMMIT_B"
test ! -e "$HOME/Applications/ShadowCode-0.28.0-x86_64.AppImage"

# 5. A failure after the swap restores the previous runtime and AppImage link.
NEXT="$SCRATCH/release/ShadowCode_0.28.2_amd64.AppImage"
rm -f "$SCRATCH/release/SHA256SUMS"
fake_appimage "$NEXT" 0.28.2 "$COMMIT_A" good
checksum "$NEXT"
chmod 555 "$XDG_DATA_HOME/applications"
expect_refusal 'unwritable desktop entry directory' "$INSTALLER" "$NEXT"
chmod 755 "$XDG_DATA_HOME/applications"
grep -Fq 'previous runtime and AppImage were restored' "$SCRATCH/refused.txt"
test ! -e "$HOME/Applications/ShadowCode-0.28.2-x86_64.AppImage"
assert_state_unchanged ShadowCode-0.28.1-x86_64.AppImage "$COMMIT_B"

# 6. A changed AppImage fails its checksum.
printf '# modified after checksum\n' >> "$NEXT"
expect_refusal 'checksum mismatch' "$INSTALLER" "$NEXT"
grep -Fq 'Checksum mismatch' "$SCRATCH/refused.txt"
assert_state_unchanged ShadowCode-0.28.1-x86_64.AppImage "$COMMIT_B"

# 7. With a built runtime in this checkout, the real llama-server installs and
#    starts from the isolated ~/.local/lib/shadowcode (no GPU work: --version).
REAL_BIN="$ROOT/packaging/llama.cpp/bin"
if [[ -x "$REAL_BIN/llama-server" && -f "$REAL_BIN/NOTICES/llama.cpp-LICENSE" ]]; then
  REAL_COMMIT="$(awk -F= '$1 == "commit" { print $2; exit }' "$REAL_BIN/COMMIT")"
  REAL="$SCRATCH/real/ShadowCode_0.28.3_amd64.AppImage"
  mkdir -p "$(dirname "$REAL")"
  fake_appimage "$REAL" 0.28.3 "$REAL_COMMIT" real
  checksum "$REAL"
  "$INSTALLER" "$REAL" > /dev/null
  assert_state_unchanged ShadowCode-0.28.3-x86_64.AppImage "$REAL_COMMIT"
  test -z "$(find "$LIB" -type l -lname '/*' -print -quit)"
  env -u LD_LIBRARY_PATH "$LIB/llama-server" --version 2>&1 | grep -Fq "commit ${REAL_COMMIT:0:9}"
  printf 'Real llama.cpp runtime %s installed and started from %s.\n' "${REAL_COMMIT:0:9}" "$LIB"
fi
printf 'AppImage installer checks passed.\n'
