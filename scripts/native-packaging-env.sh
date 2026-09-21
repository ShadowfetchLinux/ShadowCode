# Shared packaging PATH for shell packagers. Keep in lockstep with
# scripts/native-packaging-env.mjs: never inherit /usr/local/bin or Hermes.
# shellcheck shell=bash
shadowcode_apply_packaging_path() {
  local root="$1"
  local helper="$root/scripts/native-packaging-env.mjs"
  if [[ ! -f "$helper" ]]; then
    echo "missing $helper" >&2
    return 1
  fi
  local node
  node="$(command -v node 2>/dev/null || true)"
  if [[ -z "$node" && -x /usr/bin/node ]]; then
    node=/usr/bin/node
  fi
  if [[ -z "$node" ]]; then
    echo "node is required to sanitize the packaging PATH" >&2
    return 1
  fi
  local sanitized
  sanitized="$("$node" "$helper" --print "$root")"
  if [[ -z "$sanitized" || "$sanitized" == *"/usr/local/bin"* || "$sanitized" == *".hermes"* ]]; then
    echo "refusing unsafe packaging PATH: $sanitized" >&2
    return 1
  fi
  export PATH="$sanitized"
}
