# Guardian diagnostics

Guardian is off by default. Enable it and save an interval of 60–86,400 seconds
in Settings → Advanced. Run diagnostics manually there; scheduled checks run
while `shadowcode serve` is running. The desktop alone does not start a scheduler.

Checks inspect local isolation support and identify a suggested test command.
They do not run tests, edit code, push, open a PR or merge. Last-run results and
pending proposals are scoped to the current engine profile and workspace.
Results are visible in Advanced; desktop notifications are not implemented.

The optional proposal API requires both `enabled` and `allow_prepare_patch`.
`POST /api/guardian/request-patch` with `summary` returns an immutable
`approval_id`. Passing that exact ID to `POST /api/guardian/approve-patch` saves
`PROPOSAL.md` outside the project. The ID is bound to the profile, workspace and
original summary and can be used once. Despite the compatibility endpoint name,
this saves a **proposal draft**, not a generated patch or Git worktree.
