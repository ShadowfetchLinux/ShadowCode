"""Queue for dangerous-command approve/deny from the desktop UI."""

from __future__ import annotations

import threading
import time
import uuid
from typing import Any, Callable


class ApprovalTimeout(TimeoutError):
    pass


class PendingApproval:
    def __init__(self, payload: dict[str, Any]) -> None:
        self.id = uuid.uuid4().hex
        self.payload = payload
        self.created_at = time.time()
        self.decision = ""
        self.event = threading.Event()

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "created_at": self.created_at,
            "decision": self.decision,
            "pending": not self.event.is_set(),
            **self.payload,
        }


class ApprovalHub:
    def __init__(self, timeout_sec: float = 300.0) -> None:
        self.timeout_sec = timeout_sec
        self._lock = threading.Lock()
        self._pending: dict[str, PendingApproval] = {}
        self.on_request: Callable[[PendingApproval], None] | None = None

    def request(self, payload: dict[str, Any]) -> str:
        item = PendingApproval(payload)
        with self._lock:
            self._pending[item.id] = item
        if self.on_request:
            try:
                self.on_request(item)
            except Exception:
                pass
        if not item.event.wait(timeout=self.timeout_sec):
            item.decision = "timeout"
            item.event.set()
        return item.decision or "deny"

    def decide(self, approval_id: str, decision: str) -> dict[str, Any]:
        if decision not in {"approve", "deny"}:
            raise ValueError("decision must be approve or deny")
        with self._lock:
            item = self._pending.get(approval_id)
        if item is None:
            raise KeyError(approval_id)
        item.decision = decision
        item.event.set()
        return item.to_dict()

    def list_pending(self, session_id: str | None = None) -> list[dict[str, Any]]:
        with self._lock:
            rows = [item.to_dict() for item in self._pending.values() if not item.event.is_set()]
        if session_id:
            rows = [row for row in rows if row.get("session_id") == session_id]
        return rows

    def get(self, approval_id: str) -> dict[str, Any] | None:
        with self._lock:
            item = self._pending.get(approval_id)
        return item.to_dict() if item else None
