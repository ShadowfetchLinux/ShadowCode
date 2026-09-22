ShadowCode 0.22.3 improves native Linux coding workflows, project isolation and recovery.
The Rust/Tauri app embeds its interface and needs no Python or Node runtime.

- **Conversation control:** fork from a response with completed tool context;
  later turns stay out of the fork and the original remains intact.
- **Safe recovery:** pause, steer and resume tasks; checkpoint rewind waits for
  a safe boundary and refuses competing workspace activity.
  Attached windows retain events after engine replacement and can close during
  reconnection without waiting for a new engine.
- **Local model tuning:** visible context and Ollama residency controls, with
  validated settings preserved when selecting saved models.
- **Parallel workspaces:** prepare up to two branches, open explicit tasks,
  check the combined merge result and remove clean checkouts while retaining
  commits. No automatic worker dispatch or integration is implied.
- **Code intelligence:** private bounded AST cache, confined file access,
  Unicode-safe signatures, stale-entry removal and syntactic call-site queries.
- **Execution hardening:** owned scratch directories, correct project mount
  order, early bubblewrap probing and no replay of failed shell executions.
- **Guardian:** optional diagnostics and properly scoped proposal approval.
  Diagnostics identify test commands but do not run tests or generate patches.
- **Linux launch reliability:** preserve the caller directory and relative project
  paths; keep external Python tools usable and both installed CLI names in sync.
- **Reproducible release checks:** clean hosts build the real application before
  process tests, and packaging retains Rustup while excluding unsafe PATH entries.

Downloads: `ShadowCode_0.22.3_amd64.AppImage`,
`ShadowCode_0.22.3_amd64.deb`, the AppImage runtime source archive, and `SHA256SUMS`.
Binaries target x86_64, Ubuntu 24.04+ / glibc 2.39+. Git and model providers such
as Ollama remain external.

Keep SHA256SUMS beside the AppImage and run
`scripts/install-appimage.sh /path/to/ShadowCode_0.22.3_amd64.AppImage` to verify
and install it in `~/Applications`. Existing settings, keys and task history
are preserved. Extraction mode works without FUSE.

See `docs/SHADOWCODE_022_QUALIFICATION_REPORT.md` for tested scope and limitations.
