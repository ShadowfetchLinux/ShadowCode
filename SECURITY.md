# Security policy

## Supported versions and reporting

The latest release and `main` are supported. Report vulnerabilities through a
[private security advisory](https://github.com/Shadowfetchapps/ShadowCode/security/advisories/new).
Include the affected versions, the impact and steps to reproduce, and leave out
live credentials. There is no bug bounty program.

## What ShadowCode is not

**ShadowCode is not an operating-system sandbox.** Shell commands run as your
Linux user, with that user's files and network. If `bwrap` (bubblewrap) is
available, ShadowCode's own `exec` tool can wrap a command in a limited profile:
the project is writable, home and system directories are read-only, and the
network is off unless allowed. If bubblewrap is missing or user namespaces are
blocked, commands run unwrapped and Doctor reports it. Command classification
(privileged, network, destructive Git) is a lexical policy check. It is not
containment.

Vendor CLIs (Codex, Claude Code, Cursor, Antigravity, Grok) run as your user
with their own tools and sandboxes. ShadowCode doesn't wrap them in bubblewrap,
and its network and `sudo` rules don't apply to them. Use a container or a
separate account when you need stronger isolation.

## Vendor credentials

- **ShadowCode never reads vendor credentials.** Sign-in state comes from each
  vendor's documented status command or protocol: Codex app-server
  `account/read`, `claude auth status`, the ACP handshake for Cursor and Grok,
  `agy models`. ShadowCode never opens `~/.codex/auth.json`, Claude's
  credential files, the Grok or Cursor login files, or Antigravity's keyring
  entry.
- **Connect and Disconnect** run the vendor's own login and logout commands
  as supervised child processes. The vendor opens the browser or prints a URL
  or device code. ShadowCode only relays the printed lines after redaction, and
  never shows a URL that carries a code or token parameter as a link.
  Antigravity sign-in and sign-out happen only inside `agy`.
- **No API keys reach vendor CLIs.** Every vendor CLI that runs a task, and
  every login and logout, starts without `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`,
  `OPENAI_API_KEY`, `CODEX_API_KEY`, `CURSOR_API_KEY`, `XAI_API_KEY`,
  `GROK_API_KEY`, `GEMINI_API_KEY` and `GOOGLE_API_KEY`, so a subscription turn
  can't turn into a per-token API call. A CLI that is itself signed in with an
  API key is labelled *API key login · billed per token*.
- **Claude Code's terms.** Anthropic doesn't allow third-party clients to use
  Pro/Max OAuth tokens directly. ShadowCode only drives the official `claude`
  binary under your own login, which is tolerated but not guaranteed.
  `cli_agents.claude_enabled: false` turns the adapter off.

## Local model runtime

- **One server, one model.** The managed `llama-server` serves one model at a
  time and binds `127.0.0.1` on a free port.
- **Per-launch key.** Each launch gets a new random 32-byte key through the
  `LLAMA_API_KEY` environment variable. The key never appears in argv, is never
  written to disk, and ShadowCode's clients send it as a bearer token.
- **Isolated process.** The server's web UI is off (`--no-webui`). Its
  environment is cleared except for an allow-list. It runs in its own process
  group and dies with ShadowCode (`PR_SET_PDEATHSIG`). Clients connect directly
  and never through a proxy.
- **Weights are never modified.** ShadowCode never downloads weights and never
  deletes them when a row is removed. Ollama imports reference the store's
  blobs read-only.
- **Vetted runtime.** The runtime is resolved from fixed locations only (see
  [ARCHITECTURE.md](ARCHITECTURE.md#local-runtime-lifecycle)), never from a bare
  `PATH` lookup. The installer refuses a bundled runtime that has absolute or
  dangling symlinks, missing notices, or a `llama-server` that doesn't report
  its pinned commit.

## Web tools

`web_fetch` and `web_search` exist only for ShadowCode's own agent loop (local
models and OpenRouter models). They are offered only when you turn on Web for the task and the
network mode is *Online*.

- **Addresses.** Only http and https URLs without embedded credentials, on
  ports 80 and 443. DNS is resolved once, and every address is checked. The
  connection is pinned to the checked addresses, so a second DNS answer can't
  redirect it.
- **Blocked ranges.** Loopback, private, CGNAT (100.64/10), link-local
  (including cloud metadata 169.254.169.254), unspecified, multicast,
  broadcast, reserved and documentation ranges are refused, including IPv4
  addresses embedded in IPv6 forms. In 192.0.0.0/24 only the special-purpose
  hosts (DS-Lite .0–.7, PCP/TURN anycast .9–.10, NAT64 discovery .170–.171)
  are refused, because some VPN resolvers answer public names with other
  addresses in that block. Exact `host:port` entries in
  `network.allow_local_dev` may reach loopback or private dev servers.
  Link-local and metadata addresses stay blocked even then.
- **Redirects.** At most 5, followed by hand and re-checked each time. A
  redirect from https to http is refused.
- **Limits.** The client ignores proxies. Connect timeout 5 s, total 20 s,
  body cap 2 MB, and only allow-listed text content types.
- **Page content is data.** Every result is framed as data from the URL, not
  instructions. Search results that can't be retrieved are reported as
  `blocked`, never invented.
- **Search sources.** A configured SearXNG instance first, then DuckDuckGo's
  HTML page, then Marginalia Search's public API when DuckDuckGo refuses.
  ShadowCode names itself in its user agent and never works around a bot
  check.

## OpenRouter API key

The key is checked with OpenRouter's `GET /api/v1/key`, then stored in the
profile's `secrets.env` (mode 600) under `OPENROUTER_API_KEY`. It is never
returned to the window, written to logs or passed to vendor CLIs, and it is
sent only to `openrouter.ai`. Turns run on ShadowCode's own agent loop, so the
permission mode, approvals and checkpoints apply as they do for local models.
Offline mode sends nothing to OpenRouter.

## Permissions

| Mode | ShadowCode's own tools |
| --- | --- |
| Ask before actions | File edits and shell commands ask |
| Allow project edits | File edits inside the project run. Shell commands, deletes, Git staging and history changes, and background processes ask |

Both modes also follow these rules:

- Edits outside the project, including through symlinks, fail before any
  approval is asked.
- `sudo`, `su`, `pkexec`, `doas` and `run0` are denied unless `allow_root` is
  set, and then they still ask.
- Destructive Git commands in the shell ask. `git_reset` and `git_clean` need
  the elevated level.
- Network commands in the shell are denied unless network is enabled, and
  always denied offline.
- Plan and Review tasks are read-only.

What each vendor runtime enforces (from `permissions.rs`, shown in
**Settings › Permissions & network**):

| Vendor | Enforcement |
| --- | --- |
| Codex | Codex's own sandbox: `workspace-write`, or `read-only` for Plan/Review, with `approvalPolicy: on-request`. Codex decides which actions ask. The `codex exec` fallback has no approval channel. It is used only if app-server fails before a turn starts, and only when shell commands are set not to ask (`permissions.approve_shell: false`). With the defaults of both modes, it never runs |
| Claude Code | Permission prompts come to ShadowCode (`--permission-prompts host`). Plan/Review uses `--permission-mode plan`. Claude's own settings can pre-approve tools that ShadowCode never sees |
| Cursor | ACP permission requests come to ShadowCode. Plan/Review uses Cursor's plan mode when offered |
| Grok | ACP permission requests come to ShadowCode. Grok has no read-only mode, so Plan/Review is not enforced by Grok |
| Antigravity | Never asks ShadowCode. It applies its own settings (`~/.gemini/antigravity-cli/settings.json`) and soft-denies actions it may not run. Plan/Review uses `--mode plan` |

In read-only tasks, ShadowCode denies vendor approval requests automatically.
Unanswered vendor requests are denied after `approval_timeout_sec` (600 s by
default).

## Trust gate and consent

- **Trust.** Every task entry point (desktop, CLI, goals, MCP, workflows) goes
  through one engine check, and tasks are refused in a project you haven't
  trusted. Project instructions, skills, hooks, plugins and MCP definitions are
  executable or influential content: trust a project only if you trust that
  content.
- **Consent before cloud.** Nothing leaves the computer silently when a
  conversation moves to a cloud route. The turn needs your consent in three
  cases: the previous turn ran locally, the conversation moves to another
  provider (a bounded handoff of at most 12,000 characters), or images go to a
  cloud route for the first time in the conversation. Without consent, no job
  row is written and nothing is sent.
- **Offline mode.** Jobs on cloud routes are refused, and no vendor process is
  started for status, models or usage.

## Stored data and redaction

- **Before model context.** Common secret patterns (private keys, GitHub,
  Slack and AWS tokens, `sk-…` keys, JWTs, bearer tokens, `api_key=…`) and
  high-entropy tokens are replaced with `[redacted secret]` in file contents
  and tool output. Requests to read `.env`, `.env.*`, `secrets.env`, credential
  JSON or private keys are refused.
- **Stored events.** `tool.started`, `tool.completed`, `approval.requested`
  and `command.completed` payloads are written to SQLite in redacted form.
  Vendor CLI output is redacted before it is shown or stored.
- **Limits.** Redaction matches patterns only. It is not a guarantee.
- **The history database is sensitive.** `shadow-agent.db` holds prompts,
  answers and source excerpts. It is created with mode 600, and migration
  backups get the same mode.
- **Keys for HTTP providers** belong in `~/.config/shadow-agent/secrets.env`
  (mode 600) or in environment variables named in the config. Never put them in
  `config.yaml`.

## Local trust boundary

- **No network listener.** The desktop talks to the engine over Tauri IPC in
  the same process. The CLI uses a Unix socket in a private per-user directory
  (`/run/user/<uid>/shadowcode/`). Both peers check the user ID, protocol and
  profile, frames are bounded, and there is no TCP listener. Sockets left by
  dead engines are removed at startup. Processes running as the same user are
  trusted.
- **Private directories.** Profile config, data and state directories are
  created with mode 700 and must be owned by the current user. The profile lock
  is a private regular file. Symlinks, extra hard links and foreign owners are
  rejected.
- **Webview restrictions.** The webview only navigates to the app's own
  origin. Markdown from models renders without raw HTML or remote images. Only
  `http`/`https` links without embedded credentials stay clickable.
- **Optional MCP HTTP server.** `shadowcode mcp serve --http` binds loopback
  only, needs a bearer credential and rejects browser origins. See
  [docs/NATIVE_MCP.md](docs/NATIVE_MCP.md).
- **Plugins and hooks.** [Plugins](docs/NATIVE_PLUGINS.md) install without
  running anything. [Hooks](docs/NATIVE_HOOKS.md) need explicit activation
  pinned to the exact definition. Enabled commands run as your user.

## Recovery

- **No replay.** Crash recovery never replays shell commands, Git history
  changes or file edits. Interrupted jobs are marked `interrupted`.
- **Review applies only current hunks.** If the diff changed, you get a
  conflict instead of a stale patch.
- **Rewind covers only ShadowCode's own file tools.** It doesn't undo shell
  effects or vendor CLI edits.

## Releases

Check downloads against `SHA256SUMS`. Checksums catch damaged or mismatched
files; they are not signatures. The installer refuses an AppImage without a
matching entry unless you pass `--unverified`. Package notices and the bundled
llama.cpp licence texts are checked during packaging
([licenses/native](licenses/native/README.md)). ShadowCode sends no telemetry.
