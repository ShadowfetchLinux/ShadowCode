# Native isolated worktrees — development

The native engine can create a Git worktree on a separate `shadowcode/<ID>` branch
from an existing local commit or branch. Staged, unstaged and untracked changes
remain in the source checkout. The new checkout starts from the selected commit.

```sh
shadowcode --workspace /path/to/repository worktree
shadowcode --workspace /path/to/repository worktree --create
shadowcode --workspace /path/to/repository worktree --create --reference main
```

In the native desktop, open **Settings → Worktrees**. The panel shows the source
project and managed checkouts, creates from a chosen local reference, and opens a
checkout through the normal trust flow. **Inspect removal** shows its exact path,
branch, commit and status before enabling **Remove clean worktree**. Dirty reviews
remain blocked; an error clears the review so a fresh inspection is required.
Actions include the displayed source project, and the engine rejects a changed
selection. The panel supports keyboard focus and light/dark/compact layouts.

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
The active inventory currently permits at most 64 records. Successful removal
archives its record under `records/archive`, freeing an active slot. Git's normal worktree and branch
commands remain available for inspecting the repository metadata.

## Inspect and remove a clean checkout

```sh
shadowcode --workspace /path/to/repository worktree --inspect FULL_WORKTREE_ID
shadowcode --workspace /path/to/repository worktree --remove FULL_WORKTREE_ID --hash REVIEW_HASH
```

Inspection returns the exact path, current branch and commit, status including
ignored files, eligibility and a review hash. Removal requires that current hash
and checks the Git repository identity and registration again. It refuses dirty,
untracked or ignored files, detached HEADs, locked worktrees, active tasks/manual
operations and background processes. A reservation prevents a new managed task
or background process from starting during removal. Git performs its own final
checks; removal never uses `--force` or deletes branches. Unmerged commits remain
on the preserved branch. Source files and selection remain unchanged.

Files deliberately changed by external programs should be preserved before
removal, just as with ordinary Git operations. Damaged checkout paths or missing Git registrations still require manual
inspection; the recovery record remains. For a missing path whose Git
registration survives, use the reviewed rescue below.

Carrying uncommitted changes into an isolated checkout, reviewed return/merge
operations and recovery controls for missing/damaged checkouts remain part of the
[native migration gates](NATIVE_MIGRATION.md).

Inventory and individual actions share the same bounded, non-following record
reader. A record whose filename, managed path or branch identity has changed is
rejected before it can be presented as a usable worktree.

## Rescue a missing checkout's committed work

```sh
shadowcode --workspace /path/to/repository worktree --recovery FULL_WORKTREE_ID
shadowcode --workspace /path/to/repository worktree --restore FULL_WORKTREE_ID --recovery-hash REVIEW_HASH
```

The review resolves the missing checkout's registered HEAD, including commits
not merged into the source project. Restoration creates a **new** managed branch
and checkout from that exact commit. It does not reconstruct missing uncommitted
files. The original branch, registration, index and recovery record remain
untouched so staged changes or a moved checkout can still be recovered manually.
No global Git prune, forced removal or branch reset is performed.

An existing path (including a symlink), locked registration, changed repository,
missing commit, unavailable registration or stale review blocks rescue. A locked
worktree may be on an unavailable device; inspect its location before unlocking.
Trust and writable/idle source requirements also apply. Open and trust the new
checkout explicitly. The original record remains in the active inventory until
its underlying checkout is repaired and reviewed for removal. Dedicated desktop
rescue controls and automatic repair of damaged paths remain open.
