# ShadowCode 0.28 desktop API contract

The React window talks to the in-process Rust engine through
`invoke("api", {request: {method, path, body}})` (see `ui/src/lib/transport.ts`).
This file is the contract the 0.28 UI and backend are built against. All
shapes are JSON. Unknown values are `null`, never invented.

## Picker

`GET /api/picker` → `{ targets: PickerTarget[], local_engine: LocalCatalog, vendors: {[vendorId]: VendorStatus}, generated_at: number }`

`PickerTarget` (one row of the composer dropdown):

```
{
  id: string,             // stable routing id: "cli:codex:gpt-6-astra", "cli:cursor:auto",
                          // "cli:cursor:gpt-5.5[context=272k,...]", "cli:claude", "local:gguf:<hash>"
  provider: string,       // "cli:codex" | "cli:claude" | "cli:cursor" | "cli:antigravity" | "cli:grok" | "llamacpp"
  account: string,        // "account:codex" … | "this-computer"
  model: string,          // exact model value the runtime accepts; "default" | "auto"
  route: string,          // "vendor_cli" | "local_llamacpp"
  group: "subscriptions" | "local",
  name: string,           // "Codex · GPT-6-Astra", "qwen3-14b · This computer"
  subtitle: string,       // "Cloud · subscription" | "Runs on this computer · No subscription quota"
  inference: "cloud" | "local",
  availability: "ready" | "sign_in" | "setup_required" | "unavailable",
  availability_label: "Ready" | "Sign in" | "Setup required" | "Unavailable",
  reason: string,         // why not ready, or the ready detail (version, plan, email)
  featured: boolean,
  vision: boolean,        // images accepted by model AND runtime (protocol/mmproj)
  tools: boolean,         // false ⇒ "Chat only"
  is_default: boolean,
  usage: UsageSnapshot,
  local?: LocalDetail      // present for local rows
}
```

`UsageSnapshot`:

```
{
  state: "ok" | "stale" | "unavailable" | "local" | "limit_reached",
  label: string,                       // one line for the row
  detail: string[],                    // tooltip / expandable lines
  plan: string|null, pool: string|null, pool_shared: boolean,
  windows: [{ label, used_percent, remaining_percent, window_minutes, resets_at }],
  remaining_percent: number|null,
  credits: { has_credits, unlimited, balance }|null,
  limit_reached: boolean,
  last_refresh: number|null,           // unix seconds
  provider_usage_url: string|null
}
```

Rows that are not `ready` are still rendered (not disabled): activating a
`sign_in` row opens Accounts › Connect, a `setup_required` row opens the
matching setup hint, `unavailable` shows `reason`.

## Accounts

`GET /api/accounts?refresh=1` → `{ vendors: {[id]: VendorStatus}, config: CliAgentsConfig, local_engine: LocalCatalog }`

`VendorStatus`:

```
{
  id: "cli-codex", label, state: "ready"|"not_logged_in"|"not_installed"|"unavailable",
  status: "pass"|"warn"|"info", availability, availability_label, detail, version, binary, fix,
  account: { email, plan, auth_mode }|null,
  models: [{ id, label, is_default, vision }],
  accepts_images: boolean, asks_approval: boolean,
  fetched_at: number, error: string|null, usage_note: string|null,
  login_command: string[], logout_command: string[], shared_cli_note: string,
  usage: UsageSnapshot                  // account-level (default pool)
}
```

- `POST /api/accounts/{vendor}/connect` → `{ ok, state: "started"|"unsupported"|"already_running", note, hint? }`.
  Runs the official login command with the user's environment; the vendor
  opens the browser or prints a URL. Progress lines arrive as events
  `account.login {vendor, line}`; completion as `account.login.done {vendor, ok, detail}`.
  Antigravity returns `unsupported` with the hint to run `agy` once.
- `GET /api/accounts/{vendor}/login` → `{ running: boolean, lines: string[], done: { ok, detail } | null }`
  (added by the UI branch): the redacted output lines of the current or last
  Connect for that vendor. The window re-reads it when `shadowcode:events`
  wakes it (the Tauri wake-up carries no event body) and every 1.5 s while a
  login runs. `connect` may also return the first `lines` it already has.
  Without this route the Accounts page still shows "Finish signing in on the
  page the vendor opened" and Refresh confirms the result.
- `POST /api/accounts/{vendor}/disconnect` body `{confirm: true}` (sent only after
  the UI showed `shared_cli_note`) → `{ ok, ran: string[], note }`; runs the
  official logout command (never touches credential files), clears cached
  usage/models, re-probes.
- `POST /api/accounts/{vendor}/refresh` → `VendorStatus` (forced, bounded by backoff).
- `POST /api/accounts/{vendor}/cancel-login` → `{ ok }`.

## Local models

