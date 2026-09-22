# Optional bubblewrap sandbox

Shell/exec may run under `bwrap` when present:

- Workspace is bind-mounted read-write.
- `/home` and `/root` are read-only (never writable).
- Network is off unless permissions allow it.
- An ephemeral scratch upper dir is bind-mounted at `/shadowcode-scratch`
  (`SHADOWCODE_SCRATCH`). Discard it via the discard-scratch API when done.

This is **not** a full OS sandbox, not Landlock/ZFS/whole-disk OverlayFS, and
not kernel-proof. If user namespaces block bwrap, Doctor reports the fallback
and shell continues with the ordinary heuristic policy.
