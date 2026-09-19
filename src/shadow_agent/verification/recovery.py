from __future__ import annotations

from shadow_agent.verification.verifier import VerificationResult


class RecoveryPolicy:
    def __init__(self, max_retries: int = 4) -> None:
        self.max_retries = max_retries
        self.attempts = 0
        self.history: list[str] = []

    def note(self, verdict: VerificationResult) -> str:
        self.attempts += 1
        self.history.append(verdict.reason)
        if self.attempts >= self.max_retries:
            return (
                f"Verification failed {self.attempts} times ({verdict.reason}). "
                "Stop claiming success. Summarize remaining issues."
            )
        return (
            "VERIFICATION FAILED. Do not finish. "
            f"Reason: {verdict.reason}. Evidence:\n{verdict.evidence[:2000]}\n"
            "Inspect the failure, apply a structured fix, re-run verification."
        )

    def exhausted(self) -> bool:
        return self.attempts >= self.max_retries
