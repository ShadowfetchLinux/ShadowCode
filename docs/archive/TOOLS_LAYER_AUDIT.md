# Tools layer audit (0.21)

Evidence from `native/core/src/tools.rs`, `permissions.rs`, `autonomy.rs`,
and tests in `native_tools.rs` / `autonomy_021.rs`. Not a sandbox claim.

## Approvals

- Tool arguments never set permissions. `permissions::check` decides
  Allow / Ask / Deny before the action.
- Ask waits on `ApprovalHub`. Wrong-session decide is rejected. Denied,
  cancelled, and expired approvals return failure (`success: false`).
- Read-only mode does not run `exec`. Cancellation of a pending shell
  approval does not create the file.

## Invalid arguments

- Non-object arguments fail with `Tool arguments must be an object`.
- `execute` maps inner errors to `ToolResult { success: false }`. It does
  not panic on bad JSON types, missing paths, or out-of-range integers.

## Cancel is not success

- `cancelled` / timeout / denial map to `ToolStatus` variants other than
  `Success`. A cancelled shell does not report `ok: true`.

## Replay classes (mutating tools)

| Tool | Replay |
| --- | --- |
| `list_files`, `read_file`, searches, sqlite reads, git status/diff/log | SafeToReplay |
| `git_branch` | ReEvaluate |
| `exec`, background start/stop, `mcp_call`, `git_commit` / `checkout` / `add` | RequiresConfirmation |
| file mutations, `git_reset`, `git_clean` | NeverAutoReplay |

Crash repair writes an unknown-result tool message. Shell and mutations
are never auto-replayed. Engine reattach does not execute tools.

## Truncation

Unbounded reads, listings, search hits, process output, and
`ToolResult::message` set `truncated` (and a `note` / `next_offset` where
the range budget applies). The model is expected to see that flag.

## Not claimed

Lexical shell policy is not kernel isolation. MCP processes can change
the workspace. Checkpoints cover native file tools, not arbitrary shell.
