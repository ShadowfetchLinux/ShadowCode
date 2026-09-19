# Security Policy

## Supported versions

The `main` branch is the supported line. Please upgrade before reporting
an issue that may already be fixed.

## Reporting a vulnerability

**Do not open a public issue for security reports.**

Email the maintainers through GitHub: open a
[private security advisory](https://github.com/ShadowfetchLinux/ShadowCode/security/advisories/new)
on this repository.

Include:

- A description of the issue and its impact
- Steps or a proof of concept (no live credentials)
- Affected version / commit

You will receive an acknowledgement when the report is seen. There is no
bug bounty program.

## Secrets

API keys belong in `~/.config/shadow-agent/secrets.env` (mode 600), never
in YAML, git, or issue text. The MCP bearer token lives in
`~/.config/shadow-agent/mcp-token`.
