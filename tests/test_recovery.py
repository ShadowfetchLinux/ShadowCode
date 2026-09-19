from __future__ import annotations

from shadow_agent.verification.recovery import RecoveryPolicy
from shadow_agent.verification.verifier import VerificationResult, Verifier


def test_recovery_stops_after_retries():
    policy = RecoveryPolicy(max_retries=2)
    first = policy.note(VerificationResult(ok=False, reason="tests failed", evidence="boom"))
    assert "VERIFICATION FAILED" in first
    assert not policy.exhausted()
    second = policy.note(VerificationResult(ok=False, reason="still failing"))
    assert policy.exhausted()
    assert "2 times" in second


def test_verifier_rejects_unrun_hello(workspace):
    verdict = Verifier(workspace).verify("Create a Python hello-world project")
    assert not verdict.ok
