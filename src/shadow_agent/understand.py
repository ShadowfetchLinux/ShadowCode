"""/understand — analyze a repo and save a project map to memory.

Detects language / framework / build / test / arch, lists important modules,
and flags obvious technical debt. The result is appended to
`.shadow/memory/project.md` so subsequent agent runs reuse it.

This is a deterministic, model-agnostic analyzer: it reads manifests and
file trees, never calls a model. A model can refine the map by editing
`.shadow/memory/project.md` directly.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

from shadow_agent.context.memory import MemoryStore


def _read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def detect_stack(workspace: Path) -> dict[str, Any]:
    """Identify language, framework, build, test, and arch signals."""
    ws = Path(workspace)
    stack: dict[str, Any] = {
        "languages": [],
        "frameworks": [],
        "build": [],
        "test": [],
        "arch": [],
        "manifests": [],
    }

    def has(*names: str) -> bool:
        return any((ws / name).exists() for name in names)

    if has("pyproject.toml", "setup.py", "setup.cfg"):
        stack["languages"].append("python")
        stack["manifests"].extend(p.name for p in [ws / "pyproject.toml", ws / "setup.py", ws / "setup.cfg"] if p.exists())
        if has("pyproject.toml"):
            text = _read(ws / "pyproject.toml")
            for fw, key in (("fastapi", "fastapi"), ("typer", "typer"), ("django", "django"), ("flask", "flask"), ("pytest", "pytest")):
                if key in text.lower():
                    stack["frameworks"].append(fw)
            if "setuptools" in text or "hatchling" in text:
                stack["build"].append("setuptools/hatch")
        if has("tests", "test") or any(ws.glob("test_*.py")) or any(ws.glob("*/test_*.py")):
            stack["test"].append("pytest")
    if has("package.json"):
        stack["languages"].append("typescript/javascript")
        stack["manifests"].append("package.json")
        text = _read(ws / "package.json")
        for fw, key in (("react", "react"), ("vite", "vite"), ("next", "next"), ("vue", "vue"), ("svelte", "svelte")):
            if f'"{key}"' in text:
                stack["frameworks"].append(fw)
        if "vite" in text:
            stack["build"].append("vite")
        if "vitest" in text or "jest" in text:
            stack["test"].append("vitest/jest")
    if has("Cargo.toml"):
        stack["languages"].append("rust")
        stack["manifests"].append("Cargo.toml")
        stack["build"].append("cargo")
        if has("tests") or any(ws.glob("tests/*.rs")):
            stack["test"].append("cargo test")
    if has("go.mod"):
        stack["languages"].append("go")
        stack["manifests"].append("go.mod")
        stack["build"].append("go")
        if any(ws.glob("*_test.go")) or any(ws.glob("**/*_test.go")):
            stack["test"].append("go test")
    if has("meson.build"):
        stack["build"].append("meson")
        stack["languages"].append("c/c++/vala")
    if has("CMakeLists.txt"):
        stack["build"].append("cmake")
        stack["languages"].append("c/c++")
    if has("Dockerfile", "docker-compose.yml", "compose.yaml"):
        stack["arch"].append("containerized")
    if has(".github/workflows"):
        stack["arch"].append("ci/cd")
    if has("src/") and (ws / "src").is_dir():
        stack["arch"].append("src/ layout")
    if has("docs"):
        stack["arch"].append("docs/")
    # De-duplicate while preserving order.
    for key in ("languages", "frameworks", "build", "test", "arch"):
        seen: list[str] = []
        for item in stack[key]:
            if item not in seen:
                seen.append(item)
        stack[key] = seen
    return stack


def list_modules(workspace: Path, max_modules: int = 12) -> list[dict[str, Any]]:
    """List the most important top-level modules (dirs with code)."""
    ws = Path(workspace)
    skip = {"node_modules", ".git", ".venv", "venv", "__pycache__", "dist", "build", ".tox", ".mypy_cache", ".pytest_cache", "target"}
    modules: list[dict[str, Any]] = []
    for entry in sorted(ws.iterdir()):
        if not entry.is_dir() or entry.name in skip or entry.name.startswith("."):
            continue
        files = [p for p in entry.rglob("*") if p.is_file() and p.suffix in {".py", ".ts", ".tsx", ".js", ".rs", ".go"}]
        if not files:
            continue
        modules.append({"name": entry.name, "files": len(files)})
        if len(modules) >= max_modules:
            break
    return modules


def flag_debt(workspace: Path) -> list[str]:
    """Cheap technical-debt flags from file presence and sizes."""
    ws = Path(workspace)
    debt: list[str] = []
    if (ws / "TODO").is_file() or (ws / "TODO.md").is_file():
        debt.append("TODO file present — open tasks tracked outside code")
    todos = 0
    for p in ws.rglob("*"):
        if p.suffix in {".py", ".ts", ".tsx", ".js", ".rs", ".go"} and "node_modules" not in str(p) and ".venv" not in str(p):
            try:
                text = p.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            todos += len(re.findall(r"\b(TODO|FIXME|XXX|HACK)\b", text))
            if len(text) > 100_000:
                debt.append(f"large file: {p.relative_to(ws)} ({len(text)//1000} KB)")
    if todos > 50:
        debt.append(f"{todos} TODO/FIXME markers — consider cleanup")
    elif todos > 10:
        debt.append(f"{todos} TODO/FIXME markers")
    if (ws / ".github" / "workflows").is_dir():
        workflows = list((ws / ".github" / "workflows").iterdir())
        if not workflows:
            debt.append(".github/workflows is empty — CI not configured")
    return debt


def build_project_map(workspace: Path) -> dict[str, Any]:
    return {
        "workspace": str(Path(workspace).resolve()),
        "stack": detect_stack(workspace),
        "modules": list_modules(workspace),
        "debt": flag_debt(workspace),
    }


def render_map(mp: dict[str, Any]) -> str:
    """Render the map as Markdown for `.shadow/memory/project.md`."""
    stack = mp["stack"]
    lines = ["# Project map (auto-generated by /understand)", ""]
    lines.append(f"- **Languages**: {', '.join(stack['languages']) or 'unknown'}")
    lines.append(f"- **Frameworks**: {', '.join(stack['frameworks']) or 'none detected'}")
    lines.append(f"- **Build**: {', '.join(stack['build']) or 'none detected'}")
    lines.append(f"- **Test**: {', '.join(stack['test']) or 'none detected'}")
    lines.append(f"- **Arch**: {', '.join(stack['arch']) or 'unspecified'}")
    lines.append(f"- **Manifests**: {', '.join(stack['manifests']) or 'none'}")
    lines.append("")
    lines.append("## Important modules")
    if mp["modules"]:
        for m in mp["modules"]:
            lines.append(f"- `{m['name']}/` — {m['files']} source files")
    else:
        lines.append("- (no top-level code modules detected)")
    lines.append("")
    lines.append("## Technical debt")
    if mp["debt"]:
        for d in mp["debt"]:
            lines.append(f"- {d}")
    else:
        lines.append("- (nothing obvious flagged)")
    return "\n".join(lines) + "\n"


def save_project_map(workspace: Path, task_id: str = "understand") -> dict[str, Any]:
    """Build the map, append it to project memory, and return it."""
    mp = build_project_map(workspace)
    rendered = render_map(mp)
    # Append to the project memory file directly so it persists across sessions.
    memory = MemoryStore(workspace, task_id)
    memory.append_project(rendered)
    return mp
