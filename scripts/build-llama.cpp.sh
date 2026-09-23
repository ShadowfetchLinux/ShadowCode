#!/usr/bin/env bash
# Build a pinned llama.cpp into ShadowCode's managed runtime.
# No sudo. No curl|bash. Does not download model weights.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${SHADOWCODE_LLAMA_SRC:-$ROOT/tools/llama.cpp}"
PIN="$ROOT/tools/llama.cpp.pin"
OUT_REPO="$ROOT/packaging/llama.cpp"
OUT_USER="${SHADOWCODE_LLAMA_PREFIX:-$HOME/.local/lib/shadowcode}"
URL="${SHADOWCODE_LLAMA_URL:-https://github.com/ggml-org/llama.cpp.git}"
# Empty means: clone/fetch and record whatever commit we built.
PINNED="${SHADOWCODE_LLAMA_COMMIT:-}"
if [[ -z "$PINNED" && -f "$PIN" ]]; then
  PINNED="$(awk -F= '/^commit=/{print $2; exit}' "$PIN")"
fi
CMAKE="${SHADOWCODE_CMAKE:-}"
if [[ -z "$CMAKE" ]]; then
  if [[ -x "$ROOT/.venv/bin/cmake" ]]; then
    CMAKE="$ROOT/.venv/bin/cmake"
  elif command -v cmake >/dev/null 2>&1; then
    CMAKE="$(command -v cmake)"
  else
    echo "cmake is required. Install it into the repo venv: .venv/bin/pip install cmake" >&2
    exit 1
  fi
fi
GENERATOR=()
if command -v ninja >/dev/null 2>&1; then
  GENERATOR=(-G Ninja)
fi
mkdir -p "$(dirname "$SRC")" "$OUT_REPO/bin" "$OUT_USER"
if [[ ! -d "$SRC/.git" ]]; then
  git clone --filter=blob:none "$URL" "$SRC"
fi
git -C "$SRC" remote set-url origin "$URL"
if [[ -n "$PINNED" ]]; then
  git -C "$SRC" fetch --depth 1 origin "$PINNED"
  git -C "$SRC" checkout --detach FETCH_HEAD
else
  git -C "$SRC" fetch --depth 1 origin master
  git -C "$SRC" checkout --detach FETCH_HEAD
fi
COMMIT="$(git -C "$SRC" rev-parse HEAD)"
# CPU build. CUDA is skipped: this machine has no nvcc and we will not sudo.
# Vulkan headers exist but glslc/shaderc are missing, so GGML_VULKAN stays off.
"$CMAKE" -S "$SRC" -B "$SRC/build" "${GENERATOR[@]}" \
  -DCMAKE_BUILD_TYPE=Release \
  -DGGML_NATIVE=ON \
  -DGGML_CUDA=OFF \
  -DGGML_VULKAN=OFF \
  -DLLAMA_BUILD_TESTS=OFF \
  -DLLAMA_BUILD_EXAMPLES=OFF \
  -DLLAMA_BUILD_SERVER=ON \
  -DLLAMA_CURL=OFF
"$CMAKE" --build "$SRC/build" --target llama-cli llama-server --parallel
BIN_DIR=""
for candidate in "$SRC/build/bin" "$SRC/build"; do
  if [[ -x "$candidate/llama-server" && -x "$candidate/llama-cli" ]]; then
    BIN_DIR="$candidate"
    break
  fi
done
[[ -n "$BIN_DIR" ]] || { echo "llama.cpp build did not produce llama-cli/llama-server" >&2; exit 1; }
install_runtime() {
  local dest="$1"
  mkdir -p "$dest"
  install -m 755 "$BIN_DIR/llama-cli" "$dest/llama-cli"
  install -m 755 "$BIN_DIR/llama-server" "$dest/llama-server"
  find "$BIN_DIR" -maxdepth 1 \( -name 'libllama*.so*' -o -name 'libggml*.so*' -o -name 'libmtmd.so*' \) -exec cp -a {} "$dest/" \;
  if command -v patchelf >/dev/null 2>&1; then
    for bin in llama-cli llama-server; do
      patchelf --set-rpath '$ORIGIN' "$dest/$bin"
    done
    for lib in "$dest"/lib*.so*; do
      [[ -f "$lib" && ! -L "$lib" ]] || continue
      patchelf --set-rpath '$ORIGIN' "$lib"
    done
  fi
}
install_runtime "$OUT_REPO/bin"
install_runtime "$OUT_USER"
{
  printf 'url=%s\n' "$URL"
  printf 'commit=%s\n' "$COMMIT"
  printf 'backend=cpu\n'
  printf 'built=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$PIN"
cp "$PIN" "$OUT_REPO/COMMIT"
cp "$PIN" "$OUT_USER/COMMIT"
"$OUT_USER/llama-cli" --version
"$OUT_USER/llama-server" --version
printf 'Installed managed llama.cpp %s -> %s\n' "$COMMIT" "$OUT_USER"
