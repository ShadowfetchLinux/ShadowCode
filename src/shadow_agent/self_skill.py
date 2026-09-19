"""Self-skilling — detect repeated workflows and synthesize reusable skills.

When ShadowCode notices a workflow has been repeated N times (default 4), it
offers: "I've noticed this workflow has been repeated N times. Create
``/<name>`` as a reusable ShadowCode skill?" On yes, it records a skill under
``.shadowcode/skills/<name>.md`` with the captured steps. Then ``/<name>``
runs that pipeline.

The detector normalizes task prompts into a workflow signature (lowercased,
trimmed, words only) and counts occurrences in the session event log. When
the count crosses the threshold, ``detect()`` returns the signature and
sample prompts so the UI/CLI can offer the synthesis. ``synthesize_skill``
writes the skill file; ``run_skill`` reads it back as a prompt for the agent.
"""

from __future__ import annotations

import re
import time
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from shadow_agent import paths

DEFAULT_THRESHOLD = 4
SKILLS_DIR_NAME = "skills"


def _signature(task: str) -> str:
    """Normalize a task prompt into a workflow signature."""
    text = task.lower().strip()
    text = re.sub(r"[^a-z0-9 ]+", " ", text)
    text = re.sub(r"\s+", " ", text)
    # Drop trailing punctuation artifacts and common stop words at the start.
    words = [w for w in text.split() if w not in {"the", "a", "an", "please", "this", "that", "to", "of", "for"}]
    return " ".join(words)[:120]


@dataclass
class WorkflowObservation:
    signature: str
    count: int
    samples: list[str] = field(default_factory=list)
    last_ts: float = 0.0


class WorkflowDetector:
    """Tracks task prompts and flags repeated workflows.

    Observations are persisted to ``state_dir/workflows.json`` so the count
    survives restarts. The detector is best-effort: it never blocks the loop.
    """

    def __init__(self, state_dir: Path | None = None, threshold: int = DEFAULT_THRESHOLD) -> None:
        self.root = state_dir or paths.state_dir()
        self.root.mkdir(parents=True, exist_ok=True)
        self.path = self.root / "workflows.json"
        self.threshold = threshold
        self._observations: dict[str, WorkflowObservation] = {}
        self._load()
        self._offered: set[str] = set()

    def _load(self) -> None:
        if not self.path.is_file():
            return
        try:
            import json

            data = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return
        for sig, raw in (data.get("observations") or {}).items():
            self._observations[sig] = WorkflowObservation(
                signature=sig,
                count=int(raw.get("count", 0)),
                samples=list(raw.get("samples") or []),
                last_ts=float(raw.get("last_ts", 0.0)),
            )
        self._offered = set(data.get("offered") or [])

    def _save(self) -> None:
        import json

        data = {
            "observations": {
                sig: {"count": obs.count, "samples": obs.samples[:8], "last_ts": obs.last_ts}
                for sig, obs in self._observations.items()
            },
            "offered": sorted(self._offered),
        }
        self.path.write_text(json.dumps(data, indent=2), encoding="utf-8")

    def observe(self, task: str) -> WorkflowObservation:
        sig = _signature(task)
        obs = self._observations.get(sig)
        if obs is None:
            obs = WorkflowObservation(signature=sig, count=0)
            self._observations[sig] = obs
        obs.count += 1
        if task not in obs.samples:
            obs.samples.insert(0, task)
            obs.samples = obs.samples[:8]
        obs.last_ts = time.time()
        self._save()
        return obs

    def detect(self) -> WorkflowObservation | None:
        """Return the first workflow that has crossed the threshold and not
        yet been offered. Returns None if no workflow qualifies."""
        for obs in self._observations.values():
            if obs.count >= self.threshold and obs.signature not in self._offered:
                return obs
        return None

    def mark_offered(self, signature: str) -> None:
        self._offered.add(signature)
        self._save()

    def reset(self) -> None:
        self._observations.clear()
        self._offered.clear()
        self._save()


def synthesize_skill(workspace: Path, name: str, steps: list[str], *, description: str = "") -> Path:
    """Write ``.shadowcode/skills/<name>.md`` with the captured steps."""
    skill_dir = workspace / ".shadowcode" / "skills"
    skill_dir.mkdir(parents=True, exist_ok=True)
    safe = re.sub(r"[^a-z0-9_-]+", "-", name.lower()).strip("-")
    if not safe:
        safe = "skill"
    path = skill_dir / f"{safe}.md"
    body = [f"# {name}", ""]
    if description:
        body.append(description)
        body.append("")
    body.append("Steps:")
    for i, step in enumerate(steps, 1):
        body.append(f"{i}. {step}")
    body.append("")
    path.write_text("\n".join(body), encoding="utf-8")
    return path


def list_skills(workspace: Path) -> list[dict[str, str]]:
    """List saved skills in ``.shadowcode/skills/`` (and legacy ``.shadow/skills``)."""
    out: list[dict[str, str]] = []
    for base in [workspace / ".shadowcode" / "skills", workspace / ".shadow" / "skills"]:
        if not base.is_dir():
            continue
        for path in sorted(base.glob("*.md")):
            out.append({"name": path.stem, "path": str(path.relative_to(workspace)) if path.is_relative_to(workspace) else str(path), "preview": path.read_text(encoding="utf-8")[:240]})
    return out


def load_skill(workspace: Path, name: str) -> str | None:
    """Read a skill file by stem name. Returns None if not found."""
    safe = re.sub(r"[^a-z0-9_-]+", "-", name.lower()).strip("-")
    for base in [workspace / ".shadowcode" / "skills", workspace / ".shadow" / "skills"]:
        path = base / f"{safe}.md"
        if path.is_file():
            return path.read_text(encoding="utf-8")
    return None
