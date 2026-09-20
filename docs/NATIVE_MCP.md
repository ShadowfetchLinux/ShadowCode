# Native MCP migration — development

MCP remains an open [native release gate](NATIVE_MIGRATION.md). The Rust stdio
client foundation is implemented and tested. Application registration,
workspace/content activation, agent approvals, Settings/CLI integration,
Streamable HTTP, built-in SQLite compatibility, and the ShadowCode MCP server
still need their native implementation and verification. The stdio module alone
does not enable MCP tools in the application.

## Stdio client foundation

`native/core/src/mcp.rs` uses the official Rust SDK, pinned to `rmcp` 3.4.0.
Its [upstream source](https://github.com/modelcontextprotocol/rust-sdk/tree/fd7811fdaa9fefa1c8034534b4d7a31c97204f89)
supplies negotiation, request correlation, capabilities and protocol models.
ShadowCode supplies a bounded newline transport and process ownership.
The omitted crate license is retained from that same commit in
`licenses/native/rmcp-3.4.0/`, with a checked digest in the notice manifest.

The low-level `Client::connect` API starts only the literal command supplied by
its caller, in a canonical project directory. It does not discover or activate
servers. Callers must obtain server activation and exact tool approval before
using it; those application controls are the next integration step. A server's
tool annotations, descriptions or instructions cannot supply approval.

The child receives basic path/home/locale/XDG environment variables and explicitly
provided entries. Provider credentials, application control variables and dynamic
library overrides are not inherited by default. Environment values and arguments
are passed literally, without shell expansion. An explicitly configured shell
still executes its own script; this is a user-level process boundary.

Transport and catalog limits:

- 16 simultaneous connections per process; permits last through cleanup.
- 1 MiB per JSON-RPC frame, 32 MiB received over a connection's lifetime, and
  128 incoming frames per one-second window, including blank lines.
- 512 KiB of tool arguments, 128 catalog tools, 2 MiB of catalog data, eight
  pages, and 4 KiB pagination cursors. Duplicate tools and repeated cursors fail.
- A 0.1–120-second caller-selected timeout for initialization, discovery and
  each tool call. All discovery pages share one deadline.
- A 16 KiB stderr capture that continues draining after its cap. It is separate
  from protocol errors and requires redaction before application display/storage.

Malformed, oversized or incomplete frames fail the connection. Response caching
is disabled so discovery cannot silently return stale tools after an error.
Remote `isError`, structured data and content blocks are preserved; errors are
not converted into successful tool output. Unknown local tool names and invalid
arguments are rejected before sending a call.

The client advertises no roots, sampling, elicitation or task capabilities.
Unsolicited sampling fails and elicitation is declined. Interactive or background
task continuations returned from a tool are rejected rather than automatically
supplying inputs or starting an unattended polling loop. A failed call does not
prove that the remote tool made no changes; the client never retries it silently.

## Ownership and cancellation

Each connection owns its subprocess group and bounded stderr drain. Cancellation
also works while initialization is pending or the connection is idle. Cleanup
sends TERM, allows 150 ms, then kills the group and waits for the leader. The
leader remains unreaped until group signaling finishes, preventing PID reuse
from redirecting a late signal. Detached pipes have a bounded drain period.
Processes that deliberately escape their group are outside this user-level
cleanup boundary.

`Client::close` waits for cleanup. Dropping a client or abandoning its connection
future triggers cleanup asynchronously; application task integration must await
close before reporting task completion or releasing its workspace reservation.

## Verification

```sh
cargo test -p shadowcode-core --test mcp_stdio --locked
```

Ten regressions exercise real subprocess pipes, current and legacy protocol
negotiation, paginated discovery, exact arguments/environment, tool error results,
malformed/oversized/truncated initialization, duplicate names, pagination loops,
catalog limits, command timeouts, cancellation, idle/drop/abandoned initialization,
stubborn descendant cleanup, leader reaping, stderr floods, protocol floods, and
refusal of unsolicited sampling/credential requests. The small Node peer is a
test fixture; no Node or Python runtime is added to the application.

These checks validate the client foundation. They do not stand in for the
remaining application permission, UI, HTTP, server, interoperability and real
local-model checks required for the full MCP migration.
