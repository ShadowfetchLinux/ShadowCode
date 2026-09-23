# Vendor CLI agent backends

ShadowCode can run a coding task by spawning the official Claude, Codex, or
Grok CLI in a trusted workspace. In this mode the **vendor agent** owns the
loop, its tools, and its sandbox. ShadowCode is the workspace, transcript,
review, approval, and steering shell.

This is distinct from a **local model (ShadowCode agent)**, where ShadowCode
calls an OpenAI-compatible or Ollama HTTP API and injects its own tools.

ShadowCode **never** reads, stores, proxies, or re-implements vendor OAuth
tokens. Login is always:

| Vendor | Login |
| --- | --- |
| Codex | `codex login` |
| Grok | `grok login` |
| Claude | `claude auth login` |

Doctor only checks that the binary is on `PATH` and that a login *marker*
exists (`~/.codex/auth.json`, `~/.grok/auth.json`, `~/.claude/.credentials.json`,
or `claude auth status --json` → `loggedIn`). Credential contents are never
opened, logged, or shown.

## Adapters

### Codex

Primary: `codex app-server` — JSON-RPC 2.0 over stdio (`initialize` →
`thread/start` → `turn/start`, streamed `item/*` notifications, approval
requests such as `item/commandExecution/requestApproval`).

Fallback: `codex exec --json` only when `app-server` is unavailable (unknown
subcommand or missing help). Exec has no approval channel.

References: [OpenAI Codex docs](https://developers.openai.com/codex),
[openai/codex](https://github.com/openai/codex), and the T3 Code provider
notes at
[docs/internals/providers.md](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md).

### Grok

`grok agent stdio` speaks the [Agent Client Protocol](https://agentclientprotocol.com)
(JSON-RPC over stdio). ShadowCode declares no `fs` / `terminal` client
capabilities, so Grok performs its own I/O. See [xAI build docs](https://docs.x.ai/build).

### Claude

`claude -p --output-format stream-json --input-format stream-json --verbose --include-partial-messages --permission-prompts host`
([Claude Code headless](https://code.claude.com/docs/en/headless)).
Permission prompts become ShadowCode Allow/Deny cards.

**Policy:** Anthropic forbids third-party clients from using Pro/Max OAuth
tokens directly (enforced 2026). Driving the official `claude` binary with the
user's own login is currently tolerated but not guaranteed. Settings → Advanced
has `cli_agents.claude_enabled` to disable the adapter.

## What ShadowCode does and does not do

- Maps vendor text, tool start/finish, file-change reports, results, and errors
  into the existing transcript.
- Routes vendor permission prompts through the existing Allow/Deny flow.
- Cancels by killing the child **process group**.
- Applies the same redaction used for ShadowCode tool output.
- Does **not** inject ShadowCode tools or bubblewrap into the vendor process.
- **Pause / Steer** = interrupt the vendor turn (where the protocol supports it)
  and send a follow-up prompt on Resume.
- **Rewind does not apply.** Vendor CLIs write files with their own tools;
  ShadowCode file-tool checkpoints do not cover those edits. Use Git or the
  vendor CLI to undo.

## Doctor states

Each vendor reports one of:

- `not_installed` — binary not on `PATH`
- `not_logged_in` — installed, no login marker
- `ready` — installed and login detected
- `disabled` — turned off in Settings → Advanced

## Settings

Knobs live only in **Settings → Advanced**:

- Enable vendor CLI backends
- Enable Claude adapter
- Binary names/paths
- Approval and stall timeouts

The composer model picker lists vendor backends under
**Claude / Codex / Grok (vendor agent)** and local HTTP models under
**Local model (ShadowCode agent)**. A Doctor status chip shows the three
install/login states.
