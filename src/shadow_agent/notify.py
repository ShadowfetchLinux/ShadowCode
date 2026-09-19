"""Desktop notification when a long-running agent job finishes (notify-send)."""

from __future__ import annotations

import os
import shutil
import subprocess


def notify_done(
    task: str,
    *,
    success: bool,
    duration_sec: float,
    summary: str = "",
    enabled: bool = True,
    after_sec: float = 4.0,
) -> bool:
    """Send a desktop notification for jobs that ran long enough to walk away from.

    Returns True when a notification was actually dispatched. Never raises:
    notification failure must not affect the job result.
    """
    if not enabled or duration_sec < after_sec:
        return False
    if os.environ.get("SHADOW_AGENT_NO_NOTIFY"):
        return False
    binary = shutil.which("notify-send")
    if not binary:
        return False
    title = "ShadowCode — task complete" if success else "ShadowCode — task stopped"
    first_line = (summary or task).splitlines()[0][:140]
    body = f"{task.splitlines()[0][:80]}\n{first_line}" if summary else task.splitlines()[0][:160]
    try:
        subprocess.Popen(  # noqa: S603 - fixed binary, list args, no shell
            [binary, "-a", "ShadowCode", "-i", "shadow-agent", title, body],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return True
    except OSError:
        return False
