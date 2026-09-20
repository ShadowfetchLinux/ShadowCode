"""Locate bundled assets in source checkouts, wheels, and standalone builds."""
from pathlib import Path
import sys


def resource_root() -> Path:
    if getattr(sys, "frozen", False):
        return Path(sys._MEIPASS)
    packaged = Path(__file__).parent / "_assets"
    if (packaged / "ui" / "dist" / "index.html").is_file():
        return packaged
    return Path(__file__).resolve().parents[2]
