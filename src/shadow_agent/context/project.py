"""Load .shadow/{instructions.md,skills/,config/} and .shadowcode/{agents,hooks,mcp,skills}/ from a workspace.

Also accepts ``SHADOW.md`` at the repo root (Claude-Code-style) as an alias
for ``.shadow/instructions.md``. ``SHADOW.md`` takes precedence when both
exist, since it's the user-visible project file.
"""

from __future__ import annotations

from pathlib import Path


class ProjectSkills:
    def __init__(self, workspace: Path) -> None:
        self.workspace = Path(workspace)
        self.shadow = self.workspace / ".shadow"
        self.shadowcode = self.workspace / ".shadowcode"

    def instructions(self) -> str:
        # SHADOW.md at repo root takes precedence (Claude-Code-style).
        shadow_md = self.workspace / "SHADOW.md"
        if shadow_md.is_file():
            return shadow_md.read_text(encoding="utf-8")
        for base in [self.shadow, self.shadowcode]:
            path = base / "instructions.md"
            if path.is_file():
                return path.read_text(encoding="utf-8")
        return ""

    def skills(self) -> list[tuple[str, str]]:
        out: list[tuple[str, str]] = []
        for base in [self.shadowcode / "skills", self.shadow / "skills"]:
            if not base.is_dir():
                continue
            for path in sorted(base.glob("*.md")):
                text = path.read_text(encoding="utf-8")
                preview = "\n".join(text.splitlines()[:24])
                out.append((path.stem, preview))
        # Dedupe by name (shadowcode wins because it's iterated first).
        seen: set[str] = set()
        dedup: list[tuple[str, str]] = []
        for name, body in out:
            if name in seen:
                continue
            seen.add(name)
            dedup.append((name, body))
        return dedup

    def prompt_block(self) -> str:
        parts: list[str] = []
        inst = self.instructions()
        if inst:
            parts.append("Project instructions:\n" + inst)
        skills = self.skills()
        if skills:
            catalog = "\n\n".join(f"### {name}\n{body}" for name, body in skills)
            parts.append("Project skills:\n" + catalog)
        return "\n\n".join(parts)
