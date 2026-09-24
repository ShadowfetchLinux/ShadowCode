# Optional shell isolation

> **Reference.** This covers ShadowCode's own `exec` tool. For the everyday workflow see the [user guide](USER_GUIDE.md), [subscriptions](SUBSCRIPTIONS.md) and [local models](LOCAL_MODELS.md).

Before an approved shell command runs, ShadowCode probes bubblewrap when it is
installed. The profile makes system directories and `/home` read-only, then
binds the selected project read-write. Network namespaces are isolated unless
network permission is enabled. A private temporary directory is mounted at
`/shadowcode-scratch` and exposed as `SHADOWCODE_SCRATCH`.

The scratch directory is removed when the command returns. Cleanup accepts only
scratch directories created and retained by this process; it cannot delete an
arbitrary supplied path. It is ordinary temporary storage, **not copy-on-write**.
Approved commands still modify the live project. File checkpoints do not cover
arbitrary shell or Git side effects.

If the initial probe fails, execution uses the ordinary approved shell path and
reports that fallback. Once execution begins, errors are returned without
replaying the command outside bubblewrap. Doctor reports the probe result.

This optional profile is not a complete operating-system security boundary.
Commands run as your account. Review approvals and resulting changes.
