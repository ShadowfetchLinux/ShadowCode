# Security policy

## Supported versions and reporting

The latest stable release and `main` are supported. Please use a
[private security advisory](https://github.com/ShadowfetchLinux/ShadowCode/security/advisories/new)
for vulnerabilities. Include affected versions, impact, and reproduction steps
without live credentials. There is no bug bounty program.

## Local trust boundary

The desktop API defaults to `127.0.0.1:7430`. It validates loopback Host values,
rejects cross-origin and cross-site browser requests, and sends framing, MIME,
referrer, and content-security headers. Do not expose it to a network, reverse
proxy it to the public internet, or run it under a shared untrusted account.
Local processes running as your user are trusted and can call the API.

The MCP HTTP transport has its own optional bearer token, stored in
`~/.config/shadow-agent/mcp-token`. Stdio MCP inherits the launching client's
trust. Browser API protections do not replace MCP authentication.

## Files and command execution

Filesystem tools resolve paths within the chosen workspace. Direct API writes
honor read-only mode; desktop workspace mutations are blocked while an agent
job is active there. Dangerous operations use the harness permission policy and
approval flow.

**The terminal is not an OS sandbox.** Commands execute as your Linux user with
that user's filesystem and network capabilities. Command classification is a
heuristic control, not a containment guarantee. Workspace hooks, project
instructions, MCP servers, and installed plugins are executable or influential
project content. Trust a project only if you trust that content. Use a container
or a separate account when stronger isolation is required.

Review applies only current unstaged hunks. A changed diff causes a conflict,
rather than applying a stale client patch. File restores can overwrite later
edits; review the checkpoint and retain independent version-control backups.

## Secrets and transcript content

Keys belong in `~/.config/shadow-agent/secrets.env` with mode 600, or environment
variables named in config. Never commit keys, tokens, sessions, or personal
workspace data. The installer does not source the secrets file as shell code.

Local databases and drafts can contain prompts, tool output, and source code.
Protect the user account and filesystem accordingly. The configured model
provider receives the task context sent by the harness. Cloud providers may
therefore receive project content; local providers keep inference on the machine.

The UI renders Markdown without executing raw HTML. It does not automatically
load remote images embedded in model responses. External links open with
`noopener noreferrer`.

## Releases

Verify release assets against `SHA256SUMS`. Checksums detect mismatched or damaged
downloads; they are not independent signatures. The AppImage bundles Python and
its dependencies, so security updates require installing a new build. Source
installations use a project virtual environment. Neither installer deletes keys,
configuration, or saved task history.
