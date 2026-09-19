"""Documentation research — implement a feature using upstream docs.

Workflow: SEARCH DOCS → READ API CHANGES → COMPARE CURRENT CODE → PLAN
MIGRATION → IMPLEMENT → TEST.

Uses a small WebFetch-style helper (httpx GET) to read a documentation URL
and extract the relevant section. If offline (no network / fetch fails), it
degrades gracefully: it returns a structured plan based on the task text
alone and tells the user to provide the docs URL.

This module is the *harness* side: it produces a structured DocResearchPlan
the agent loop can execute. The actual web fetch is a tool call so the
agent's permission gate still applies.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import httpx


@dataclass
class DocResearchPlan:
    task: str
    steps: list[dict[str, str]] = field(default_factory=list)
    docs_url: str = ""
    fetched_snippet: str = ""
    offline: bool = False
    notes: str = ""


def _build_steps(task: str, docs_url: str) -> list[dict[str, str]]:
    return [
        {"id": "search", "title": f"Search docs for: {task[:80]}", "status": "pending"},
        {"id": "read", "title": f"Read API changes at {docs_url or '<docs url>'}", "status": "pending"},
        {"id": "compare", "title": "Compare current code against the documented API", "status": "pending"},
        {"id": "plan", "title": "Plan the migration (list affected files + change per file)", "status": "pending"},
        {"id": "implement", "title": "Implement the migration", "status": "pending"},
        {"id": "test", "title": "Run tests and verify", "status": "pending"},
    ]


def plan_doc_research(task: str, docs_url: str = "", workspace: Path | None = None) -> DocResearchPlan:
    """Build a DocResearchPlan. If `docs_url` is given, fetch a snippet."""
    plan = DocResearchPlan(task=task, docs_url=docs_url, steps=_build_steps(task, docs_url))
    if not docs_url:
        plan.offline = True
        plan.notes = "No docs URL provided. Provide one with `/docs <url> <task>` or set routing.vision for a vision-capable model to read screenshots."
        return plan
    try:
        resp = httpx.get(docs_url, timeout=10.0, follow_redirects=True, headers={"User-Agent": "ShadowCode/0.8"})
        if resp.status_code != 200:
            plan.offline = True
            plan.notes = f"docs fetch returned HTTP {resp.status_code}; proceeding with the plan but no snippet."
            return plan
        # Extract a readable snippet: strip HTML tags, collapse whitespace.
        text = re.sub(r"<script.*?</script>", " ", resp.text, flags=re.DOTALL | re.IGNORECASE)
        text = re.sub(r"<style.*?</style>", " ", text, flags=re.DOTALL | re.IGNORECASE)
        text = re.sub(r"<[^>]+>", " ", text)
        text = re.sub(r"\s+", " ", text).strip()
        plan.fetched_snippet = text[:4000]
        plan.steps[1]["status"] = "done"
        plan.steps[1]["detail"] = f"fetched {len(text)} chars"
    except Exception as exc:  # noqa: BLE001 - offline is a valid state
        plan.offline = True
        plan.notes = f"docs fetch failed ({exc.__class__.__name__}); proceeding offline."
    return plan


def render_plan(plan: DocResearchPlan) -> str:
    lines = [f"Documentation research  ·  {plan.task[:80]}", ""]
    if plan.offline:
        lines.append(f"(offline) {plan.notes}")
        lines.append("")
    for step in plan.steps:
        mark = {"done": "✓", "pending": "○", "in_progress": "▸"}.get(step["status"], "○")
        lines.append(f"  {mark} {step['id']:10} {step['title']}")
        if step.get("detail"):
            lines.append(f"      {step['detail']}")
    if plan.fetched_snippet:
        lines.append("")
        lines.append("Fetched snippet (first 600 chars):")
        lines.append(plan.fetched_snippet[:600])
    return "\n".join(lines)
