"""Deterministic offline model used for tests and first-run without API credits."""

from __future__ import annotations

import ast
import json
import re
import uuid
from collections.abc import Iterator

from shadow_agent.models.interface import ModelProvider
from shadow_agent.models.types import Capabilities, ChatRequest, ChatResponse, Message, ToolCall

_PYTEST = "python3 -B -m pytest -q --rootdir=. -p no:cacheprovider -o addopts="

_HELLO = re.compile(r"hello[- ]?world|print\(\s*['\"]hello", re.I)
_FIX_TESTS = re.compile(
    r"failing tests|fix(?:ing)? (?:the )?tests|find failing|rerun|pytest|broken tests",
    re.I,
)
_ASSERT = re.compile(
    r"assert\s+([A-Za-z_][A-Za-z0-9_]*)\(([^)]*)\)\s*==\s*(.+)$",
    re.M,
)


def _id() -> str:
    return "call_" + uuid.uuid4().hex[:10]


def _call(name: str, **arguments: object) -> ToolCall:
    return ToolCall(id=_id(), tool_name=name, arguments=dict(arguments))


def _resp(text: str = "", calls: list[ToolCall] | None = None, finish: bool = False) -> ChatResponse:
    return ChatResponse(text=text, tool_calls=calls or [], finish=finish)


def _last_user(messages: list[Message]) -> str:
    for msg in reversed(messages):
        if msg.role == "user":
            return msg.content
    return ""


def _tool_named(messages: list[Message], name: str) -> list[Message]:
    return [msg for msg in messages if msg.role == "tool" and msg.name == name]


def _parse_json(content: str) -> object:
    try:
        return json.loads(content)
    except json.JSONDecodeError:
        return content


def _unwrap(content: str) -> dict:
    payload = _parse_json(content)
    if not isinstance(payload, dict):
        return {"output": content, "raw": content}
    inner = payload.get("output")
    if isinstance(inner, str):
        parsed = _parse_json(inner)
        if isinstance(parsed, dict):
            merged = dict(payload)
            merged.update(parsed)
            return merged
    return payload


def _available(request: ChatRequest, name: str) -> bool:
    return any(spec.name == name for spec in request.tools) or not request.tools


