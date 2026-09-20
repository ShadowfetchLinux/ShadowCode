# Native MCP migration — development

MCP remains an open [native release gate](NATIVE_MIGRATION.md). The Rust stdio and Streamable HTTP
clients now connect to native Settings, CLI registration, project activation,
and individually approved agent tool calls. The native stdio server now exposes
tasks, goals, reviews, memory, SQLite inspection and checkpoints to external clients.
The remaining server features and broader interoperability/real-model
verification are still being migrated. Neither direction launches Python.

## Connect another coding tool to ShadowCode

The native executable can serve a single MCP client without a display:

```sh
shadowcode --workspace /absolute/project mcp serve
shadowcode --workspace /absolute/project mcp register
```

`mcp register` prints generic `mcpServers` JSON containing the absolute executable,
canonical project, and explicit `--profile` when supplied. It does not edit another
application's settings or start a model. AppImage registration uses the persistent
AppImage path and `--appimage-extract-and-run`, not a temporary extracted binary.
Keep the registered executable in that location. The server's stdout contains only
newline-delimited JSON-RPC; errors go to stderr. Do not combine `mcp serve` with
the CLI's `--json` option.

The server uses the active desktop/headless engine for the selected profile, or
opens its own temporary engine. Its project selection does not navigate the
desktop. It cannot switch projects, modify trust, or change the profile's settings.
Sessions are filtered by project before the listing limit is applied. Goals and
checkpoints are checked against that same canonical project before being returned
or changed.

Access is read-only by default. `--allow-write` permits changes only in an already
trusted project and within its configured permission level. Each `shadow_run`
also defaults to `permission_level: "read_only"`; a client must explicitly request
`"workspace"` or `"elevated"` for a writing task. This per-task ceiling can reduce
the configured authority, never increase it, and does not alter saved settings.

Pending actions can be approved through ShadowCode's desktop or CLI. To delegate
approval decisions to the connecting client, explicitly add **both**
`--allow-write --allow-approvals` to `mcp serve` (or to `mcp register` when producing
its configuration). The `shadow_approve` tool then resolves one exact pending
approval from a task created by this connection. It cannot approve another
client's task or all pending actions. Denying an owned approval does not require
the approval flag. The client is responsible for obtaining the user's agreement
to the displayed action before submitting approval.

The current server exposes seventeen tools:

- `shadow_status`, `shadow_models`, `shadow_sessions`, `shadow_review`, and
  `shadow_tools` inspect the selected project and native capabilities.
- `shadow_understand`, `shadow_doctor`, and `shadow_why` provide bounded project
  maps, native diagnostics and recorded change history. See
  [inspection and diagnostics](NATIVE_INSPECTION.md) for their limits and optional
  saving/model-probe behavior.
- `shadow_memory` reads, appends or replaces project/task notes. Task scope uses
  an exact existing task ID in this project; replacement requires the current
  content hash. Changes require write access. See [native memory](NATIVE_MEMORY.md).
- `shadow_sqlite` lists tables or runs one bounded read-only query against an
  existing project database. It supports bound parameters and live WAL data;
  SQLite may maintain coordination sidecars. See [SQLite inspection](NATIVE_SQLITE.md).
- `shadow_goal` creates, lists, inspects, advances or abandons durable goals.
  Creating or advancing a goal here does not start an agent automatically.
- `shadow_run` creates a fresh conversation and returns its job ID. `shadow_jobs`
  inspects that connection's jobs, paginated events and pending approvals, or
  cancels one. A returned job ID is not a claim that the task has completed.
- `shadow_approve` handles the individual decision described above.
- `shadow_test` submits an owned native test job without calling a model. An
  omitted command must have one unambiguous detected candidate. Write access
  and exact command approval are required; results include real output and exit
  status through `shadow_jobs`.
- `shadow_checkpoint` inspects a project checkpoint. `shadow_rollback` requires
  an exact task ID, `confirm: true`, write access and the engine's conflict checks.

Resources are `shadow://project`, `shadow://sessions`, `shadow://memory`, and `shadow://plan`.
The plan resource reads recorded events and returns null when no plan is recorded.
The `delegate` prompt accepts a task for the fixed project; `understand` requests
a read-only project inspection.

