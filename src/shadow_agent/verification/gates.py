"""Verification gates: IMPLEMENT → BUILD → TEST → LINT → SECURITY → GIT DIFF → REVIEW → DONE.

Each gate is a small, deterministic checker that returns a GateResult. The
VerificationEngine runs gates in order; on failure it emits a `gate.failed`
event and the agent loop's FIX retry kicks in (up to max_fix_retries).

Gates are best-effort and project-aware: if a gate doesn't apply (e.g. no
linter configured, no git repo), it passes with `skipped=True` so the engine
never blocks on a missing toolchain.
"""

from __future__ import annotations

import re
import subprocess
from enum import Enum
from pathlib import Path
from typing import Any, Callable

from pydantic import BaseModel


class Gate(str, Enum):
    IMPLEMENT = "IMPLEMENT"
    BUILD = "BUILD"
    TEST = "TEST"
    LINT = "LINT"
    SECURITY = "SECURITY"
    GIT_DIFF = "GIT_DIFF"
    REVIEW = "REVIEW"
    DONE = "DONE"


GATE_ORDER = [Gate.IMPLEMENT, Gate.BUILD, Gate.TEST, Gate.LINT, Gate.SECURITY, Gate.GIT_DIFF, Gate.REVIEW, Gate.DONE]


class GateResult(BaseModel):
    gate: Gate
    ok: bool
    skipped: bool = False
    reason: str = ""
    evidence: str = ""


