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

`GET /api/accounts?cached=1` returns the same shape without probing anything:
vendors not checked yet in this process have `availability: "unavailable"`,
`detail: "Not checked yet"`, and their persisted usage marked
`state: "stale"` ("Last checked …"). Use it for the first paint, then
`GET /api/accounts` (probes at most once per 5 minutes per vendor).

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
  billing: "subscription" | "api_key",  // api_key: CLI signed in with an API key (billed per token)
  usage: UsageSnapshot                  // account-level (default pool)
}
```

An API-key login (Codex `auth_mode: "apiKey"`, Claude `authMethod` other
than `claude.ai`) is labelled `API key login · billed per token` (row
`subtitle` and `usage.label`) and never shows plan usage. Vendor CLIs are
always started without `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`,
`OPENAI_API_KEY`, `CODEX_API_KEY`, `CURSOR_API_KEY`, `XAI_API_KEY`,
`GROK_API_KEY`, `GEMINI_API_KEY`, `GOOGLE_API_KEY` in their environment.
A model row whose usage pool reports the plan limit is `availability:
"unavailable"` with the usage label as `reason`.

- `POST /api/accounts/{vendor}/connect` → `{ ok, state: "started"|"unsupported"|"already_running", note, hint? }`.
  Runs the official login command (`codex login`, `claude auth login`,
  `cursor-agent login`, `grok login`) with the user's environment; the vendor
  opens the browser or prints a URL / device code. Progress lines arrive as
  events `account.login {vendor, line, url}` (`line` redacted; `url` is the
  first https URL on the line unless it carries a code/token parameter, else
  `null`); completion as `account.login.done {vendor, ok, detail, availability, availability_label}`
  after a forced re-probe. One login per vendor; stops after 10 minutes.
  These events are not tied to a session (broadcast wakeup only), so the UI
  reads the lines from `GET /api/accounts/{vendor}/login`.
  Antigravity returns `unsupported` with the hint to run `agy` once.
- `GET /api/accounts/{vendor}/login` → `{ vendor, running, started_at, lines: [{vendor, line, url}], done: {ok, detail, availability}|null }`.
- `POST /api/accounts/{vendor}/disconnect {confirm: true}` → `{ ok, ran: string[], output, note, availability, availability_label }`; runs the
  official logout command (never touches credential files), then forgets the
  vendor's cached status, persisted usage and every conversation's
  `native_session:<vendor>`, and re-probes. Without `confirm: true` it runs
  nothing and answers `{ ok: false, needs_confirm: true, ran: [], note: shared_cli_note }`.
  Antigravity answers `{ ok: false, ran: [], note }` (logout only inside `agy`).
- `POST /api/accounts/{vendor}/refresh` → `VendorStatus` (skips the 5-minute freshness window, never the failure backoff).
- `POST /api/accounts/{vendor}/cancel-login` → `{ ok }` (`ok: false` when no login was running).

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
  `native_sessions: {[vendorId]: string}` (not with `?summary=true`).
- `POST /api/sessions/{id}/target {target_id}` → `{ ok, execution_target, provider, applies_to: "this_turn"|"next_turn" }`
  (remembered per conversation; also becomes the workspace default,
  `native_meta` `execution_target:<workspace>`). `target_id` must be a picker
  id the backend resolves (`cli:*`, `local:gguf:*`, a registry id); display
  names are rejected. A running job keeps the target it started with:
  `applies_to: "next_turn"` means the UI queues the switch for the next turn.
- `POST /api/jobs`: `model` is the exact picker id and is stored as the
  conversation's `execution_target` (and workspace default). Without `model`
  the conversation's target is used, then the workspace default, then the
  configured default. Job records keep the exact id in `routing.model_id`;
  `routing` also carries `inference: "local"|"cloud"` and
  `route: "vendor_cli"|"local_llamacpp"|"native_http"`.
- `POST /api/jobs` body gains `web: boolean` (web tools for this task) and
  `handoff_consent: boolean`. Consent is required when the target is a cloud
  route and (a) the previous turn ran on this computer, (b) the previous turn
  ran on a different provider (its unseen turns are handed over), or (c) the
  request attaches images and this conversation never consented to cloud
  attachments. Without consent nothing is written and the backend answers
  (as a normal IPC value, since IPC has no status codes)
  `{ok: false, status: 409, error, needs_consent: true, handoff: {from, to, excerpt_chars, images, reason}}`;
  the UI asks and resends the same body with `handoff_consent: true`.
- Handoff: when the provider changes, the turns the new provider has not seen
  (user requests, final answers, changed files; at most 12 000 characters) are
  sent as a `<prior_conversation>` block marked as context, not instructions:
  before the task text for vendor CLIs; for the native loop the same turns
  are in the conversation's message tape (vendor turns are appended to it).
  Same provider, different model: no handoff; the vendor's own mechanism
  switches the model on the resumed session and `model.switched` is emitted.
- Job status `limit_reached` (terminal): the vendor reported its plan limit;
  the job stopped without retry, `result.limit_reached = {vendor, detail, usage}`.
  ShadowCode never buys credits, redeems resets or enables overages.
- Events added: `vendor.session {vendor, session_id}`,
  `agent.handoff {from, to, excerpt_chars, turns, files, delivery: "prompt_prefix"|"message_tape", job_id}`,
  `model.switched {provider, from, to, resumed}`,
  `usage.updated {vendor, usage}` (Codex pushes during a turn),
  `limit.reached {vendor, usage, detail, job_id}` (job stops; user picks another model),
  `checkpoint.restored {task_id, paths}`,
  `web.source {url, final_url, title, status}` (also embedded in `tool.completed.sources`).
- Read-only (Plan/Review) vendor tasks: command/file-change prompts from the
  vendor are denied automatically with an `agent.warning` ("Denied
  automatically: …"). Vendors that never ask (Antigravity, `asks_approval:
  false`) get an `agent.warning` at task start saying so.

## Config

`GET/PUT /api/config`:

- `permissions.mode: "ask" | "allow_edits"` — the two user-facing modes.
  `ask` = shell and file edits ask; `allow_edits` = file edits inside the project
  are allowed, shell still asks. Vendor mapping is described in
  `permissions.vendor_notes: {[vendorId]: string}` returned with the config.
- `network.mode: "online" | "web_off" | "offline"` — `web_off` disables web
  tools only; `offline` also suppresses account/usage refresh and any helper
  network activity. Cloud rows are marked unavailable in `offline`.