class MockProvider(ModelProvider):
    name = "mock"

    def __init__(self, context_limit: int = 32000) -> None:
        self._context_limit = context_limit

    def get_capabilities(self) -> Capabilities:
        return Capabilities(chat=True, stream=True, tools=True, vision=False, provider="mock")

    def get_context_limit(self) -> int:
        return self._context_limit

    def generate(self, prompt: str, **kwargs: object) -> str:
        return self.chat(ChatRequest(messages=[Message(role="user", content=prompt)])).text

    def stream(self, request: ChatRequest) -> Iterator[str]:
        response = self.chat(request)
        if response.text:
            yield response.text

    def chat(self, request: ChatRequest) -> ChatResponse:
        task = _last_user(request.messages)
        if _HELLO.search(task):
            return self._hello(request)
        if _FIX_TESTS.search(task):
            return self._fix_tests(request)
        return self._generic(request, task)

    def _hello(self, request: ChatRequest) -> ChatResponse:
        listed = _tool_named(request.messages, "list_files")
        written = _tool_named(request.messages, "write_file")
        executed = _tool_named(request.messages, "exec")
        if not listed and _available(request, "list_files"):
            return _resp("Inspecting the workspace.", [_call("list_files", path=".")])
        if not written and _available(request, "write_file"):
            return _resp(
                "Creating a hello-world project.",
                [
                    _call("write_file", path="hello.py", content='print("Hello, World!")\n'),
                    _call("write_file", path="README.md", content="# Hello World\n\nRun `python3 hello.py`.\n"),
                ],
            )
        if not executed and _available(request, "exec"):
            return _resp("Running hello.py.", [_call("exec", command="python3 hello.py")])
        last_exec = executed[-1].content if executed else ""
        if "Hello, World!" in last_exec or '"Hello, World!"' in last_exec:
            return _resp(
                "Created hello.py, ran it, and verified output: Hello, World!",
                finish=True,
            )
        if executed and _available(request, "read_file"):
            if not _tool_named(request.messages, "read_file"):
                return _resp("Output unexpected; reading hello.py.", [_call("read_file", path="hello.py")])
            return _resp(
                "Rewriting hello.py and retrying.",
                [
                    _call("write_file", path="hello.py", content='print("Hello, World!")\n'),
                    _call("exec", command="python3 hello.py"),
                ],
            )
        return _resp("Hello-world project is ready.", finish=True)

    def _fix_tests(self, request: ChatRequest) -> ChatResponse:
        listed = _tool_named(request.messages, "list_files")
        executed = _tool_named(request.messages, "exec")
        pytest_runs = [msg for msg in executed if "pytest" in msg.content or self._cmd_has_pytest(msg)]
        if not listed and _available(request, "list_files"):
            return _resp("Inspecting the project.", [_call("list_files", path=".")])
        if not pytest_runs and _available(request, "exec"):
            return _resp("Running the test suite.", [_call("exec", command=_PYTEST)])
        last = pytest_runs[-1] if pytest_runs else None
        if last and self._pytest_passed(last.content):
            return _resp("Tests passed. Summarized: suite is green after analysis and fixes.", finish=True)
        if last and not self._files_read_for_failure(request, last.content):
            paths = self._failure_paths(request, last.content)
            if paths and _available(request, "read_file"):
                return _resp(
                    "Reading failing tests and implementation.",
                    [_call("read_file", path=path) for path in paths],
                )
        if last and _available(request, "edit_file"):
            patch = self._infer_patch(request)
            if patch:
                return _resp(
                    "Applying a structured fix from the failing assertions.",
                    [_call("write_file", **patch), _call("exec", command=_PYTEST)],
                )
            if _available(request, "search_text"):
                searches = _tool_named(request.messages, "search_text")
                if not searches:
                    return _resp(
                        "Searching for the implementation under test.",
                        [_call("search_text", query="def ", path=".")],
                    )
        return _resp(
            "Could not infer an automatic fix; see the last pytest output.",
            finish=True,
        )

    def _generic(self, request: ChatRequest, task: str) -> ChatResponse:
        if not _tool_named(request.messages, "list_files") and _available(request, "list_files"):
            return _resp("Inspecting the workspace.", [_call("list_files", path=".")])
        if "pytest" in task.lower() and _available(request, "exec"):
            if not any("pytest" in (msg.content or "") for msg in _tool_named(request.messages, "exec")):
                return _resp("Running tests.", [_call("exec", command=_PYTEST)])
        return _resp(f"Inspected the workspace. Task received: {task[:240]}", finish=True)

    @staticmethod
    def _cmd_has_pytest(message: Message) -> bool:
        payload = _unwrap(message.content)
        return "pytest" in str(payload.get("command", "")) or "pytest" in str(payload.get("stdout", "")) or "pytest" in message.content

    @staticmethod
    def _pytest_passed(content: str) -> bool:
        payload = _unwrap(content)
        text = json.dumps(payload)
        if payload.get("exit_code") not in (None, 0) or payload.get("success") is False:
            return False
        if payload.get("exit_code") == 0 or payload.get("success") is True:
            return "FAILED" not in text
        return bool(re.search(r"(\d+ passed|passed in )", text)) and "failed" not in text.lower()

    def _failure_paths(self, request: ChatRequest, content: str) -> list[str]:
        payload = _unwrap(content)
        text = " ".join(str(payload.get(key, "")) for key in ("stdout", "stderr", "error", "output"))
        found = re.findall(r"([A-Za-z0-9_./-]+\.py)(?::\d+)?", text)
        listed = self._paths_from_listing(request)
        listed_set = set(listed)
        out: list[str] = []
        for path in found + listed:
            if path in out:
                continue
            if "site-packages" in path or path.startswith("/") or path.endswith("pytest.py"):
                continue
            if listed_set and path not in listed_set and path not in listed:
                continue
            out.append(path)
        if not out:
            out = listed
        return out[:8]

    @staticmethod
    def _paths_from_listing(request: ChatRequest) -> list[str]:
        paths: list[str] = []
        for msg in _tool_named(request.messages, "list_files"):
            payload = _unwrap(msg.content)
            entries = payload.get("entries") if isinstance(payload, dict) else []
            if isinstance(entries, list):
                for item in entries:
                    name = item if isinstance(item, str) else str(item.get("path", item.get("name", "")))
                    if name.endswith(".py"):
                        paths.append(name)
        return paths

    def _files_read_for_failure(self, request: ChatRequest, content: str) -> bool:
        needed = self._failure_paths(request, content)
        read = {msg.content[:80] for msg in _tool_named(request.messages, "read_file")}
        # After at least one read of a .py file we can try a patch.
        reads = _tool_named(request.messages, "read_file")
        if not needed:
            return bool(reads)
        read_paths = []
        for msg in reads:
            payload = _unwrap(msg.content)
            if payload.get("path"):
                read_paths.append(str(payload["path"]))
        return any(path in read_paths or any(path in chunk for chunk in read) for path in needed) or len(reads) >= 2

    def _infer_patch(self, request: ChatRequest) -> dict[str, str] | None:
        sources: dict[str, str] = {}
        for msg in _tool_named(request.messages, "read_file"):
            payload = _unwrap(msg.content)
            if payload.get("content") is not None:
                sources[str(payload.get("path", "unknown"))] = str(payload["content"])
            else:
                sources[f"anon{len(sources)}"] = msg.content
        tests = "\n".join(text for path, text in sources.items() if "test" in path)
        impls = {path: text for path, text in sources.items() if "test" not in path}
        if not impls:
            impls = dict(sources)
        if not tests:
            tests = "\n".join(sources.values())
        cases: dict[str, list[tuple[list[str], str]]] = {}
        for match in _ASSERT.finditer(tests):
            fn, args, expected = match.group(1), match.group(2), match.group(3).strip()
            cases.setdefault(fn, []).append(([part.strip() for part in args.split(",") if part.strip()], expected))
        if not cases:
            return None
        fn, examples = next(iter(cases.items()))
        body = self._infer_body(examples)
        if not body:
            return None
        for path, text in impls.items():
            replaced = _replace_function(text, fn, body)
            if replaced:
                old, new = replaced
                if old != new:
                    return {"path": path, "content": text.replace(old, new, 1)}
        return None

    @staticmethod
    def _infer_body(examples: list[tuple[list[str], str]]) -> str | None:
        numeric: list[tuple[list[float], float]] = []
        for args, expected in examples:
            try:
                numeric.append(([float(ast.literal_eval(a)) for a in args], float(ast.literal_eval(expected))))
            except Exception:
                if len(examples) == 1 and not args:
                    return f"return {expected}"
                if len(examples) == 1:
                    return f"return {expected}"
                return None
        if not numeric:
            return None
        if all(len(args) == 2 for args, _ in numeric):
            if all(abs(args[0] + args[1] - expected) < 1e-9 for args, expected in numeric):
                return "return a + b"
            if all(abs(args[0] - args[1] - expected) < 1e-9 for args, expected in numeric):
                return "return a - b"
            if all(abs(args[0] * args[1] - expected) < 1e-9 for args, expected in numeric):
                return "return a * b"
        if all(len(args) == 0 for args, _ in numeric):
            return f"return {examples[0][1]}"
        return None


def _replace_function(text: str, fn: str, body: str) -> tuple[str, str] | None:
    lines = text.splitlines(keepends=True)
    start = next((i for i, line in enumerate(lines) if re.match(rf"def {re.escape(fn)}\(", line)), None)
    if start is None:
        return None
    end = start + 1
    while end < len(lines) and (lines[end].startswith((" ", "\t")) or lines[end].strip() == ""):
        if end + 1 < len(lines) and lines[end].strip() == "" and re.match(r"(def |class )", lines[end + 1]):
            break
        end += 1
    old = "".join(lines[start:end])
    new = lines[start] + f"    {body}\n"
    return old, new
