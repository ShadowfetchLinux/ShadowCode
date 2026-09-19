"""Turn raw exceptions and tool errors into short, human-readable sentences."""

from __future__ import annotations

from typing import Any


def friendly_error(error: Any) -> str:
    text = str(error or "").strip() or "Something went wrong."
    lowered = text.lower()

    rules: list[tuple[str, str]] = [
        ("workspace not found", "That folder does not exist. Pick a project folder first."),
        ("workspace does not exist", "That folder does not exist. Pick a project folder first."),
        ("path escapes workspace", "ShadowCode will not touch files outside the project folder."),
        ("not a directory", "That path is not a folder."),
        ("binary file", "That file is binary, so ShadowCode will not open it as text."),
        ("old_string not found", "The edit missed: that text is no longer in the file. Re-read the file and try again."),
        ("old_string is not unique", "The edit is ambiguous because that text appears more than once. Add nearby lines so it is unique."),
        ("dangerous command requires elevated", "That command is blocked. Raise permissions to Elevated and approve it in the UI."),
        ("dangerous command waiting", "That command needs your approval in the UI before it can run."),
        ("network commands are disabled", "Network commands are off. Enable network in Settings if you really need them."),
        ("root/sudo is disabled", "sudo and root commands are disabled."),
        ("blocked in read-only", "The agent is in read-only mode, so it cannot change files or run write commands."),
        ("requires elevated", "That action needs Elevated permissions."),
        ("user denied", "You denied that command."),
        ("cancelled", "The agent was stopped."),
        ("connection refused", "The model endpoint is not reachable. Check the URL, or switch to Mock to work offline."),
        ("timed out", "The model or command timed out. Try again, or raise the tool timeout in Settings."),
        ("unauthorized", "The API key was rejected. Paste a new key in Settings or Onboarding."),
        ("401", "The API key was rejected. Paste a new key in Settings or Onboarding."),
        ("api key", "No API key is loaded. Paste one in Onboarding or set the environment variable named in Settings."),
        ("unknown tool", "The model called a tool this harness does not have."),
        ("not found:", "A file or folder the agent expected is missing. Inspect the project and retry."),
    ]
    for needle, message in rules:
        if needle in lowered:
            return message
    if len(text) > 240:
        text = text[:237] + "…"
    return text


def friendly_http(status: int, detail: str = "") -> str:
    if status == 400:
        return friendly_error(detail or "The request was invalid.")
    if status == 404:
        return friendly_error(detail or "That item was not found.")
    if status == 409:
        return friendly_error(detail or "That action conflicts with the current state.")
    if status >= 500:
        return "The Agent API hit an internal error. Check ~/.local/state/shadow-agent/ui.log."
    return friendly_error(detail or f"HTTP {status}")
