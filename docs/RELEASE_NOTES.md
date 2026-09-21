ShadowCode 0.20 is a native Linux application for local and compatible coding models. The desktop, terminal interface, CLI, and MCP server use the same Rust engine. The desktop embeds its interface and communicates through Tauri IPC; it does not bundle Python, Node.js, or a browser launcher.

It includes local/Ollama and compatible model routing, streamed tasks, explicit tool approvals, Plan/Build/Review modes, durable task history, project instructions and skills, checkpoints and rewind, workspace queues, goals, managed background processes, reviewed worktree workflows, and MCP client and server support. A desktop window can attach to a running `shadowcode serve` or TUI engine and leave its work running when the window closes.

Downloads:
- **AppImage:** `ShadowCode_0.20.0_amd64.AppImage` is the desktop application. Run it directly, or add `--appimage-extract-and-run` when FUSE is unavailable.
- **Debian package:** `shadowcode_0.20.0_amd64.deb` installs the same native app on supported Debian/Ubuntu systems.
- **Runtime source archive:** retained source and build records for the bundled AppImage runtime.
- **SHA256SUMS:** download-integrity checksums for every release artifact.

The Linux binaries target x86_64, Ubuntu 24.04+ / glibc 2.39+. Git and model providers such as Ollama remain external. User data remains in the existing `shadow-agent` XDG directories.

Verify `SHA256SUMS` before installing. Keep it beside the downloaded AppImage so `scripts/install-appimage.sh /path/to/ShadowCode_0.20.0_amd64.AppImage` verifies the matching SHA-256 entry automatically. The installer checks the application version, installs to `~/Applications`, updates the desktop launcher, and removes older ShadowCode AppImages only after the new build is ready. Existing profile data is preserved.

Release publishing runs native Rust, UI, terminal, MCP, stress, real-window, package inspection, runtime-rebuild, and packaged-app checks before uploading artifacts. The shell is a user-level process, not an OS sandbox; review model-generated changes and verification evidence.
