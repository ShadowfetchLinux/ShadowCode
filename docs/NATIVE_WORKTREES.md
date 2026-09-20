# Native isolated worktrees — development foundation

The native engine can create a Git worktree on a separate `shadowcode/<ID>` branch
from an existing local commit or branch. Staged, unstaged and untracked changes
remain in the source checkout. The new checkout starts from the selected commit.

```sh
shadowcode --workspace /path/to/repository worktree
shadowcode --workspace /path/to/repository worktree --create
shadowcode --workspace /path/to/repository worktree --create --reference main
```

The command returns the new path, branch, base commit and recovery-record ID.
Open that path through the existing project controls, or use it as `--workspace`.
Review and trust the new project before running tasks there:

```sh
shadowcode --workspace /returned/worktree/path trust
shadowcode --workspace /returned/worktree/path run "Implement and test the change"
```

Creation requires a trusted, writable repository-root workspace with no active
task or manual operation in that workspace. The current desktop/CLI project stays
selected. References resolve to one local commit before checkout; no fetch or
remote branch publication is performed. Git checkout hooks and filesystem-monitor
hooks are disabled. Git's configured checkout filters still apply, as they do for
ordinary Git checkout operations.

Private records and checkouts live under the profile's
`data/managed-worktrees/records` and `data/managed-worktrees/checkouts` directories.
A recovery record is saved before Git starts creating the checkout. An interrupted
or failed operation is recorded as `needs_attention`; an abruptly killed process
may leave `creating`. Partial files and branches are retained for inspection.
The inventory currently permits at most 64 records. Git's normal worktree and branch
commands remain available for inspecting the repository metadata.

This is the creation foundation, not the completed worktree workflow. Carrying
uncommitted changes into an isolated checkout, reviewed return/merge operations,
safe removal and recovery controls, and dedicated desktop controls remain part
of the [native migration gates](NATIVE_MIGRATION.md).
