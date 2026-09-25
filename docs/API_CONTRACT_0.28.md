# ShadowCode desktop API contract (since 0.28)

The React window talks to the in-process Rust engine through
`invoke("api", {request: {method, path, body}})` (see `ui/src/lib/transport.ts`).
This file is the contract the UI and backend are built against. It started
with 0.28 and keeps its name; it is current as of 0.30.1 (OpenRouter from
0.29.0, Antigravity's agent server from 0.30.0). All shapes are JSON. Unknown
values are `null`, never invented.

## Picker

`GET /api/picker` → `{ targets: PickerTarget[], local_engine: LocalCatalog, vendors: {[vendorId]: VendorStatus}, generated_at: number }`

`PickerTarget` (one row of the composer dropdown):

```
{
  id: string,             // stable routing id: "cli:codex:gpt-6-astra", "cli:cursor:auto",
                          // "cli:cursor:gpt-5.5[context=272k,...]", "cli:claude", "local:gguf:<hash>",
                          // "api:openrouter:<slug>"
  provider: string,       // "cli:codex" | "cli:claude" | "cli:cursor" | "cli:antigravity" | "cli:grok" | "llamacpp" | "openrouter"
  account: string,        // "account:codex" … | "this-computer" | "" (OpenRouter)
  model: string,          // exact model value the runtime accepts; "default" | "auto"
  route: string,          // "vendor_cli" | "local_llamacpp" | "native" (OpenRouter)
  group: "subscriptions" | "local" | "api",
  name: string,           // "Codex · GPT-6-Astra", "qwen3-14b · This computer"
  subtitle: string,       // "Cloud · subscription" | "Runs on this computer · No subscription quota" | "OpenRouter · <slug>"
  inference: "cloud" | "local",
  availability: "ready" | "sign_in" | "setup_required" | "unavailable",
  availability_label: "Ready" | "Sign in" | "Setup required" | "Unavailable",
  reason: string,         // why not ready, or the ready detail (version, plan, email)
  featured: boolean,
  vision: boolean,        // images accepted by model AND runtime (protocol/mmproj)
  tools: boolean,         // false ⇒ "Chat only"
  reasoning: boolean,     // the composer's reasoning-effort control applies
                          // (Codex, Claude Code, OpenRouter models listing
                          // `reasoning`, GGUF templates with a thinking switch)
  is_default: boolean,
  usage: UsageSnapshot,
  local?: LocalDetail      // present for local rows
}
```

`UsageSnapshot`:

```
{
  state: "ok" | "stale" | "unavailable" | "local" | "limit_reached" | "api_key",
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

The UI shows three groups in this order: `subscriptions`, `local`, `api`
(titled *API keys*). Without OpenRouter rows the `api` group shows one
*Add an OpenRouter API key…* entry that opens Accounts. See
[OpenRouter](#openrouter-api-keys) for the `api` rows.

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
  usage: UsageSnapshot,                 // account-level (default pool)
  install: AntigravityInstall|null      // Antigravity only; null for other vendors
}
```

`AntigravityInstall` (also returned by the install endpoints below):

```
{
  installed: boolean,       // agy_acp_server.par and localharness_external found
  version: string,          // pinned server version, "1.2.1"
  path: string|null,        // the server file in use
  managed: boolean,         // true when it is ShadowCode's own install
  download_bytes: number,   // pinned archive size
  installed_bytes: number,  // unpacked size
  source: string,           // pinned dl.google.com URL
  dir: string,              // ~/.local/share/shadowcode/antigravity-acp/1.2.1
  state: "not_installed" | "downloading" | "verifying" | "unpacking" | "installed" | "error",
  busy: boolean,            // downloading, verifying or unpacking
  done: number, total: number,  // bytes, for the progress bar
  error: string|null
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
  Antigravity has no login command: Connect starts the installed agent server
  with ShadowCode's private profile, sends ACP `initialize` and
  `authenticate {methodId: "oauth-personal"}`, lets the server open the
  browser and relays the sign-in link it prints. It fails with the install
  hint when the server isn't installed.
- `GET /api/accounts/{vendor}/login` → `{ vendor, running, started_at, lines: [{vendor, line, url}], done: {ok, detail, availability}|null }`.
- `POST /api/accounts/{vendor}/disconnect {confirm: true}` → `{ ok, ran: string[], output, note, availability, availability_label }`; runs the
  official logout command (never touches credential files), then forgets the
  vendor's cached status, persisted usage and every conversation's
  `native_session:<vendor>`, and re-probes. Without `confirm: true` it runs
  nothing and answers `{ ok: false, needs_confirm: true, ran: [], note: shared_cli_note }`.
  For Antigravity (after the same confirmation) it runs no command: it deletes
  ShadowCode's private Antigravity profile and answers
  `{ ok: true, ran: [], output, note, availability, availability_label }`.
- `POST /api/accounts/{vendor}/refresh` → `VendorStatus` (skips the 5-minute freshness window, never the failure backoff).
- `POST /api/accounts/{vendor}/cancel-login` → `{ ok }` (`ok: false` when no login was running).
- `GET /api/accounts/antigravity/install` → `AntigravityInstall`.
- `POST /api/accounts/antigravity/install {confirm: true}` → `AntigravityInstall`
  plus `started: boolean` (`false` when a download is already running).
  Starts the download in the background: size and SHA-256 are checked against
  the pinned values before the archive is unpacked. Without `confirm: true` it
  fails with "Confirm the 334 MB download first"; offline it fails with
  "Offline mode: downloads are off".
- `POST /api/accounts/antigravity/uninstall` → `AntigravityInstall`. Deletes
  the installed server (not the sign-in profile) and forgets the cached status.

## OpenRouter (API keys)

`GET /api/openrouter` → `OpenRouterStatus`:

```
{
  key_set: boolean,
  key: { label, usage, limit, limit_remaining, is_free_tier }|null,  // from GET /api/v1/key
  key_error: string|null,
  models: number, tool_models: number,   // from the cached model list
  fetched_at: number|null,               // unix seconds
  offline: boolean,
  keys_url: "https://openrouter.ai/keys",
  activity_url: "https://openrouter.ai/activity"
}
```

The key itself is never returned. With a key set and not offline, the status
checks the key with OpenRouter's `GET /api/v1/key`.

- `POST /api/openrouter/key {api_key}` → `OpenRouterStatus`. A non-empty key
  is checked with `GET /api/v1/key` and stored as `OPENROUTER_API_KEY` in the
  profile's `secrets.env` (mode 600) only if OpenRouter accepts it; the first
  save also fetches the model list. An empty `api_key` removes the stored key.
  Saving a key is refused offline; removing one is not.
- `POST /api/openrouter/refresh` → `OpenRouterStatus` after fetching the model
  list now. Refused offline.

The model list comes from OpenRouter's public `GET /api/v1/models` (text
models only) and is cached in the state directory as
`openrouter-models.json`. It is fetched only once a key is saved. `/api/picker`
shows the cached list at once and refreshes it in the background when it is
older than 6 hours.

Picker rows (`group: "api"`, present only while a key is saved):
`id: "api:openrouter:<slug>"`, `provider: "openrouter"`, `account: ""`,
`route: "native"`, `inference: "cloud"`, `featured: false`,
`is_default: false`, `vision` and `tools` from OpenRouter's model list
(`tools: false` ⇒ "Chat only"), `availability: "ready"` (or `unavailable`
offline). `usage` has `state: "api_key"`, a label such as
`API key · $0.15/M in · $0.60/M out` or `API key · free`, and
`provider_usage_url` set to the activity URL.

A job on an `api:openrouter:` id runs on the native loop (`routing.route:
"native_http"`, `inference: "cloud"`). It is refused before a job exists when
offline or when no key is stored. The context limit comes from the cached
list, capped at 200,000 tokens.

## Allowance

`GET /api/allowance` (`?refresh=1` re-checks vendor accounts) returns
`{generated_at, rows}`, one row per source, built only from reported data
(`native/core/src/allowance.rs`):

```ts
Row {
  id: "cli:<vendor>" | "openrouter" | "local",
  kind: "subscription" | "api_key" | "local",
  product: string,
  state: "ok" | "low" | "limit_reached" | "unknown" | "sign_in"
       | "not_installed" | "unavailable" | "offline" | "no_key" | "none",
  headline: string,                 // "2% left", "$4.99 of $5.00 left", …
  remaining_percent: number | null, // lowest reported window, or key credit
  windows: {label, remaining_percent, resets_at}[],
  plan, note, last_checked, usage_url,
  // openrouter: used, limit, limit_remaining (USD)
  // local: ready_models, on_limit: "local" | "ask", fallback: {id, name} | null
}
```

`low` means 10% or less left.

## Plan limits

Config `limits`: `{on_limit: "local" | "ask", fallback_model: "" | "local:gguf:…"}`
(default `on_limit: "local"`). When a vendor job ends with status
`limit_reached`, the engine records a `limit.fallback` event on that task:

- `{ok: true, from, to, target, job_id}`: a follow-up job started in the same
  session on the local model `target`, with the task "Continue where <from>
  stopped when its plan limit was reached. The request was: …"; the session's
  `execution_target` becomes `target`.
- `{ok: false, ask: true}`: `on_limit` is `"ask"`; nothing started.
- `{ok: false, from, reason}`: no local model is ready.

The fallback model is `fallback_model` when ready, else the last local model
used in the project, else the first ready local model with tool support.

## Compare

See [Compare](COMPARE.md) for `POST /api/compare`, `GET /api/compare/<id>`,
`GET /api/compares`, `keep`, `discard`, `cancel` and the scoreboard.

## Code intelligence

Language servers, the code index, search, the repo map and embedding models
([code intelligence](CODE_INTELLIGENCE.md)). Routes act on the selected
project. Downloads are refused in offline mode.

`GET /api/code-intel/status` →

```
{
  config: CodeIntelSettings,          // effective code_intel settings
  config_error: string|null,          // set when config.yaml's section is invalid (defaults in use)
  offline: boolean,
  languages: [{ language: "rust"|"typescript"|"python"|"go"|"c", label, enabled,
                available, server?, path?, source?: "config"|"managed"|"path",
                note?, install_hint?, managed_package: "typescript"|"python"|null }],
  servers: [{ root, language, server, program, state: "ready"|"loading"|"backoff"|"stopped"|"idle"|"busy",
              pid, idle_sec, starts, failures, last_error }],
  managed: [{ id: "typescript"|"python", label, packages: ["name@version"], approx_bytes,
              installed, installed_bytes: number|null, versions: [{name, version|null}], path,
              progress: {state: "installing"|"installed"|"error", error, log}|null }],
  managed_dir, npm: { available, path, node },
  index: { files, symbols, references, chunks, languages: {[lang]: files}, max_files } | null,
  embeddings: { models: EmbeddingModel[], active: string|null, runtime: string|null,
                server: {model, pid, idle_sec}|null,
                coverage: {embedded, chunks}|null,
                backfill: {state: "running"|"done"|"error", embedded, error}|null }
}
CodeIntelSettings = { lsp, diagnostics_on_edit, diagnostics_wait_ms, lsp_idle_minutes, max_servers,
                      servers: {[language]: {command, args}}, repo_map_tokens, semantic_search, embedding_model }
EmbeddingModel = { id, name, summary, bytes, license, sha256, url, dims, installed, active,
                   progress: {state: "downloading"|"verifying"|"installed"|"error", done, total, error}|null }
```

- `POST /api/code-intel/config {…some CodeIntelSettings fields}` → `{ok, config}`.
  Unknown or mistyped fields and out-of-range values are errors. Turning `lsp`
  off stops running servers.
- `POST /api/code-intel/install {package: "typescript"|"python"}` →
  `{ok, started, managed}`; the npm install runs in the background (poll
  status). `POST /api/code-intel/uninstall {package}` → `{ok, removed, managed}`.
- `POST /api/code-intel/servers/stop` → `{ok, stopped, embedding_server_stopped}`.
- `POST /api/code-intel/embeddings/install {model}` → `{ok, started, models}`;
  downloads in the background, verifies size and SHA-256, then makes the
  model active if none was chosen and embeds the project.
  `POST /api/code-intel/embeddings/remove {model}` → `{ok, removed, models}`.
- `POST /api/code-intel/reindex` → `{ok, index, embedding_started}`.
- `POST /api/code-intel/search {query, path?, max_hits?}` → the `search_code`
  result: `{ok, query, mode: "bm25"|"hybrid", count, hits: [{path, start_line,
  end_line, score, preview, symbols?, bm25?, similarity?}], semantic, note}`.
- `GET /api/code-intel/repo-map?tokens=&query=` → `{ok, map, files, symbols,
  tokens_estimate, focus, note}`.

Native tools: edit results (`write_file`, `edit_file`, `apply_patch`) may
carry `diagnostics: {new_errors: [{path, line, column, severity, message,
source?, code?}], checked: [path], servers?, pending?, unverified?,
unavailable?, truncated?, note}`. New tools `repo_map {query?, paths?,
max_tokens?}` and `search_code {query, path?, max_hits?}` are read-only.
`goto_definition` / `find_references` accept `path`, `line`, `column`
(1-based) and then answer `{ok, source: "lsp:<server>", count, truncated,
locations: [{path, line, column, preview}], note}`; without a position, or
when no server answers, they keep the tree-sitter shape (plus `lsp_note`).
`get_diagnostics {path}` answers `{ok, path, server, errors, total,
truncated, diagnostics: [...], note}` from the language server, or
`{ok: false, pending: true, error}` while it loads.

## Local models

`GET /api/local-models` → `LocalCatalog`:

```
{
  hardware: { cpu_cores, ram_bytes, gpu: string|null, vram_bytes: number|null,
              backend: "vulkan"|"cpu"|"unknown", devices: string[], detail: string },
  runtime:  { state: "ready"|"setup_required"|"unavailable", path, origin: "bundled"|"managed"|"other",
              version, backend, commit, detail },
  models: GgufEntry[],
  loaded:  { id, name, port, since, context_tokens, backend,
             cpu_fallback: boolean, fallback_reason: string|null,
             vision: boolean, in_use: number }|null,
  ollama_store: { path, available: boolean, models: [{ tag, path, projector, bytes, compatible, reason, already_added }] }
}
```

`runtime.state` is `ready` only after `<llama-server> --version` succeeded
(cached per binary path + size + mtime); `hardware` comes from the same
binary's `--list-devices` plus `/proc/meminfo` (no `nvidia-smi`).
Resolution order: `local_engine.llama_binary` (explicit), the bundled runtime
next to the executable when the managed dir is missing or older (COMMIT
`built=`), `~/.local/lib/shadowcode`, the bundle, `SHADOWCODE_LLAMA_SERVER`.
Never `llama-cli`, never a bare PATH lookup. `loaded` is `null` when read
through a route without the engine (e.g. `/api/accounts`).

`GgufEntry`:

```
{
  id: "local:gguf:<hash>", name, path, bytes, source: "file"|"directory"|"ollama",
  architecture: string|null, context_train: number|null, context_tokens: number,
  compatible: boolean, reason: string,
  vision: boolean, mmproj: string|null, tools: boolean, tools_reason: string,
  memory: { weights_bytes, kv_cache_bytes, compute_bytes, projector_bytes, overhead_bytes, total_bytes, context_tokens },
  fits: "gpu"|"cpu"|"no", availability: "ready"|"setup_required"|"unavailable",
  last_error: string|null,
  thinking_switch: boolean   // template has an enable_thinking switch (sent as false)
}
```

`context_tokens` is the one context number: the server's `--ctx-size` and the
engine's `context_limit` for that row (min(trained, `local_engine.context_size`
default 16384), halved until the memory estimate fits VRAM, else RAM; never
below 4096 unless the model is smaller). `tools` comes from the chat template
(`false` ⇒ "Chat only": no tool schemas are sent). `vision` is a paired
projector; after load it is what the server reports in `/props`
`modalities.vision`. Projector, vocabulary-only and embedding GGUFs are not
listed as models.

- `POST /api/local-models/add {path}` (file or folder) → `{ ok, local_engine }`;
  a projector/vocab/embedding file is refused with the reason.
- `POST /api/local-models/remove {id}` or `{path}` → `{ ok, deleted_weights: false, detail, local_engine }`;
  a file found through a folder is added to `local_engine.excluded`.
- `POST /api/local-models/import-ollama {tag, root?}` → adds the blob paths (never copies,
  never writes the store; `root` defaults to `OLLAMA_MODELS`, the systemd user unit's
  `Environment=OLLAMA_MODELS`, then `~/.ollama/models`) → `{ ok, local_engine }`.
  Incompatible tags are refused with the reason (e.g. `unsupported architecture gptoss`).
- `POST /api/local-models/load {id}` → `{ ok, loaded }` (starts llama-server; one at a time;
  refused with "Another task is using …" while a task holds the loaded model)
- `POST /api/local-models/unload` → `{ ok, unloaded: boolean }` (also aborts a load in
  progress; refused while a task holds the model)
- `POST /api/models/test {id: "local:gguf:…"}` loads and tests a local row.
- `POST /api/jobs` with a `local:gguf:` model (or default) and `images` on a row without
  vision is refused before a job is created.

Local picker rows carry `local: GgufEntry` and `availability_label`
`Ready | Setup required | Unavailable`; a loaded row's `reason` starts with
`Loaded ·` (and says `CPU fallback (GPU load failed)` when the GPU start failed).

The managed server binds 127.0.0.1 on a free port with `--no-webui --jinja
--parallel 1`, `--mmproj` when paired, `-ngl 999` when the plan fits the GPU
(one retry with `--device none -ngl 0`), and a fresh 32-byte hex key per
launch passed as `LLAMA_API_KEY` in its environment (never in argv, never
persisted). Clients use it as a bearer token and never go through a proxy.

Config (`config.yaml`, no main-UI control): `local_engine: { directories, files,
imports: [{path, mmproj, name, source}], excluded, llama_binary, context_size }`.

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
- `POST /api/jobs` body also takes `effort: "low"|"medium"|"high"` (omitted
  or `"default"`: the model's own setting; anything else is refused) and
  `mentions: [{path, kind: "file"|"dir"}]` (at most 20, inside the project).
  Effort maps to OpenRouter `reasoning.effort`, llama.cpp
  `chat_template_kwargs.enable_thinking` (false for `low`) on templates with
  the switch, Codex `-c model_reasoning_effort="…"` and Claude Code
  `MAX_THINKING_TOKENS` (4 000 / 16 000 / 31 999); other runtimes ignore it.
  Native models read each mentioned file's current text (64 KB each, 256 KB
  in all) or folder listing with the prompt; the stored prompt and
  `user.message` keep only the text (vendor CLIs resolve the `@path` in it).
  `purpose` is `coder` (Code), `planner` (Plan) or `reviewer` (Ask); Plan and
  Ask run read-only.
- `GET /api/workspace/mentions?q=&limit=30` (limit ≤ 100) →
  `{ items: [{path, kind: "file"|"dir"}], truncated }`: fuzzy matches (letters
  in order; file names, word starts and runs rank higher), skipping
  `.gitignore`d and hidden entries; an empty `q` lists the shallowest
  entries. The listing is cached for 5 s per project.
- `POST /api/sessions/{id}/fork {event_id, title?, before: true}` keeps only
  the events before the task `event_id` belongs to (its prompt included):
  Edit & resend forks there and sends the edited text. Before the first
  event the fork is an empty conversation with `parent_id` set.
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
  `checkpoint.restored {task_id, paths, undo_id?}` (the transcript shows a
  *Rewound to here · N files restored* divider above the task's prompt),
  `checkpoint.rewind_undone {task_id, paths, undo_id}`,
  `review.undone {task_id, path, hunk, whole}`,
  `approval.granted {tool, call_id|job_id, grant}` (allowed without a prompt
  by an earlier "Allow for this task"),
- `GET /api/sessions/{id}.execution_target` may already carry the workspace
  default for a conversation that has none of its own; the UI otherwise falls
  back to the last target chosen in that project (local cache) and never to
  `config.model`.
- `GET /api/onboarding` → `{ completed, suggested_workspace, levels,
  defaults: {permission_level, permission_mode, theme} }`. It probes no
  providers, local servers or vendor CLIs.
- `POST /api/onboarding` is sent without `provider`/`model` (the backend keeps
  its model configuration): `{ workspace, permission_level: "workspace",
  permission_mode: "ask" | "allow_edits", theme: "system" }`. The UI also
  writes `permissions.mode` with `PUT /api/config`.
- `/model` is handled by the window (opens the picker); it is never sent to
  `POST /api/commands/run`.
  `web.source {url, final_url, title, status}` (also embedded in `tool.completed.sources`).
- Read-only (Plan/Review) vendor tasks: command/file-change prompts from the
  vendor are denied automatically with an `agent.warning` ("Denied
  automatically: …"). Every vendor now sends permission requests
  (`asks_approval: true`, Antigravity through its ACP agent server since
  0.30.0). Antigravity questions sent through the permission channel
  (`interaction_` tool call ids) are cancelled with an `agent.warning` telling
  the user to answer in the next message.
- `model.stream_end {message_id, complete}` marks the end of a streamed reply.
  Vendor turns send it too (since 0.30.1), so the final result does not repeat
  a reply the transcript already shows.
- Events the UI also reads (optional): `routing.selected.inference`
  ("cloud" | "local", shown as "· Cloud" / "· This computer"),
  `model.switched {provider, from, to, resumed}`, `approval.requested` /
  `approval.resolved` (Waiting for approval step), `checkpoint.updated.paths`,
  `files.changed.paths` and `verification.summary {status, commands[{command,
  exit_code, success, timed_out}], presented_as, note, vendor_agent}` (summary
  card). The activity timeline classifies `tool.started/completed` by tool
  name: native names and vendor names such as `codex.command_execution`,
  `codex.file_change`, `cursor.read`, `Bash`, `Edit`.

## Usage, cost, retries and compaction (native loop)

Token and cost accounting, per model request ("turn"), per job and per
session. One shape, `Usage`, is used everywhere:

```ts
Usage = {
  prompt_tokens: number,       // input, including cached_tokens
  completion_tokens: number,   // output
  total_tokens: number,
  cached_tokens: number,       // input served from the provider's prompt cache
  cache_write_tokens: number,  // input written to the cache, when reported
  cost_usd: number|null,       // null = no cost known
  cost_estimated: boolean,     // cost from the price list, or some turns had none
  estimated: boolean,          // some token counts were estimated by ShadowCode
  source: "provider"|"local"|"vendor"|"mixed"|"",  // who reported the numbers
  turns: number,               // model requests / vendor turns counted
}
```

- Where cost comes from: OpenRouter's `usage.cost` (requested with
  `usage: {include: true}`); `0` for a model on this computer (llama.cpp,
  Ollama, loopback servers); OpenRouter's cached per-token prices when a turn
  reported no cost (`cost_estimated: true`; cached input is priced as full
  input, so it is an upper bound); otherwise `null`. Subscription CLI jobs
  carry what the vendor reports with `source: "vendor"`: token counts (Claude,
  Codex, ACP), cached input (Claude cache reads, Codex cached input) and
  Claude's `total_cost_usd` (the run's running total). A vendor that reports
  no counts gives `estimated: true` and zeros.
- `GET /api/jobs/{id}`: `usage: Usage` for the job so far (also in
  `result.usage` at the end). `usage_is_estimated` is kept for older readers.
- `GET /api/sessions/{id}` (with or without `?summary=true`): `usage: Usage`,
  the sum of the session's finished jobs (`usage_json` still holds the raw
  text). `GET /api/sessions/{id}/cost`: `{session_id, tasks[{task_id, prompt,
  status, usage}], usage, cost, cost_estimated, note}`.
- Event `usage.updated {purpose, turn, job, session}` after every counted
  request: `purpose` is `"turn"` (an agent step), `"compaction"` (a summary
  request) or `"vendor"` (a finished subscription CLI turn); `turn`, `job` and
  `session` are `Usage` (`session` includes the running job). The older
  `usage.updated {vendor, usage}` from a vendor's rate-limit push (Codex) still
  exists and has no `turn`; tell them apart by `vendor`.
- Event `model.retry {attempt, max_attempts, reason, status, delay_ms,
  retry_after, discard_message_id}`: a model request failed for a passing
  reason and will be re-sent after `delay_ms`. `reason` is `rate_limited`
  (429), `overloaded` (503/529), `server_error` (408, 425, 500, 502, 504,
  520–528), `stream_error` (the provider's error inside the stream),
  `disconnected` (the stream stopped before its finish marker, or the body
  failed), `stalled` (no bytes for 120 s) or `connect_failed`. `status` is the
  HTTP status or null; `retry_after: true` means the wait is the provider's
  `Retry-After`/`Retry-After-Ms`. `discard_message_id` names the streamed
  message of the failed attempt (partial text already shown) so the UI can
  drop or grey it; null when nothing was streamed. Local runtimes are retried
  only on 429/503. At most `agent.model_retries` retries (default 3, max 10);
  waits double from `agent.retry_backoff_sec` with jitter (capped at 30 s); a
  `Retry-After` over 120 s is not waited for. Tool calls run only after a
  complete response, so a retry never repeats a tool.
- Event `context.compacted {before_estimated_tokens, after_estimated_tokens,
  omitted_messages, response_token_limit, method, preserved, summary?,
  summary_model?, summary_ms?, fallback_reason?}`: `method` is
  `"model_summary"` when the current model summarized the removed messages
  (`summary` is the text, at most 6,000 bytes) or `"bounded_history"` for the
  built-in digest note. `fallback_reason` says why no summary was used:
  `disabled`, `context_too_small` (under 8,192 tokens), `offline_demo`,
  `timeout`, `empty_summary`, `summary_too_large`, or the request's error.
  The summary is kept in the conversation's message tape, so later turns of
  the session start from it.
- Prompt caching: OpenRouter requests to Claude models (`anthropic/…`) mark
  the system prompt and a rolling point at the newest and previous request end
  with `cache_control`; Gemini (`google/gemini…`) gets one mark on the system
  prompt. Other providers cache on their own; `cached_tokens` is recorded
  when reported (`prompt_tokens_details.cached_tokens`, DeepSeek's
  `prompt_cache_hit_tokens`).
- Tool descriptions: models with 32K+ context, or hosted models with 16K+,
  get the complete native tool descriptions; smaller ones get descriptions cut
  to 64 bytes.


## Subagents and agent definitions

See [SUBAGENTS.md](SUBAGENTS.md) for behaviour and definition files.

- `GET /api/agents?workspace=` → `{agents, shadowed, issues, dirs, settings, user_dir, workspace}`.
  `agents[]`: `{name, description, model, tools, deny, mode: "read-only"|"write",
  max_turns, source: "builtin"|"project"|"user", path, hash, ignored, instructions_preview}`.
  `settings` is the effective `subagents` config.
- `GET /api/subagents?session_id=` → `{runs}` (runs started from that
  conversation, oldest first); `GET /api/subagents/{run_id}` → one run
  `{id, agent, description, prompt, mode, model, parent_session, parent_task,
  parent_job, job_id, session_id, status, summary, error, files[{path, status,
  additions, deletions, binary}], files_truncated, binary_files, patch, applied,
  usage, steps, depth, notes, created_at, finished_at}`.
- `GET /api/sessions` hides subagent conversations unless
  `include_subagents=true`; rows carry `subagent_parent`.
  `GET /api/sessions/{id}` adds `subagent_parent` and `subagent_run`.
- Events (parent conversation): `subagent.started {run_id, agent, description,
  prompt, mode, model, job_id, session_id, depth}`, `subagent.finished {run_id,
  agent, description, mode, model, status, summary, error, job_id, session_id,
  files, files_truncated, binary_files, patch, usage, steps, notes, duration_s}`,
  `subagent.applied {run_id, agent, paths}`. `context.attached` gains
  `origin: "nested_guidance"`. `mcp.warning {server?, text}`.
- Native tools: `spawn_agent {agent?, prompt, description?, model?, write?}` or
  `{tasks: [...]}` (at most 8); `apply_agent_changes {run_id}` (runs as
  `apply_patch`); `load_skill {name}`; approved MCP tools as
  `mcp__<server>__<tool>` (run as `mcp_call`). A subagent's approvals carry the
  parent's `session_id` and a reason starting `Subagent <name>:`.
- Slash commands (`GET /api/commands`) include `.claude/commands/*.md`;
  `arg_spec` is the command's `argument-hint` when set.

## Approvals and jobs feed

The window no longer polls approvals and jobs. It reads one feed when the
engine says something changed, plus a 15 s backstop read.

- `GET /api/feed?session_id=&limit=100` →
  `{ approvals: Approval[], jobs: JobSummary[], events: string[] }`.
  `approvals` are the pending approvals of that conversation (all
  conversations without `session_id`), as `GET /api/approvals`; `jobs` are
  the same rows as `GET /api/jobs?view=summary&limit=100`; `events` lists
  the broadcast types after which the feed may have changed
  (`approval.requested`, `approval.resolved`, `job.changed`, `agent.started`,
  `agent.completed`, `agent.paused`, `agent.resumed`, `limit.fallback`).
- Engine broadcast `job.changed {job_id, status}` (with `session_id` and
  `task_id`): a job was queued or is being cancelled. It is a wake-up only,
  never stored and never in `/api/sessions/{id}/events`.
- The desktop shell's `shadowcode:events` wake-up now carries
  `{session_id, type}`. The window reads the feed for types in `events`, for
  `view.*` hints and for untyped wake-ups (a lagged or reattached stream), at
  most once per 30 ms burst. `GET /api/approvals` and `GET /api/jobs` are
  unchanged.
- `POST /api/workspace/diffstat {paths: string[]}` (at most 200) →
  `{ stats: {[path]: {add, del} | null} }`: added and removed lines per file,
  counted like the per-file diff (unstaged plus staged lines; every line of a
  new untracked file). `null` marks binary files, unreadable or symlinked new
  files, and every path outside a Git repository; a path without changes
  counts `{add: 0, del: 0}`. Keys are the paths as given (relative or
  absolute inside the project); paths outside the project or the project
  folder itself are rejected. Task summaries use it for all changed files in
  one request instead of one `GET /api/workspace/diff` per file.

### Approval answers, previews and task grants

- `Approval` records (feed, `GET /api/approvals`, `approval.requested`) add
  `preview`, `grant` and `note`:
  - `preview`: `{kind: "files", files: [{path, status: "added"|"modified"|"deleted",
    diff, added, removed, truncated, binary}]}` — `diff` is a unified diff body
    against the file as it is now (at most 2 000 lines per file, 256 KB per
    prompt; later files keep their counts with `truncated`); or
    `{kind: "command", command, cwd}`, `{kind: "move", from, to}`,
    `{kind: "folder", path}`; `null` when nothing can be shown. Vendor
    prompts carry previews where the protocol describes the change (Claude
    `Write`/`Edit`/`MultiEdit`, ACP `diff` content, Codex `applyPatchApproval`
    file changes; commands with their `cwd`).
  - `grant`: what "Allow for this task" covers ("file edits", "`cargo test`
    commands", "`server / tool` calls"), empty when the action is allowed
    once only (chained, redirected, privileged, deleting or history-rewriting
    commands; extra sandbox permissions).
  - `note`: a deny note reaches the agent (native tools and Claude Code).
- `POST /api/approvals/{id} {decision, session_id?, scope?: "once"|"task", note?}`.
  `scope: "task"` with `approve` keeps a grant until the task ends: later
  requests of the same task with the same scope (tool kind, or the same
  program and subcommand for commands) are allowed without a prompt and
  recorded as `approval.granted`. A scope the prompt does not offer is
  refused. For Codex the first grant answers `acceptForSession` /
  `approved_for_session`; other vendors receive single allows. `note`
  (with `deny`, at most 2 000 bytes) becomes the tool error the model reads
  ("The user denied this action and said: …") or Claude's denial message;
  `approval.resolved` carries `scope` and `note`.

### Per-task review and rewinds

- `GET /api/review/tasks/{task_id}` → `{task_id, session_id, workspace, busy,
  files: [{path, status: "added"|"modified"|"deleted"|"unchanged"|"unavailable",
  source: "checkpoint"|"git"|"none", added, removed, binary, error?}]}`: only
  the files this task changed, each compared with the checkpoint taken before
  the task's first write (`checkpoint`), or for paths a vendor CLI reported,
  with the last commit (`git`). `busy`: a task is queued or running in the
  project.
- `GET /api/review/tasks/{task_id}/file?path=` → the row plus `hash` and
  `hunks: [{id, header, old_start, old_len, new_start, new_len,
  lines: [{kind: "add"|"del"|"ctx", text, eol?: false}]}]`.
- `POST /api/review/tasks/{task_id}/undo {path, hunk?}` puts one hunk (by
  `id`), or without `hunk` the whole file, back as it was before the task,
  and answers the file's review. Refused while a task runs in the project,
  outside the open project, and when the file changed since the hunk was
  computed. The conversation's message tape gets a process note.
- `POST /api/checkpoints/tasks/{task_id}/restore` → `{ok, restored, undo_id}`:
  before writing, the files are recorded as they are (checkpoint rows of
  task `rewind:<undo_id>`, `native_meta` `rewind_undo:<undo_id>`).
  `POST /api/checkpoints/rewinds/{undo_id}/undo` → `{ok, restored, task_id}`
  puts them back once (refused if they changed since the rewind); the task
  can then be rewound again.
## Terminals

The drawer's interactive terminals: the user's login shell on a
pseudo-terminal, started in the selected project with the user's environment
(AppImage library paths removed, `TERM=xterm-256color`). They run outside the
sandbox, need no approval or trust, work while a task runs (no workspace
reservation), and are never stored or shown to a model. Terminals belong to
one desktop view: an attached window has its own, and they end when the view
or the app closes (`SIGHUP` to the shell's session, then `SIGKILL`). At most
12 per view.

- `GET /api/terminals` → `{ workspace, terminals: Terminal[], limits: {open,
  scrollback_bytes} }` for the selected project. `Terminal` is `{id (32 hex),
  title ("Terminal N"), number, workspace, shell, cols, rows, created, exited,
  exit_code, cursor}`.
- `POST /api/terminals {cols?, rows?}` → `Terminal` (a new shell).
- `POST /api/terminals/{id}/input {data}` → `{ok}`; at most 64 KB per call.
  Refused once the shell has exited.
- `POST /api/terminals/{id}/resize {cols, rows}` → `{cols, rows}` (clamped).
- `GET /api/terminals/{id}/output?after=<byte offset>` → `{id, data (base64
  bytes), from, cursor, more, truncated, exited, exit_code}`. Offsets count
  everything the terminal printed; the engine keeps the last 512 KB, so an
  older `after` starts at the oldest kept byte with `truncated: true`. At most
  256 KB per read; `more` asks for another.
- `POST /api/terminals/{id}/close` → `{ok}`.
- Engine broadcast `terminal.output {terminal_id}` / `terminal.exited
  {terminal_id}`: transient wake-ups (at most one per 16 ms per terminal,
  never stored, never carrying output). The desktop shell forwards them as the
  `shadowcode:terminal` event `{type, terminal_id}`, not as
  `shadowcode:events`; attached views receive `terminal_id` in their
  notifications.

## Git panel

Branches, suggested messages, push and pull requests for the selected
project. Staging and commits stay on `POST /api/workspace/git/add` and
`/commit`. Git runs with hooks disabled; push, `gh` and `glab` run with the
user's sign-in environment (SSH agent, askpass, credential helpers,
`GH_TOKEN`/`GITLAB_TOKEN` when set) and prompts disabled. No credential is
read or stored; remote URLs are reported without user-info; tool errors are
redacted.

- `GET /api/git?remote=` → `{repo, branch|null, detached, has_commits,
  upstream ("origin/x"|null), ahead, behind, staged, changed, branches:
  [{name, upstream, current}], remotes: [{name, info: RemoteInfo|null}],
  remote, remote_info, bases: string[], default_base}`; `{repo: false}`
  outside a repository. `RemoteInfo` is `{host, path ("owner/repo"), web_url,
  kind: "github"|"gitlab"|"other"}`. The remote is the requested one, else the
  branch's upstream remote, else `origin`, else the first.
- `POST /api/git/branch {name, create}` → `{ok, branch, created}`. Names are
  validated (no spaces, control characters, `~^:?*[\`, `..`, `@{`, `//`,
  leading `-` or `/`, trailing `/`, `.` or `.lock`, components starting with
  `.`), then `git check-ref-format --branch`. Needs a trusted, writable
  project and no running task.
- `POST /api/git/suggest {kind: "commit"|"pr", base?, remote?}` → `{kind,
  source: "model"|"local"|"summary", model, note, message}` for commits or
  `{…, title, body}` for pull requests. Commit drafts read the staged diff
  (an error when nothing is staged); PR drafts read the commits and diff since
  `base` (default: the remote's HEAD branch, else main/master/trunk/develop).
  `model` uses the conversation's picker target when ShadowCode runs it
  (local GGUF, API, OpenRouter; not subscriptions, and only loopback models
  when offline), `local` the loaded local model, `summary` a deterministic
  text. Secret-looking paths are listed but their contents are never sent,
  and the text is redacted before it leaves. 60 s limit; failures fall back
  to `summary` with a `note`.
- `POST /api/git/push {remote?}` → `{ok, remote, branch, output,
  remote_info}`. Pushes `refs/heads/<branch>` to the same name with
  `--set-upstream`, never forced; 180 s limit. Sign-in and rejected pushes
  answer with an explanation.
- `GET /api/git/pr?remote=&base=` → `{remote, provider, remote_info, cli:
  {name: "gh"|"glab"|null, installed, version, authenticated, detail,
  install_url, login_command}, base, compare_url, pr: {number, url, state,
  draft, title, base}|null}`. `cli.authenticated` comes from `gh|glab auth
  status --hostname <host>`; `compare_url` is the forge's new pull/merge
  request page for the current branch.
- `POST /api/git/pr {title, body, base, draft, remote?}` → `{ok, url, number,
  provider, pushed, branch, base, draft}`. Pushes first when the branch has
  no upstream or unpushed commits, then runs `gh pr create` (`glab mr
  create` for GitLab). Refused on the base branch itself and when the CLI is
  missing or signed out.
- `GET /api/git/pr/checks?number=&remote=` → `{supported, checks: [{name,
  workflow, state, bucket: "pass"|"fail"|"pending"|"skipping", link,
  description}], summary: {bucket: count}, overall:
  "pass"|"fail"|"pending"|"none", url, checked_at}` from `gh pr checks
  --json`. GitLab answers `supported: false` with the pipelines URL.

## Config

`GET/PUT /api/config`:

- `permissions.mode: "ask" | "allow_edits"` — the two user-facing modes.
  `ask` = shell and file edits ask; `allow_edits` = file edits inside the project
  are allowed, shell still asks. Vendor mapping is described in
  `permissions.vendor_notes: {[vendorId]: string}` returned with the config.
- `agent.model_retries` (0–10, default 3) and `agent.retry_backoff_sec`
  (0–30, default 1.0): model request retries (see above).
  `agent.summary_compaction` (default true) and `agent.summary_timeout_sec`
  (5–600, default 60): model-written compaction summaries.
- `network.mode: "online" | "web_off" | "offline"` — `web_off` disables web
  tools only; `offline` also suppresses account/usage refresh and any helper
  network activity. Cloud rows are marked unavailable in `offline`.
  The backend refuses jobs on cloud routes (vendor CLIs, non-loopback HTTP
  endpoints) in `offline` with `Offline mode: choose a model that runs on this
  computer`; a loopback endpoint or managed llama.cpp still runs.

Details (safety branch):

- `permissions.mode` defaults to `allow_edits` (the 0.27 behaviour: edits
  allowed, shell asks). Configs without a mode are migrated on load:
  `workspace` → `allow_edits`; `read_only` stays read-only (advanced level);
  `elevated` → `allow_edits` with `approve_shell: true`, unless the user had
  both `approve_shell: false` and `require_approval_for_dangerous: false`.
  `permissions.level` (`read_only | workspace | elevated`), `network`,
  `allow_root` and `approve_shell` remain as advanced fields.
- `PUT /api/config {values: {permissions: {mode}}}` also sets
  `approve_shell: true` unless the same request sets it.
- `permissions.vendor_notes` keys: `native`, `codex`, `claude`, `cursor`,
  `grok`, `antigravity`, `network`. Read-only (ignored on PUT).
- `network.allow_local_dev: string[]` — exact `host:port` entries
  (`localhost:3000`, `[::1]:8080`; an `http://` prefix is accepted) that web
  tools may reach although they are local or not on port 80/443. Invalid
  entries are rejected by PUT. `network.offline: boolean` is derived and
  returned with the config (ignored on PUT).
- Approvals in `ask` mode carry a readable `reason`: `Write <path>`,
  `Edit <path>`, `Create directory <path>`, `Move <a> to <b>`,
  `Delete <path>`, `Apply a patch to <files>`. Edits outside the project
  (including through symlinks) fail before any approval is requested.
- Privileged shell commands (sudo, su, pkexec, doas, run0) are denied, or asked
  when `allow_root`; never allowed silently. Destructive Git commands in the
  shell always ask. Network shell commands are denied offline.

### Shell sandbox and checkpoints (0.32)

- `sandbox: { require: bool = false, home_binds: string[], landlock: bool = true }`.
  `home_binds` are paths relative to the home folder, mounted read-only in
  the bubblewrap sandbox (default `.cargo .rustup .nvm .npm .cache/pip
  .local/bin .gitconfig .pyenv .bun .deno`). PUT rejects absolute paths, `..`,
  and anything equal to, inside or containing `.ssh .aws .gnupg .config
  .local/share .netrc .docker .kube .password-store .pki .azure .npmrc
  .pypirc .git-credentials .mozilla .var`. `require: true`: without
  bubblewrap, `exec` fails with "The command did not run: 'Require sandbox'
  is on …". `landlock`: without bubblewrap (and `require` off), commands
  run under Landlock when the kernel has it.
- `network.shell: "on" | "off" | "allowlist"` (default `on`) and
  `network.allow: string[]` (at most 128; `host`, `*.domain`, `host:port`,
  `[v6]:port`; without a port, 80 and 443). The effective shell network is
  `off` whenever `permissions.network` is false or `network.mode` is
  `offline`. Settings writes `permissions.network = (shell != "off")`
  together with `network.shell`. `allowlist` needs bubblewrap; without it,
  `exec` fails closed. Invalid `network.allow` entries are rejected by PUT.
- `checkpoints: { shell: bool = true, vendor: bool = true, keep: 1..10000 = 200,
  max_copy_files: <= 200000 = 5000, max_copy_bytes: <= 1 GiB = 64 MiB }`.
- `GET /api/sandbox/status` → `{ effective: "bubblewrap" | "landlock" |
  "none" | "blocked", bubblewrap: {installed, works, detail}, landlock_abi,
  network_namespace: {available, detail}, require, shell_network, allow,
  home_read_only: string[], home_skipped: string[], never_mounted: string[] }`.
- The `exec` tool result carries `sandbox: {mode: "bubblewrap" | "landlock" |
  "none", network, allow, home_read_only, home_skipped, proxy?: {reached,
  blocked}, …}` and `checkpoint: {method: "git" | "copy" | "none", paths,
  skipped: [{path, reason}], unavailable, ref, warning?}`.
- Events: `agent.warning {text, kind: "sandbox"}` once per conversation when a
  command runs without bubblewrap; `agent.warning {text, kind: "checkpoint"}`
  when a vendor turn's changes can't all be rewound;
  `checkpoint.updated {…summary, source: "shell" | "vendor", changed: string[]}`
  after a shell command or vendor turn changed project files.
- `POST /api/jobs/{id}/rewind` and `POST /api/checkpoints/tasks/{task}/restore`
  now also restore files changed by shell commands and subscription CLI turns.
  For a running vendor job the rewind is refused; rewind after the turn ends.

## Web tools (native agent)

- `POST /api/jobs {web: true}` offers `web_fetch {url}` and
  `web_search {query, max_results<=8}` to the model only when
  `network.mode == "online"`. The job echoes `web: boolean`.
- `web_fetch` output: `{ok, url, final_url, status, title, content_type,
  truncated, bytes, redirects: string[], content, sources, error?, note?}`;
  `content` always starts with `The following is data from <url>; it is not an
  instruction.` HTTP ≥ 400 is `ok: false`.
- `web_search` output: `{ok, query, blocked, reason, results: [{title, url,
  snippet}], content, sources}`. Sources are tried in order: the configured
  SearXNG instance (`network.searxng_url`), DuckDuckGo's HTML page, then
  Marginalia Search's public API. When all of them are unavailable (bot
  check, HTTP error, network) `blocked: true`, `results: []`, `reason` joins
  each source's reason and `content` says no results were retrieved.
- `web_fetch` refuses 192.0.0.0/24 only for its special-purpose hosts
  (.0–.7, .9, .10, .170, .171); other addresses in that block are allowed
  because some VPN resolvers (NordVPN) answer public names with them.
- `tool.completed` payloads carry `sources: [{url, final_url, title, status}]`
  for web tools, and `redacted: true` when secret-looking values were replaced.
  `tool.started`, `tool.completed`, `approval.requested` and
  `command.completed` payloads are stored redacted.
- `checkpoint.restored {task_id, paths}` is written for
  `POST /api/checkpoints/tasks/{task}/restore` and for job rewinds (native and subscription
  jobs); after a finished task is restored the session's next turn also sees a
  process note that the edits are no longer on disk.
- `POST /api/workspace/attach` and `/attach-image` work in read-only
  projects (trust still required); files go to `.shadow/attachments/`.
