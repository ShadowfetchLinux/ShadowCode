# Guardian (optional, approval-gated)

`shadowcode serve` may run a scheduled **read-only** health check when enabled
in Settings → Advanced. **Default OFF.**

- Checks Doctor/sandbox probes and test hints without writing the main tree.
- May open a worktree and prepare a patch **only after explicit approval**.
- Never pushes, opens a PR, or notify-and-merges while idle.
- Desktop notification via `notify-send` when available; otherwise logs.
