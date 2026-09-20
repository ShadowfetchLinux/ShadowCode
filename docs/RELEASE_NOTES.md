ShadowCode 0.19.0 upgrades the complete Linux coding workspace: a persistent project/task sidebar, polished light and dark themes, Markdown conversations, saved drafts, live plans, a terminal panel, and staged/unstaged review.

Tasks now restore their workspace and conversation context, reconnect without duplicate output, stream past the former 800-event limit, and survive API restarts as explicit interrupted records. Cancellation waits for the worker to stop. Direct API edits enforce read-only permissions and guard against active tasks; browser access is restricted to the same origin.

Downloads:
- **AppImage:** bundled Python and desktop UI; run with `--appimage-extract-and-run` when FUSE is unavailable.
- **Portable archive:** extract and run `ShadowCode/shadowcode`.
- **Python wheel/source archive:** includes the compiled interface.
- **SHA256SUMS:** download-integrity checksums.

The Linux binaries target x86_64, Ubuntu 24.04+ / glibc 2.39+. Git and a browser are external requirements. The browser window uses Chromium/Chrome/Brave when available. User data remains in the existing `shadow-agent` XDG directories.

Install with `scripts/install-appimage.sh /path/to/ShadowCode-0.19.0-x86_64.AppImage` from the source checkout. It installs to `~/Applications`, updates the desktop launcher, and removes older ShadowCode AppImages after checking the new build. Finish any running tasks and restart the previous API before launching the upgraded app.

See the README, user guide, security policy, and changelog for usage and practical limits. The shell is a user-level process, not an OS sandbox; review model-generated changes and verification evidence.
