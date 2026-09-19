"""Load .shadow/{instructions.md,skills/,config/} from a workspace."""

from __future__ import annotations

from pathlib import Path


class ProjectSkills:
    def __init__(self, workspace: Path) -> None:
        self.root = Path(workspace) / ".shadow"

    def instructions(self) -> str:
        path = self.root / "instructions.md"
        if path.is_file():
            return path.read_text(encoding="utf-8")
        return ""

    def skills(self) -> list[tuple[str, str]]:
        skill_dir = self.root / "skills"
        if not skill_dir.is_dir():
            return []
        out: list[tuple[str, str]] = []
        for path in sorted(skill_dir.glob("*.md")):
            text = path.read_text(encoding="utf-8")
            preview = "\n".join(text.splitlines()[:24])
            out.append((path.stem, preview))
        return out

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
