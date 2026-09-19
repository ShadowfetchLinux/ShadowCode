"""In-process event bus + JSONL audit log."""

from __future__ import annotations

import json
import threading
import time
from collections import defaultdict
from collections.abc import Callable
from pathlib import Path
from typing import Any

from shadow_agent import paths

Listener = Callable[[str, dict[str, Any]], None]


class EventBus:
    def __init__(self, log_path: Path | None = None) -> None:
        self._lock = threading.RLock()
        self._listeners: dict[str, list[Listener]] = defaultdict(list)
        self._any: list[Listener] = []
        self._history: list[dict[str, Any]] = []
        self._log_path = log_path

    def subscribe(self, event_type: str | None, listener: Listener) -> None:
        with self._lock:
            if event_type is None:
                self._any.append(listener)
            else:
                self._listeners[event_type].append(listener)

    def emit(
        self,
        event_type: str,
        payload: dict[str, Any] | None = None,
        *,
        session_id: str | None = None,
        task_id: str | None = None,
    ) -> dict[str, Any]:
        event = {
            "ts": time.time(),
            "type": event_type,
            "session_id": session_id,
            "task_id": task_id,
            "payload": payload or {},
        }
        with self._lock:
            self._history.append(event)
            listeners = list(self._listeners.get(event_type, [])) + list(self._any)
            log_path = self._log_path or paths.events_log()
        self._append_log(log_path, event)
        for listener in listeners:
            try:
                listener(event_type, event)
            except Exception:
                continue
        return event

    def history(self, event_type: str | None = None, limit: int = 200) -> list[dict[str, Any]]:
        with self._lock:
            rows = list(self._history)
        if event_type:
            rows = [row for row in rows if row["type"] == event_type]
        return rows[-limit:]

    @staticmethod
    def _append_log(path: Path, event: dict[str, Any]) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(event, default=str) + "\n")