`GET /api/local-models` → `LocalCatalog`:

```
{
  hardware: { cpu_cores, ram_bytes, gpu: string|null, vram_bytes: number|null,
              backend: "vulkan"|"cpu"|"unknown", devices: string[], detail: string },
  runtime:  { state: "ready"|"setup_required"|"unavailable", path, origin: "bundled"|"managed"|"other",
              version, backend, commit, detail },
  models: GgufEntry[],
  loaded:  { id, name, port, since, context_tokens, backend }|null,
  ollama_store: { path, available: boolean, models: [{ tag, path, projector, bytes, compatible, reason, already_added }] }
}
```

`GgufEntry`:

```
{
  id: "local:gguf:<hash>", name, path, bytes, source: "file"|"directory"|"ollama",
  architecture: string|null, context_train: number|null, context_tokens: number,
  compatible: boolean, reason: string,
  vision: boolean, mmproj: string|null, tools: boolean, tools_reason: string,
  memory: { weights_bytes, kv_cache_bytes, compute_bytes, projector_bytes, overhead_bytes, total_bytes, context_tokens },
  fits: "gpu"|"cpu"|"no", availability: "ready"|"setup_required"|"unavailable",
  last_error: string|null
}
```

- `POST /api/local-models/add {path}` (file or folder) → `{ ok, local_engine }`
- `POST /api/local-models/remove {path}` → `{ ok, deleted_weights: false }`
- `POST /api/local-models/import-ollama {tag}` → adds the blob paths (never copies) → `{ ok, local_engine }`
- `POST /api/local-models/load {id}` → `{ ok, loaded }` (starts llama-server; cancellable; one at a time)
- `POST /api/local-models/unload` → `{ ok }`

## Sessions and jobs

- `GET /api/sessions/{id}` includes `execution_target: string|null` and
  `native_sessions: {[vendorId]: string}`.
- `POST /api/sessions/{id}/target {target_id}` → `{ ok }` (remembered per conversation; the
  workspace default is `native_meta` `execution_target:<workspace>`).
- `POST /api/jobs` body gains `web: boolean` (web tools for this task) and
  `handoff_consent: boolean` (required when the previous turn of the session ran
  on a different provider, or when local content/attachments would go to a cloud route).
  Without consent the backend answers `409 {error, needs_consent: true, handoff: {from, to, excerpt_chars, images}}`.
  The desktop IPC has no HTTP status, so the same body may arrive either as the
  successful value of the call or as the error string (JSON text, optionally
  after a prefix); the UI treats both as a consent request and creates nothing
  until it resends with `handoff_consent: true`.
- `GET /api/sessions/{id}.execution_target` may already carry the workspace
  default for a conversation that has none of its own; the UI otherwise falls
  back to the last target chosen in that project (local cache) and never to
  `config.model`.
- `POST /api/onboarding` is sent without `provider`/`model` (the backend keeps
  its model configuration): `{ workspace, permission_level: "workspace",
  permission_mode: "ask" | "allow_edits", theme: "system" }`. The UI also
  writes `permissions.mode` with `PUT /api/config`.
- `/model` is handled by the window (opens the picker); it is never sent to
  `POST /api/commands/run`. A command result with `overlay: "model" | "picker"`
  also opens the picker.
- Events added: `vendor.session {vendor, session_id}`, `agent.handoff {from, to, excerpt_chars}`,
  `usage.updated {vendor, usage}`, `limit.reached {vendor, usage}` (job pauses;
  user picks another model), `checkpoint.restored {task_id, paths}`,
  `web.source {url, final_url, title, status}` (also embedded in `tool.completed.sources`).
- Events the UI also reads (optional): `routing.selected.inference`
  ("cloud" | "local", shown as "· Cloud" / "· This computer"),
  `model.switched {provider, from, to, resumed}`, `approval.requested` /
  `approval.resolved` (Waiting for approval step), `checkpoint.updated.paths`,
  `files.changed.paths` and `verification.summary {status, commands[{command,
  exit_code, success, timed_out}], presented_as, note, vendor_agent}` (summary
  card). The activity timeline classifies `tool.started/completed` by tool
  name: native names and vendor names such as `codex.command_execution`,
  `codex.file_change`, `cursor.read`, `Bash`, `Edit`.

## Config

`GET/PUT /api/config`:

- `permissions.mode: "ask" | "allow_edits"` — the two user-facing modes.
  `ask` = shell and file edits ask; `allow_edits` = file edits inside the project
  are allowed, shell still asks. Vendor mapping is described in
  `permissions.vendor_notes: {[vendorId]: string}` returned with the config.
- `network.mode: "online" | "web_off" | "offline"` — `web_off` disables web
  tools only; `offline` also suppresses account/usage refresh and any helper
  network activity. Cloud rows are marked unavailable in `offline`.
