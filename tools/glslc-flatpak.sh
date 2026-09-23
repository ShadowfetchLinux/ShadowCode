#!/usr/bin/env bash
# glslc shim for building the managed llama.cpp Vulkan backend without root.
#
# Uses the shader compiler shipped in the user-installed org.freedesktop.Sdk
# flatpak runtime, executed directly through that runtime's own dynamic loader
# (no sandbox, no system package changes). Override with SHADOWCODE_GLSLC to
# point at any other glslc.
set -euo pipefail
if [[ -n "${SHADOWCODE_GLSLC:-}" ]]; then
  exec "$SHADOWCODE_GLSLC" "$@"
fi
if command -v glslc >/dev/null 2>&1; then
  exec glslc "$@"
fi
for base in "$HOME/.local/share/flatpak/runtime/org.freedesktop.Sdk/x86_64" \
            "/var/lib/flatpak/runtime/org.freedesktop.Sdk/x86_64"; do
  for files in "$base"/*/*/files; do
    if [[ -x "$files/bin/glslc" && -e "$files/lib64/ld-linux-x86-64.so.2" ]]; then
      exec "$files/lib64/ld-linux-x86-64.so.2" \
        --library-path "$files/lib/x86_64-linux-gnu:$files/lib" \
        "$files/bin/glslc" "$@"
    fi
  done
done
echo "glslc not found: install shaderc, or the org.freedesktop.Sdk flatpak runtime, or set SHADOWCODE_GLSLC" >&2
exit 127
