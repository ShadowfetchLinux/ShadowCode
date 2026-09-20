#!/usr/bin/env bash
# Source installation: isolated Python dependencies and an atomic launcher update.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN="$HOME/.local/bin"
PYTHON="${SHADOW_PYTHON:-python3}"
"$PYTHON" -c 'import sys; assert sys.version_info >= (3,12), "ShadowCode requires Python 3.12+"'
command -v npm >/dev/null || { echo 'Node.js 20.19+ or 22.12+ and npm are required to build the UI.' >&2; exit 1; }
[[ -x "$ROOT/.venv/bin/python" ]] || "$PYTHON" -m venv "$ROOT/.venv"
"$ROOT/.venv/bin/python" -m pip install -e "$ROOT"
npm --prefix "$ROOT/ui" ci --no-fund --no-audit
npm --prefix "$ROOT/ui" run build
mkdir -p "$BIN" "$PREFIX/applications" "$PREFIX/icons/hicolor/scalable/apps"
# Python loads secrets safely; the wrapper never sources secrets as shell code.
"$ROOT/.venv/bin/python" - "$ROOT" "$BIN" <<'PY'
import os, shlex, sys
from pathlib import Path
root, binary = map(Path, sys.argv[1:])
target = binary / 'shadow'
temporary = binary / '.shadow-install'
temporary.write_text('#!/usr/bin/env bash\nset -euo pipefail\nexec ' + shlex.quote(str(root / '.venv/bin/python')) + ' -m shadow_agent "$@"\n')
temporary.chmod(0o755)
os.replace(temporary, target)
PY
cp "$ROOT/assets/icons/shadow-agent.svg" "$PREFIX/icons/hicolor/scalable/apps/shadow-agent.svg"
sed "s|^Exec=.*|Exec=\"${BIN}/shadow\" ui|; s|^TryExec=.*|TryExec=${BIN}/shadow|" "$ROOT/packaging/shadow-agent.desktop" > "$PREFIX/applications/shadow-agent.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/applications" || true
printf 'Installed ShadowCode. Launch with %s/shadow ui\nExisting config and task history are preserved.\n' "$BIN"
