# Security policy

## Supported versions and reporting

The latest stable release and `main` are supported. Please use a
[private security advisory](https://github.com/ShadowfetchLinux/ShadowCode/security/advisories/new)
for vulnerabilities. Include affected versions, impact, and reproduction steps
without live credentials. There is no bug bounty program.

## Local trust boundary

Native 0.20 development uses embedded Tauri IPC for the desktop and a private
Unix socket for CLI clients. The socket directory must be owned by the OS user
and private; both peers check user credentials, protocol and profile identity.
Requests use bounded frames and independent project/session selection. It opens
no TCP listener. See [native CLI lifecycle and trust](docs/NATIVE_CLI.md).
These boundaries trust processes running as the same user. The full
[native release gates](docs/NATIVE_MIGRATION.md) remain in progress.

Native profile config, data, and state directories are created with mode 700;
existing application directories are restricted to that mode without deleting
their contents or changing permissions on an existing parent directory. Each
must be owned by the current account and must not itself be a symlink. A
relocated XDG base or `--profile` parent may be a symlink. The native profile lock
is a private regular file with mode 600; symlinks, additional hard links, foreign
owners and special files are rejected before acquiring it.

Native configuration and secret writes serialize the full read–modify–write
operation across threads in the owning engine; another native manager cannot
open the same locked profile. This prevents unrelated concurrent changes from
being overwritten. Reads require regular files and consume at most 1 MB plus a
limit-check byte. Writes reject oversized results before replacing saved data.
This coordination does not lock out an external text editor or another program
running as the same account.

[Native project plugins](docs/NATIVE_PLUGINS.md) install validated declarative
bundles into namespaced project files. Installation does not execute scripts,
install dependencies or activate hooks/MCP. Executable integrations require
separate content-bound approval. Private install journals determine which
unchanged files may be removed; edited files and legacy bundles are preserved.
Plugin text can influence a selected task and is subject to the same project
trust and tool permissions as other workflow instructions.

[Native lifecycle hooks](docs/NATIVE_HOOKS.md) require explicit activation for a
trusted workspace and exact definition hash. Discovery never imports repository
code. The approval pins the command definition, not the scripts or dependencies
it invokes. Enabled commands run as the user, inherit their environment, and can
have effects outside the project; lexical root/network checks are not a sandbox.
Changed manifests fail closed. Read-only tasks keep hooks inactive. Running and
queued tasks keep their configuration snapshot; cancellation stops active hook
processes. Hook shell edits are outside file-tool checkpoints. Python callbacks
remain inactive until converted and reviewed.

The supported 0.19 desktop API defaults to `127.0.0.1:7430`. It validates loopback Host values,
rejects cross-origin and cross-site browser requests, and sends framing, MIME,
referrer, and content-security headers. Do not expose it to a network, reverse
proxy it to the public internet, or run it under a shared untrusted account.
Local processes running as your user are trusted and can call the API.

The legacy 0.19 MCP HTTP transport has its own optional bearer token, stored
in `~/.config/shadow-agent/mcp-token`. The [native MCP HTTP gateway](docs/NATIVE_MCP.md)
is explicitly started, binds only to loopback and requires a bearer credential
reference. It validates Host, rejects browser Origins and bounds requests and
connections. Stdio MCP inherits the launching client's trust. Both native
transports pin a project and own their submitted jobs; browser API protections
do not replace MCP authentication.

## Files and command execution

Filesystem tools resolve paths within the chosen workspace. Direct API writes
honor read-only mode; desktop workspace mutations are blocked while an agent
job is active there. Dangerous operations use the harness permission policy and
approval flow.

**The terminal is not an OS sandbox.** Commands execute as your Linux user with
that user's filesystem and network capabilities. When `bwrap` (bubblewrap) is
installed, `exec` may wrap the command in a limited profile: workspace bind,
read-only home/system roots, and network off unless permissions allow network.
If bubblewrap is missing or user namespaces are blocked (common on some Pop!_OS
setups), ShadowCode falls back to the unsandboxed shell and Doctor reports that
clearly — it does not pretend isolation. Command classification remains a
heuristic control, not a containment guarantee. Workspace hooks, project
instructions, MCP servers, and installed plugins are executable or influential
project content. Trust a project only if you trust that content. Use a container
or a separate account when stronger isolation is required.

Review applies only current unstaged hunks. A changed diff causes a conflict,
rather than applying a stale client patch. File restores can overwrite later
edits; review the checkpoint and retain independent version-control backups.

[Native SQLite inspection](docs/NATIVE_SQLITE.md) opens existing project
databases read-only and authorizes only queries returning rows. SQL writes,
ATTACH, configuration PRAGMAs and extension loading are denied; a small explicit
allowlist supports read-only schema PRAGMAs. SQLite may maintain its WAL
coordination sidecars. Directory capabilities confine database/sidecar paths;
query work, concurrent readers and output are bounded. Cancellation interrupts
SQLite and releases its connection; an OS filesystem stall may delay return.

## Local GGUF inference

ShadowCode can spawn a managed `llama-server` on 127.0.0.1 with one user-owned
GGUF at a time. It does not start Ollama or LM Studio and does not download
weights. Removing a catalog row deletes only the pointer, never the file. The
managed binary lives under `~/.local/lib/shadowcode/` after install.

## Vendor subscription CLIs

Codex, Claude Code, Cursor, and Antigravity run as official local CLIs.
ShadowCode does not scrape cookies, extract OAuth tokens, or treat a Gemini API
key as Antigravity subscription access. Disconnecting ShadowCode does not always
log out a shared native CLI — use that vendor's own logout when you want the
CLI session gone. Image attachments are forwarded only on official vendor fields
(Codex `localImage`, Claude image source blocks, Cursor ACP `image`). Antigravity
rejects images.

## Secrets and transcript content

Keys belong in `~/.config/shadow-agent/secrets.env` with mode 600 (recommended on this
machine; libsecret/secret-service was not required for day-to-day use and is not
wired as a hard dependency), or environment
variables named in config. Never commit keys, tokens, sessions, or personal
workspace data. The installer does not source the secrets file as shell code.

Before file contents or tool output enter model context, ShadowCode scans for
common secret patterns and high-entropy tokens and replaces matches with
`[redacted secret]`. Raw `.env` / `secrets.env` / credential JSON contents are
refused rather than sent to the model. This is deliberate redaction for model
context, not TPM or keyring migration, and does not rewrite shell commands.

Local databases and drafts can contain prompts, tool output, and source code.
Protect the user account and filesystem accordingly. The configured model
provider receives the task context sent by the harness. Cloud providers may
therefore receive project content; local providers keep inference on the machine.

Vendor CLI backends (Claude, Codex, Grok) spawn the official CLI after the user
logs in with that vendor. ShadowCode holds no vendor credentials and never
reads `~/.codex/auth.json`, `~/.grok/auth.json`, or Claude credential files.
Anthropic forbids third-party clients from using Pro/Max OAuth tokens directly
(enforced 2026); driving the official `claude` binary with the user's own login
is currently tolerated but not guaranteed. See
[vendor CLI backends](docs/NATIVE_CLI_BACKENDS.md).

The UI renders Markdown without executing raw HTML. It does not automatically
load remote images embedded in model responses. Only fragment links and
absolute `http`/`https` URLs without embedded credentials stay clickable;
`javascript:`, `data:`, `file:`, and relative hrefs render as text. Allowed
external links open with `noopener noreferrer`.

Crash recovery does not auto-replay shell, Git history changes, or file
mutations. Those classes require confirmation or human review. Incomplete
tool groups are closed with a class-specific unknown-result record; shell
is never treated as safe to replay. Engine-process reattach reconnects the
desktop view only: it does not start jobs or retry mutating commands.
Worktree repair requires the recorded real directory and does not guess a
relocated checkout. Model prose is not treated as verified correctness.
Doctor metrics stay on the local machine; there is no product telemetry.

## Releases

Verify release assets against `SHA256SUMS`. Checksums detect mismatched or damaged
downloads; they are not independent signatures. The 0.19 AppImage bundles Python and
its dependencies, so security updates require installing a new build. Source
installations use a project virtual environment. Neither installer deletes keys,
configuration, or saved task history.

Native development packages contain Rust application code, embedded UI assets,
and native libraries; their dependency inventories and notices are checked during
packaging. They have not yet replaced the supported release download.
