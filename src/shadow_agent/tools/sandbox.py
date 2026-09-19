"""Workspace path sandbox. All filesystem tools resolve through here."""

from __future__ import annotations

from pathlib import Path


class SandboxError(ValueError):
    pass


class WorkspaceSandbox:
    def __init__(self, root: Path) -> None:
        self.root = Path(root).resolve()
        if not self.root.is_dir():
            raise SandboxError(f"workspace does not exist: {self.root}")

    def resolve(self, rel: str | Path, *, must_exist: bool = False) -> Path:
        raw = Path(rel) if rel not in (None, "") else Path(".")
        if raw.is_absolute():
            candidate = raw.resolve()
        else:
            candidate = (self.root / raw).resolve()
        try:
            candidate.relative_to(self.root)
        except ValueError as exc:
            raise SandboxError(f"path escapes workspace: {rel}") from exc
        if must_exist and not candidate.exists():
            raise SandboxError(f"not found: {rel}")
        return candidate

    def relative(self, path: Path) -> str:
        resolved = path.resolve()
        return str(resolved.relative_to(self.root))

    def is_inside(self, path: Path) -> bool:
        try:
            path.resolve().relative_to(self.root)
            return True
        except ValueError:
            return False
