# Native MCP migration — development

MCP remains an open [native release gate](NATIVE_MIGRATION.md). The Rust stdio
client now connects to native Settings, CLI registration, project activation,
and individually approved agent tool calls. Streamable HTTP, built-in SQLite
compatibility, the ShadowCode MCP server, and broader interoperability/real-model
verification are still being migrated. Existing URL definitions remain visible
and inactive; this development build does not silently launch the Python client.

## Enable external tools

In **Settings → MCP**, register a server with its name and a JSON array containing
the executable and literal arguments, then review and enable it for the current
trusted project. Registration and catalog refresh do not execute the command.
Four servers may be enabled per project. A grant records the project's canonical
path, the definition ID, and its SHA-256 content hash.

Project definitions are direct `.yaml`, `.yml`, or `.json` files in
`.shadowcode/mcp/` or `.shadow/mcp/`. For example:

```json
{
  "name": "my-tools",
  "command": ["/absolute/path/to/mcp-server", "--stdio"],
  "description": "Tools for this project",
  "timeout_sec": 30,
  "env_refs": { "API_TOKEN": "MY_TOOLS_API_KEY" }
}
```

`env_refs` maps child environment variables to names in the protected application
secret store or the application's environment. Referenced values resolve only
when a connection starts. Legacy literal `env` entries remain supported, but
catalogs show only their variable names. Configured environment values are
redacted from returned tool metadata and results; stderr is not put in task
history. Avoid placing credentials in executable arguments, URLs, descriptions,
or task prompts. External servers run with the user's OS privileges; activation
is not an OS sandbox or a guarantee of network confinement.

The same controls are available without a display:

```sh
shadowcode mcp add /path/to/definition.json
shadowcode --json mcp
shadowcode mcp enable config:my-tools --hash HASH_FROM_CATALOG
shadowcode mcp disable config:my-tools
shadowcode mcp remove config:my-tools --hash HASH_FROM_CATALOG
```

Project IDs use `project:.shadowcode/mcp/my-tools.yaml`. Global IDs use
`config:my-tools`. Replacing a global definition with `mcp add` requires its
current `--hash`. Removing it also removes its project grants, so re-registering
identical content does not reactivate it. Project overlays cannot grant access.
Changed or missing project definitions must be reviewed or disabled before a
new Build task uses them.

Build tasks with enabled servers receive two compact tools: `mcp_tools` discovers
servers, lists tool names in pages of 20, or reads one exact tool schema;
`mcp_call` requests a call to a server/tool with an argument object. Initialization
is lazy and uses the explicit launch grant. Every `mcp_call` requires a separate
session-scoped approval, even if the peer marks its tool read-only. The server
ID, tool name, and complete arguments appear in that approval. Plan, Review,
and untrusted projects cannot start MCP processes.

Running and queued tasks retain their configuration snapshot. Disabling a grant
or replacing a global registration applies to new tasks; cancel existing tasks
to stop their current connections. Project files are re-read and their hash
checked before each external action, including after an approval wait.
Definitions cannot replace their command while an earlier approval is pending.
MCP actions invalidate earlier file observations and disable parallel native
reads for that task. Changes made by an external server are not checkpointed by
ShadowCode's file tools.

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
using it; the application runner supplies those controls. A server's
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
A closed connection cannot return its old catalog as if it were still live;
start a new task to reconnect after a protocol failure.

## Ownership and cancellation

Each connection owns its subprocess group and bounded stderr drain. Cancellation
also works while initialization is pending or the connection is idle. Cleanup
sends TERM, allows 150 ms, then kills the group and waits for the leader. The
leader remains unreaped until group signaling finishes, preventing PID reuse
from redirecting a late signal. Detached pipes have a bounded drain period.
Processes that deliberately escape their group are outside this user-level
cleanup boundary.

`Client::close` waits for cleanup. Dropping a client or abandoning its connection
future immediately signals its group to stop; the cleanup worker reaps it.
The application task loop awaits all connections before reporting completion or
releasing the workspace, including provider errors, step limits, cancellation,
and shutdown. Connections are scoped to one task and reused within that task.
Durable `mcp.connected` and `mcp.closed` events retain the definition ID/hash
and cleanup outcome without environment values.

## Verification

```sh
cargo test -p shadowcode-core --test mcp_stdio --test mcp_application --locked
```

Ten regressions exercise real subprocess pipes, current and legacy protocol
negotiation, paginated discovery, exact arguments/environment, tool error results,
malformed/oversized/truncated initialization, duplicate names, pagination loops,
catalog limits, command timeouts, cancellation, idle/drop/abandoned initialization,
stubborn descendant cleanup, leader reaping, stderr floods, protocol floods, and
refusal of unsolicited sampling/credential requests. The small Node peer is a
test fixture; no Node or Python runtime is added to the application.

Eight application regressions cover inert/symlink-confined discovery, malformed
definitions, hidden credentials, project trust and permissions, definition
hashes, removal/re-registration, exact argument approval and denial, connection
reuse, error-result preservation, bounded single-pass credential redaction,
closed-catalog rejection, stale file observations, read-only tasks, and
process cleanup before actual engine completion on every exit path. The native
CLI and real-window probes also exercise their respective controls; the window
probe drives a scripted model through discovery, approval, a real subprocess
tool result, and cleanup.

These checks do not stand in for the remaining HTTP, server, interoperability,
and real local-model checks required for the full MCP migration. The standalone
application does not bundle runtimes for third-party servers: install whatever
an explicitly selected external command requires separately.
