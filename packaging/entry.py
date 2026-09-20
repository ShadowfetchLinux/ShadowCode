"""Standalone entry point: file-manager launch opens the desktop."""
import sys
import os
# PyInstaller adjusts the library search path for its own bootloader. Restore
# the system path before launching the browser, git, Python, or user commands.
if "LD_LIBRARY_PATH_ORIG" in os.environ:
    os.environ["LD_LIBRARY_PATH"] = os.environ["LD_LIBRARY_PATH_ORIG"]
else:
    os.environ.pop("LD_LIBRARY_PATH", None)
from shadow_agent.cli import entry
if len(sys.argv) == 1:
    sys.argv.append("ui")
entry()
