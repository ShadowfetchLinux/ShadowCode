from __future__ import annotations

import json
import re
from pathlib import Path

from pydantic import BaseModel

from shadow_agent.models.types import ToolCall, ToolResult


class VerificationResult(BaseModel):
    ok: bool
    reason: str
    evidence: str = ""


class Verifier:
    """Refuse to declare success just because files were written."""

    def __init__(self, workspace: Path) -> None:
        self.workspace = Path(workspace)
        self.last_test: ToolResult | None = None
        self.last_execs: list[ToolResult] = []
        # 0.18.0: count successful file mutations so VERIFY can refuse a
        # "done" that changed nothing when the task clearly asked for a change.
        self.mutations = 0

    def observe(self, call: ToolCall, result: ToolResult) -> str | None:
        if call.tool_name in _MUTATING_TOOLS and result.success:
            self.mutations += 1
            return None
        if call.tool_name != "exec":
            return None
        self.last_execs.append(result)
        command = str(call.arguments.get("command") or "")
        if _is_test_command(command):
            self.last_test = result
            return "test.passed" if result.success else "test.failed"
        return None

    def verify(self, task: str) -> VerificationResult:
        lowered = task.lower()
        if re.search(r"hello[- ]?world", lowered):
            hello = self.workspace / "hello.py"
            if not hello.is_file():
                return VerificationResult(ok=False, reason="hello.py was not created")
            output = _collect_stdout(self.last_execs)
            # Default: case-insensitive match (real models print "Hello, world!"
            # as often as "Hello, World!"). But when the task explicitly demands
            # the exact casing "Hello, World" AND mentions a case mismatch / fix,
            # require the exact string so the FIX→OBSERVE→VERIFY path can fire.
            exact_required = (
                "Hello, World" in task
                and re.search(r"case\s*mismatch|exact\s*case|fix\s+the\s+case|correct\s+the\s+case", lowered)
            )
            if exact_required:
                if "Hello, World!" not in output:
                    return VerificationResult(
                        ok=False,
                        reason='output did not match exact "Hello, World!" (case-sensitive)',
                        evidence=output[-2000:],
                    )
                return VerificationResult(ok=True, reason="hello-world verified (exact case)", evidence=output[-500:])
            # Real models print "Hello, world!" as often as "Hello, World!" — match loosely.
            if not re.search(r"hello,\s*world!", output, re.IGNORECASE):
                return VerificationResult(ok=False, reason="hello.py was not run or output did not match", evidence=output[-2000:])
            return VerificationResult(ok=True, reason="hello-world verified", evidence=output[-500:])
        if re.search(r"test|pytest|failing", lowered):
            if self.last_test is None:
                return VerificationResult(ok=False, reason="tests were not executed")
            if not self.last_test.success:
                return VerificationResult(
                    ok=False,
                    reason="tests are still failing",
                    evidence=self.last_test.output[-2000:],
                )
            return VerificationResult(ok=True, reason="tests passed", evidence=self.last_test.output[-500:])
        # Generic: a task that asks for a change must have changed or run something.
        # Otherwise the model "finished" by narrating — send it back through FIX.
        if _wants_change(lowered) and self.mutations == 0 and not self.last_execs:
            return VerificationResult(
                ok=False,
                reason="the task asked for a change but no files were written and no command was run",
            )
        # Generic: if tests exist, they must have been run successfully at least once.
        if _has_tests(self.workspace):
            if self.last_test is None:
                return VerificationResult(ok=False, reason="project has tests that were never run")
            if not self.last_test.success:
                return VerificationResult(ok=False, reason="project tests failed", evidence=self.last_test.output[-2000:])
        return VerificationResult(ok=True, reason="no automated verification failed")


_MUTATING_TOOLS = {"write_file", "edit_file", "delete_file", "move_file", "apply_patch"}
_CHANGE_VERBS = re.compile(r"\b(create|write|add|implement|build|make|fix|edit|update|refactor|rename|delete|remove|generate|install|scaffold|run|execute)\b")
_READ_ONLY_VERBS = re.compile(r"\b(explain|summari[sz]e|review|describe|analy[sz]e|understand|inspect|audit|what|why|how|list|show|tell|find|read)\b")


def _wants_change(lowered_task: str) -> bool:
    """True when the task text is an instruction to change or run something,
    and is not primarily a read-only question."""
    if not _CHANGE_VERBS.search(lowered_task):
        return False
    return not _READ_ONLY_VERBS.search(lowered_task)


def _is_test_command(command: str) -> bool:
    return bool(re.search(r"\bpytest\b|\bpython3?\s+-m\s+pytest\b|\bpython3?\s+-m\s+unittest\b", command))


def _has_tests(workspace: Path) -> bool:
    if (workspace / "tests").is_dir():
        return True
    return any(workspace.glob("test_*.py")) or any(workspace.glob("*/test_*.py"))


def _collect_stdout(results: list[ToolResult]) -> str:
    chunks: list[str] = []
    for result in results:
        try:
            payload = json.loads(result.output)
        except json.JSONDecodeError:
            chunks.append(result.output)
            continue
        if isinstance(payload, dict):
            chunks.append(str(payload.get("stdout") or ""))
            chunks.append(str(payload.get("output") or ""))
    return "\n".join(chunks)