class VerificationEngine:
    """Runs the 8-gate pipeline. Returns the list of GateResults.

    The engine is provider-agnostic and side-effect-free except for the shell
    commands each gate runs (build, test, lint). It does not mutate files.
    """

    def __init__(self, workspace: Path, emit: Callable[[str, dict], None] | None = None) -> None:
        self.workspace = Path(workspace).resolve()
        self._emit = emit

    def run(self, task: str = "", max_attempts: int = 3) -> list[GateResult]:
        """Run all gates. On failure, the caller (agent loop) retries up to
        max_attempts; this method runs one full pass."""
        results: list[GateResult] = []
        for gate in GATE_ORDER:
            if gate is Gate.DONE:
                # DONE only passes if every prior gate passed (not skipped-failed).
                ok = all(r.ok for r in results if not r.skipped)
                gr = GateResult(gate=Gate.DONE, ok=ok, reason="all prior gates passed" if ok else "a prior gate failed")
                results.append(gr)
                self._emit_gate(gr)
                break
            gr = self._run_gate(gate, task, results)
            results.append(gr)
            self._emit_gate(gr)
            if not gr.ok and not gr.skipped:
                break
        return results

    def _emit_gate(self, gr: GateResult) -> None:
        if self._emit:
            self._emit(
                "gate.result",
                {"gate": gr.gate.value, "ok": gr.ok, "skipped": gr.skipped, "reason": gr.reason, "evidence": gr.evidence[:1000]},
            )

    def _run_gate(self, gate: Gate, task: str, prior: list[GateResult]) -> GateResult:
        if gate is Gate.IMPLEMENT:
            return self._gate_implement(task)
        if gate is Gate.BUILD:
            return self._gate_build()
        if gate is Gate.TEST:
            return self._gate_test()
        if gate is Gate.LINT:
            return self._gate_lint()
        if gate is Gate.SECURITY:
            return self._gate_security()
        if gate is Gate.GIT_DIFF:
            return self._gate_git_diff()
        if gate is Gate.REVIEW:
            return self._gate_review(prior)
        return GateResult(gate=gate, ok=True, skipped=True, reason="unknown gate")

    def _gate_implement(self, task: str) -> GateResult:
        """IMPLEMENT: at least one source file was created/modified this task."""
        # Heuristic: the workspace has at least one source file.
        src_exts = {".py", ".ts", ".tsx", ".js", ".rs", ".go"}
        files = [p for p in self.workspace.rglob("*") if p.suffix in src_exts and ".venv" not in str(p) and "node_modules" not in str(p)]
        if not files:
            return GateResult(gate=Gate.IMPLEMENT, ok=False, reason="no source files found in the workspace")
        return GateResult(gate=Gate.IMPLEMENT, ok=True, reason=f"{len(files)} source file(s) present")

    def _gate_build(self) -> GateResult:
        """BUILD: run the project's build command if one is detected."""
        cmd = self._detect_build_cmd()
        if not cmd:
            return GateResult(gate=Gate.BUILD, ok=True, skipped=True, reason="no build command detected")
        return self._run_cmd(cmd, Gate.BUILD)

    def _gate_test(self) -> GateResult:
        """TEST: run pytest if tests exist."""
        if not self._has_tests():
            return GateResult(gate=Gate.TEST, ok=True, skipped=True, reason="no tests in the workspace")
        return self._run_cmd(["python3", "-B", "-m", "pytest", "-q"], Gate.TEST)

    def _gate_lint(self) -> GateResult:
        """LINT: run ruff if installed and a pyproject.toml is present."""
        if not (self.workspace / "pyproject.toml").is_file():
            return GateResult(gate=Gate.LINT, ok=True, skipped=True, reason="no pyproject.toml")
        import shutil
        if not shutil.which("ruff"):
            return GateResult(gate=Gate.LINT, ok=True, skipped=True, reason="ruff not installed")
        return self._run_cmd(["ruff", "check", "."], Gate.LINT)

    def _gate_security(self) -> GateResult:
        """SECURITY: cheap scan for secrets in code and obviously unsafe patterns."""
        issues: list[str] = []
        secret_pat = re.compile(r"(sk-[A-Za-z0-9]{20,}|AKIA[0-9A-Z]{16}|ghp_[A-Za-z0-9]{36})")
        unsafe_pat = re.compile(r"\beval\s*\(|\bexec\s*\(\s*input|pickle\.loads|subprocess\.shell=True|os\.system\b")
        for p in self.workspace.rglob("*"):
            if p.suffix not in {".py", ".ts", ".tsx", ".js", ".rs", ".go"}:
                continue
            if ".venv" in str(p) or "node_modules" in str(p):
                continue
            try:
                text = p.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            for m in secret_pat.finditer(text):
                issues.append(f"{p.relative_to(self.workspace)}: hardcoded secret-looking token")
                break
            for m in unsafe_pat.finditer(text):
                issues.append(f"{p.relative_to(self.workspace)}: unsafe pattern '{m.group(0)}'")
        if issues:
            return GateResult(gate=Gate.SECURITY, ok=False, reason="; ".join(issues[:5]), evidence="\n".join(issues))
        return GateResult(gate=Gate.SECURITY, ok=True, reason="no obvious secrets or unsafe patterns")

    def _gate_git_diff(self) -> GateResult:
        """GIT_DIFF: confirm the change is captured as a git diff (if a repo)."""
        if not (self.workspace / ".git").is_dir():
            return GateResult(gate=Gate.GIT_DIFF, ok=True, skipped=True, reason="not a git repo")
        proc = subprocess.run(["git", "diff", "--stat"], cwd=self.workspace, capture_output=True, text=True, check=False)
        if proc.returncode != 0:
            return GateResult(gate=Gate.GIT_DIFF, ok=False, reason="git diff failed", evidence=proc.stderr)
        if not proc.stdout.strip():
            return GateResult(gate=Gate.GIT_DIFF, ok=True, skipped=True, reason="no uncommitted changes")
        return GateResult(gate=Gate.GIT_DIFF, ok=True, reason="changes present in git diff", evidence=proc.stdout[:1000])

    def _gate_review(self, prior: list[GateResult]) -> GateResult:
        """REVIEW: a stub peer-review gate. Passes if IMPLEMENT+TEST+LINT+SECURITY all passed."""
        required = {Gate.IMPLEMENT, Gate.TEST, Gate.LINT, Gate.SECURITY}
        relevant = [r for r in prior if r.gate in required]
        if all(r.ok for r in relevant):
            return GateResult(gate=Gate.REVIEW, ok=True, reason="all required gates passed (auto-review)")
        failed = [r.gate.value for r in relevant if not r.ok]
        return GateResult(gate=Gate.REVIEW, ok=False, reason=f"cannot review: {', '.join(failed)} failed")

    def _detect_build_cmd(self) -> list[str] | None:
        if (self.workspace / "package.json").is_file():
            return ["npm", "run", "build"]
        if (self.workspace / "Cargo.toml").is_file():
            return ["cargo", "build", "--quiet"]
        if (self.workspace / "Makefile").is_file():
            return ["make", "-s"]
        return None

    def _has_tests(self) -> bool:
        if (self.workspace / "tests").is_dir():
            return True
        return any(self.workspace.glob("test_*.py")) or any(self.workspace.glob("*/test_*.py"))

    def _run_cmd(self, cmd: list[str], gate: Gate) -> GateResult:
        try:
            proc = subprocess.run(cmd, cwd=self.workspace, capture_output=True, text=True, timeout=120, check=False)
        except FileNotFoundError:
            return GateResult(gate=gate, ok=True, skipped=True, reason=f"{cmd[0]} not installed")
        except subprocess.TimeoutExpired:
            return GateResult(gate=gate, ok=False, reason=f"{cmd[0]} timed out", evidence="")
        ok = proc.returncode == 0
        evidence = (proc.stdout + proc.stderr)[:2000]
        return GateResult(gate=gate, ok=ok, reason=f"{cmd[0]} exit {proc.returncode}", evidence=evidence)


def render_gate_results(results: list[GateResult], attempt: int = 1, max_attempts: int = 3) -> str:
    """One-screen summary: 'Attempt N → passed/failed' per gate."""
    all_ok = all(r.ok for r in results)
    status = "passed" if all_ok else "failed"
    lines = [f"Attempt {attempt}/{max_attempts} → {status}", ""]
    for r in results:
        if r.skipped:
            mark = "⊘"
            tag = "skipped"
        elif r.ok:
            mark = "✓"
            tag = "passed"
        else:
            mark = "✗"
            tag = "FAILED"
        lines.append(f"  {mark} {r.gate.value:10} {tag:8}  {r.reason}")
    return "\n".join(lines)