The connection owns its delegated jobs. Normal EOF, protocol failure, explicit
cancellation and an abandoned server future cancel unfinished owned jobs and await
cleanup; unrelated tasks in the shared engine continue. A temporary engine closes
when its server exits. The first delegated task opens a private ownership socket
to the engine. Ownership is registered before scheduling, so an unread submission
reply or a forcibly killed gateway cannot silently detach its jobs. Socket EOF
cancels that owner's running and queued tasks, including active command children.
Queued cancellations do not wait for unrelated work ahead of them. Ordinary
detached CLI jobs remain independent of this ownership connection.

At most eight ownership sockets may be active per profile, reserving room for
ordinary control requests. Each accepts at most 64 jobs over its lifetime and
stays confined to its selected project. Invalid or abandoned protocol exchanges
close ownership; valid rejected task submissions leave it usable. Submissions
and cleanup acknowledgements have 30-second response deadlines. Normal close
waits for the engine's task cleanup before acknowledging it.

Incoming messages share the 1-MiB frame, 32-MiB lifetime and 128-frames-per-second
transport limits. Initialization has a ten-second deadline. Up to eight tool or
resource operations and 64 delegated tasks are allowed per connection. Other
operations have a 45-second deadline; model jobs are asynchronous and follow the
engine's own limits. Tool/resource results are capped at 2 MB and response writes
have a five-second deadline. Reconnect after a connection limit is reached.

This is a development server, not full compatibility with the earlier Python
server. HTTP serving and client-specific registration formats
remain unimplemented. Tools use the fixed project and asynchronous owned jobs;
map saving is explicit, and diagnostics do not perform automatic repairs.
The current tests use the official
Rust SDK and a real executable protocol probe; broader client interoperability
and real local-model server tasks remain release checks.

## Enable external tools

In **Settings → MCP**, register a server with its name and a JSON array containing
the executable and literal arguments, or choose **Streamable HTTP** and enter
the server endpoint. Review and enable it for the current trusted project.
Registration and catalog refresh do not execute commands or contact servers.
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

HTTP definitions use `url` instead of `command`, and an optional `api_key_env`
reference for a bearer token:

```json
{
  "name": "remote-tools",
  "url": "https://example.com/mcp",
  "api_key_env": "MY_MCP_TOKEN",
  "timeout_sec": 30
}
```

HTTP credentials resolve from the same protected store or application environment
when the connection starts; the catalog shows the reference name only. Credential
values are redacted from metadata and tool results. `env` and `env_refs` apply to
stdio commands and are rejected on HTTP definitions rather than silently ignored.
Loopback endpoints work with network access disabled. Other hosts require
**Permissions → Network access**, and bearer credentials require HTTPS outside
loopback. The hostname `localhost` resolves only to loopback addresses in this
transport. Redirects and inherited HTTP proxies are disabled. Tokens are sent
only to the reviewed endpoint; authentication challenges do not trigger an
automatic login or credential exchange. OAuth and custom authentication headers
are not yet supported.

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
and untrusted projects cannot open MCP connections.

Running and queued tasks retain their configuration snapshot. Disabling a grant
or replacing a global registration applies to new tasks; cancel existing tasks
to stop their current connections. Project files are re-read and their hash
checked before each external action, including after an approval wait.
Definitions cannot replace their command or endpoint while an earlier approval
is pending.
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

## Streamable HTTP

`native/core/src/mcp/http.rs` supplies a bounded backend to the official SDK's
HTTP worker. It first negotiates the 2026-07-28 discovery lifecycle. Legacy
2025 Streamable HTTP servers use `initialize`, optional session IDs and GET SSE,
and bounded session deletion during cleanup. The separate-endpoint 2024 HTTP+SSE
transport is not implemented; use the server's Streamable HTTP endpoint or stdio.

The 2026 lifecycle sends protocol/method/name headers and schema-declared
`x-mcp-header` parameters, including encoded Unicode values. Its requests are
stateless: no session creation, GET stream, or session deletion. The SDK validates
header annotations before exposing tools. JSON and SSE responses preserve
structured results and tool error status.

HTTP shares the 16-connection, 128-tool, 512-KiB argument and 32-MiB lifetime
limits above. JSON bodies and raw SSE events are capped at 1 MiB before parsing;
128 events per second include blank frames, preventing comment/whitespace floods
from bypassing limits. Session IDs are bounded to 1 KiB, event IDs to 4 KiB, and
protocol headers to 16 KiB. Two ordinary requests and a bounded control channel
limit transport concurrency. Initialization, discovery and calls have deadlines.
Malformed, oversized, truncated, or interrupted responses close the connection.
Peer error bodies and authentication challenges are not included in diagnostics.

Automatic retry, SSE resumption, and session reinitialization are disabled. A
404 for an expired legacy session fails the call and closes the connection; it
never silently repeats an approved operation. Cancellation closes active
streams. Legacy session deletion has a one-second deadline, including after
task cancellation. Connection permits remain held by transport workers through
cleanup, including abandoned initialization. The SSE parser's omitted notices
are retained from the exact `sse-stream` 0.2.6 source commit and digest-checked.

## Ownership and cancellation

Each stdio connection owns its subprocess group and bounded stderr drain. Cancellation
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
cargo test -p shadowcode-core --test mcp_stdio --test mcp_http --test mcp_application --locked
cargo test -p shadowcode-core --test mcp_native_server --locked
cargo test -p shadowcode-core --test control_owned --locked
node scripts/test-native-mcp-server.mjs
```

Ten regressions exercise real subprocess pipes, current and legacy protocol
negotiation, paginated discovery, exact arguments/environment, tool error results,
malformed/oversized/truncated initialization, duplicate names, pagination loops,
catalog limits, command timeouts, cancellation, idle/drop/abandoned initialization,
stubborn descendant cleanup, leader reaping, stderr floods, protocol floods, and
refusal of unsolicited sampling/credential requests. The small Node peer is a
test fixture; no Node or Python runtime is added to the application.

Eleven application regressions cover inert/symlink-confined discovery, malformed
definitions, hidden credentials, project trust and permissions, definition
hashes, removal/re-registration, exact argument approval and denial, connection
reuse, error-result preservation, bounded single-pass credential redaction,
closed-catalog rejection, stale file observations, read-only tasks, and
process cleanup before actual engine completion on every exit path. HTTP
coverage includes loopback/remote activation, transport-specific credential
fields, lazy secret resolution, redaction of reflected authorization headers,
and active stream/session cleanup on task completion, cancellation and shutdown. The native
CLI and real-window probes also exercise their respective controls; the window
probe drives a scripted model through discovery, approval, a real subprocess
tool result, and cleanup.

Six HTTP regressions exercise modern and legacy negotiation, metadata and Unicode
headers, fragmented UTF-8/CRLF SSE, explicit bearer authentication, redirect
refusal, expired sessions without replay, malformed/oversized/truncated/flooded
responses, private discovery errors, initialization/call deadlines, cancellation,
client drop, and abandoned initialization. The desktop probe also registers an
HTTP endpoint in Settings and completes an approved, authenticated streaming call.

Eight server regressions use the official SDK to check discovery, resources/prompts,
project isolation, session filtering before limits, write/trust restrictions,
per-task permission reduction, owned-job and approval isolation, real approved
execution, EOF/drop cleanup, malformed/oversized/truncated frames, and floods.
The headless executable probe completes a scripted task with a file read, real
write, exact command approval, terminal verification, checkpoint and rollback.
It also checks registration, protocol-only stdout, resources, EOF and profile
restart. Two more scripted model requests exercise an unrelated detached task
and an approved long-running terminal command. The probe kills the native MCP
gateway with SIGKILL and verifies that the command child stops, owned queued
work never reaches the model, and the unrelated task remains running. CI runs
this probe against both the source binary and AppImage.
The probe also reads project maps/diagnostics and completes an explicitly approved
native test job while verifying that no additional model requests are made.

Four private-control regressions check owner capacity without exhausting normal
control access, recovery after rejected task submissions, deleted completed
history, immediate queued cancellation behind unrelated work, aborted transport
handlers, unread submission replies, malformed frames and cross-project attempts.

These checks do not stand in for the remaining server features, interoperability,
and real local-model checks required for the full MCP migration. The standalone
application does not bundle runtimes for third-party servers: install whatever
an explicitly selected external command requires separately.
