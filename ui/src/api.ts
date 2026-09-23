import { request, ApiError } from "./lib/transport";
import type { PickerTarget, UsageSnapshot } from "./lib/picker";

// --- 0.28 contract: picker, accounts, local models (docs/API_CONTRACT_0.28.md)

export type VendorModel = {
  id: string;
  label: string;
  is_default?: boolean;
  vision?: boolean;
};

export type VendorStatus = {
  id: string;
  label: string;
  state: "ready" | "not_logged_in" | "not_installed" | "unavailable" | string;
  status?: string;
  availability: string;
  availability_label: string;
  detail?: string;
  version?: string | null;
  binary?: string | null;
  fix?: string | null;
  account?: {
    email?: string | null;
    plan?: string | null;
    auth_mode?: string | null;
  } | null;
  models?: VendorModel[];
  accepts_images?: boolean;
  asks_approval?: boolean;
  fetched_at?: number | null;
  error?: string | null;
  usage_note?: string | null;
  login_command?: string[];
  logout_command?: string[];
  shared_cli_note?: string;
  usage?: UsageSnapshot | null;
};

export type MemoryEstimate = {
  weights_bytes: number;
  kv_cache_bytes: number;
  compute_bytes?: number;
  projector_bytes: number;
  overhead_bytes?: number;
  total_bytes: number;
  context_tokens?: number;
};

export type GgufEntry = {
  id: string;
  name: string;
  path: string;
  bytes: number;
  source: "file" | "directory" | "ollama" | string;
  architecture: string | null;
  context_train: number | null;
  context_tokens: number;
  compatible: boolean;
  reason: string;
  vision: boolean;
  mmproj: string | null;
  tools: boolean;
  tools_reason?: string;
  memory?: MemoryEstimate | null;
  fits?: "gpu" | "cpu" | "no" | string;
  availability?: string;
  last_error?: string | null;
};

export type OllamaModel = {
  tag: string;
  path: string;
  projector: string | null;
  bytes: number;
  compatible: boolean;
  reason: string;
  already_added: boolean;
};

export type LocalCatalog = {
  hardware?: {
    cpu_cores?: number;
    ram_bytes?: number;
    gpu?: string | null;
    vram_bytes?: number | null;
    backend?: "vulkan" | "cpu" | "unknown" | string;
    devices?: string[];
    detail?: string;
  };
  runtime?: {
    state: "ready" | "setup_required" | "unavailable" | string;
    path?: string | null;
    origin?: string;
    version?: string | null;
    backend?: string | null;
    commit?: string | null;
    detail?: string;
  };
  models?: GgufEntry[];
  loaded?: {
    id: string;
    name: string;
    port?: number;
    since?: number;
    context_tokens?: number;
    backend?: string;
  } | null;
  ollama_store?: {
    path?: string | null;
    available: boolean;
    models: OllamaModel[];
  };
};

export type PickerResponse = {
  targets: PickerTarget[];
  local_engine?: LocalCatalog;
  vendors?: Record<string, VendorStatus>;
  generated_at?: number;
};

export type AccountsResponse = {
  vendors: Record<string, VendorStatus>;
  config?: Record<string, unknown>;
  local_engine?: LocalCatalog;
};

export type LoginProgress = {
  running: boolean;
  lines: string[];
  done: { ok: boolean; detail?: string } | null;
};

export type ConsentRequest = {
  error?: string;
  needs_consent: true;
  handoff: {
    from?: string | null;
    to?: string | null;
    excerpt_chars?: number;
    images?: number;
  };
};

export type StartJobRequest = {
  task: string;
  workspace?: string;
  session_id?: string;
  model?: string;
  purpose?: string;
  queue?: boolean;
  images?: string[];
  web?: boolean;
  handoff_consent?: boolean;
};

function consentFrom(value: unknown): ConsentRequest | null {
  if (
    value &&
    typeof value === "object" &&
    (value as { needs_consent?: unknown }).needs_consent === true
  ) {
    const body = value as ConsentRequest;
    return { ...body, handoff: body.handoff || {} };
  }
  return null;
}

export type Project = {
  id: string;
  path: string;
  name: string;
  last_opened: number;
};
export type Session = {
  id: string;
  workspace: string;
  status: string;
  title?: string;
  updated_at: number;
  usage_json?: string;
  parent_id?: string;
};
export type SessionDetail = Session & {
  tasks: { id: string; prompt: string; summary?: string; status: string }[];
  events: EventRow[];
  event_cursor: number;
  history_page?: { first_cursor: number; has_older: boolean };
  /** Picker id remembered for this conversation (POST /api/sessions/{id}/target). */
  execution_target?: string | null;
  native_sessions?: Record<string, string>;
};
export type HistoryPage = {
  events: EventRow[];
  first_cursor: number;
  event_cursor: number;
  has_older: boolean;
};
export type EventRow = {
  id?: number;
  ts: number;
  type: string;
  payload: Record<string, unknown>;
  session_id?: string;
  task_id?: string;
};
export type ProjectSkill = {
  name: string;
  content: string;
  raw_content?: string;
  description?: string;
  path?: string;
  mode?: string;
  hash?: string;
};
export type FileEntry = { name: string; path: string; type: "file" | "dir" };
export type PlanStep = {
  id: string;
  title: string;
  status: string;
  detail?: string;
};
export type Approval = {
  id: string;
  session_id?: string;
  command?: string;
  reason?: string;
  tool?: string;
  pending?: boolean;
};
export type LifecycleHook = {
  name: string;
  events: string[];
  builtin: boolean;
  command?: string;
  description?: string;
  path?: string;
  hash?: string;
  enabled?: boolean;
  timeout_sec?: number;
  path_suffix?: string;
};
export type HookCatalog = {
  hooks: LifecycleHook[];
  dirs: string[];
  issues?: string[];
  format?: string;
  workspace?: string;
  trusted?: boolean;
  approved?: { path: string; hash: string }[];
};
export type ManagedWorktree = {
  id: string;
  source: string;
  path: string;
  branch: string;
  base_commit: string;
  common_directory: string;
  state: string;
  created_at: number;
  detail: string;
};
export type WorktreeRepairReview = {
  record: ManagedWorktree;
  administrative_directory: string;
  head: string;
  checkout_pointer: string | null;
  registration_pointer: string | null;
  warning: string;
  hash: string;
};
export type WorktreeCopyReview = {
  source: string;
  head: string;
  staged_diff: string;
  unstaged_diff: string;
  untracked: { path: string; bytes: number; hash: string; mode: number }[];
  intent_to_add: string[];
  hash: string;
};
export type WorktreeReturnReview = {
  record: ManagedWorktree;
  source_head: string;
  source_branch: string;
  worktree_head: string;
  worktree_branch: string;
  merge_base: string;
  diff: string;
  hash: string;
};
export type WorktreeRecovery = {
  record: ManagedWorktree;
  commit: string;
  branch: string;
  warning: string;
  hash: string;
};
export type WorktreeInspection = {
  record: ManagedWorktree;
  head: string;
  current_branch: string;
  status: string;
  can_remove: boolean;
  reason: string;
  hash: string;
};
export type Job = {
  id: string;
  task_id?: string;
  workspace: string;
  event_cursor: number;
  started_at: number;
  finished_at?: number;
  session_id: string;
  status: string;
  summary?: string;
  task?: string;
  task_truncated?: boolean;
  purpose?: string;
  usage?: Record<string, number>;
  model?: string;
  mode?: string;
  routing?: RoutingDecision | null;
  web?: boolean;
  result?: {
    success: boolean;
    summary: string;
    plan: { goal: string; steps: PlanStep[] };
    usage?: Record<string, number>;
    verification?: Record<string, unknown>;
  };
};
export type Health = {
  ok: boolean;
  desktop_attached?: boolean;
  version?: string;
  workspace: string;
  trusted?: boolean;
  permissions?: Record<string, unknown>;
  provider?: { ok: boolean; name: string; detail: string };
  tools?: Record<string, { ok: boolean; path?: string; detail?: string }>;
  model?: Record<string, unknown>;
  onboarding?: { completed: boolean };
};
export type DiffHunk = {
  header: string;
  lines: { kind: string; text: string }[];
};
export type ExecResult = {
  ok: boolean;
  command: string;
  stdout: string;
  stderr: string;
  exit_code: number;
  error?: string;
};
export type Milestone = {
  id: string;
  title: string;
  status: string;
  detail?: string;
  task_id?: string;
  require_verification?: boolean;
  mode?: string;
};
export type Goal = {
  id: string;
  workspace: string;
  instruction: string;
  title?: string;
  status: string;
  progress: number;
  progress_pct: number;
  running: boolean;
  milestones: Milestone[];
  updated_at: number;
  session_id?: string;
  job_id?: string;
  run_detail?: string;
};
export type BackgroundTask = {
  id: string;
  name: string;
  command: string;
  status: string;
  pid: number;
  exit_code: number | null;
  output: string;
  cwd?: string;
  started_at?: number;
  ended_at?: number | null;
  error?: string;
  truncated?: boolean;
  output_preview_truncated?: boolean;
};
export type RoutingDecision = {
  purpose: string;
  source: string;
  requested: string;
  model_id: string;
  model_name: string;
  provider: string;
  context_limit: number;
  fallback_reason?: string | null;
  inference?: string;
};
export type DoctorReport = {
  ok: boolean;
  version: string;
  checks: {
    id: string;
    ok: boolean;
    status?: "pass" | "warn" | "fail" | "info" | "not_checked";
    label: string;
    detail?: string;
    fix?: string;
  }[];
  suggestions: string[];
};
export type McpServer = {
  name: string;
  command?: string[] | null;
  url?: string | null;
};
export type NativeMcpServer = McpServer & {
  api_key_env?: string | null;
  id: string;
  hash: string;
  description: string;
  timeout_sec: number;
  env_names: string[];
  env_refs: Record<string, string>;
  enabled: boolean;
  transport: "stdio" | "http";
};
export type NativeMcpCatalog = {
  format: "native-mcp-v1";
  servers: NativeMcpServer[];
  approved: { workspace: string; server: string; hash: string }[];
  issues: string[];
  workspace: string;
  trusted: boolean;
  dirs: string[];
};

const get = <T>(path: string) => request<T>(path);

async function send<T>(
  path: string,
  method: string,
  body?: unknown,
): Promise<T> {
  return request<T>(path, method, body);
}

export type ParallelPlan = {
  id: string;
  goal: string;
  source: string;
  lead_note: string;
  verify_status: string;
  workers: {
    item: { id: string; title: string; prompt: string };
    worktree_path: string;
    branch: string;
    status: string;
  }[];
};
export type GuardianStatus = {
  enabled: boolean;
  last_run?: number;
  last_result?: { tests: { hint?: string; executed: boolean } };
};
export const api = {
  parallelPlan: () => get<{ plan: ParallelPlan | null }>("/api/parallel"),
  prepareParallel: (goal: string) =>
    send<{ ok: boolean; error?: string; plan?: ParallelPlan }>(
      "/api/parallel/prepare",
      "POST",
      { goal },
    ),
  parallelWorkerStatus: (worker_id: string, status: string) =>
    send("/api/parallel/worker-status", "POST", { worker_id, status }),
  verifyParallel: () =>
    send<{ ok: boolean; conflicts?: { worker: string; detail: string }[] }>(
      "/api/parallel/verify",
      "POST",
      {},
    ),
  cleanupParallel: () =>
    send<{ cleaned: number }>("/api/parallel/cleanup", "POST", {}),
  guardianStatus: () => get<GuardianStatus>("/api/guardian"),
  runGuardian: () => send("/api/guardian/run", "POST", {}),
  health: () => get<Health>("/api/health"),
  onboarding: () =>
    get<{
      completed: boolean;
      suggested_workspace: string;
    }>("/api/onboarding"),
  completeOnboarding: (body: Record<string, unknown>) =>
    send<{ ok: boolean; workspace: string; session_id: string }>(
      "/api/onboarding",
      "POST",
      body,
    ),
  config: () => get<Record<string, unknown>>("/api/config"),
  saveConfig: (
    values: Record<string, unknown>,
    api_key = "",
    api_key_env = "",
  ) =>
    send<Record<string, unknown>>("/api/config", "PUT", {
      values,
      api_key,
      api_key_env,
    }),
  /** The composer's only source of rows (vendor + local). */
  picker: (refresh = false) =>
    get<PickerResponse>(`/api/picker${refresh ? "?refresh=1" : ""}`),
  accounts: (refresh = false) =>
    get<AccountsResponse>(`/api/accounts${refresh ? "?refresh=1" : ""}`),
  connectAccount: (vendor: string) =>
    send<{
      ok: boolean;
      state: "started" | "unsupported" | "already_running" | string;
      note?: string;
      hint?: string;
      lines?: string[];
    }>(`/api/accounts/${encodeURIComponent(vendor)}/connect`, "POST", {}),
  /** Contract extension: buffered login output for a running Connect. */
  loginProgress: async (vendor: string): Promise<LoginProgress> => {
    const raw = await get<{
      running: boolean;
      lines?: (string | { line?: string })[];
      done: { ok: boolean; detail?: string } | null;
    }>(`/api/accounts/${encodeURIComponent(vendor)}/login`);
    // The engine sends {vendor, line, url} records; the page shows text lines.
    return {
      running: raw.running,
      done: raw.done,
      lines: (raw.lines || []).map((entry) =>
        typeof entry === "string" ? entry : String(entry.line ?? ""),
      ),
    };
  },
  cancelLogin: (vendor: string) =>
    send<{ ok: boolean }>(
      `/api/accounts/${encodeURIComponent(vendor)}/cancel-login`,
      "POST",
      {},
    ),
  disconnectAccount: (vendor: string) =>
    send<{ ok: boolean; ran?: string[]; note?: string }>(
      `/api/accounts/${encodeURIComponent(vendor)}/disconnect`,
      "POST",
      { confirm: true },
    ),
  refreshAccount: (vendor: string) =>
    send<VendorStatus>(
      `/api/accounts/${encodeURIComponent(vendor)}/refresh`,
      "POST",
      {},
    ),
  localModels: () => get<LocalCatalog>("/api/local-models"),
  addLocalModel: (path: string) =>
    send<{ ok: boolean; local_engine?: LocalCatalog }>(
      "/api/local-models/add",
      "POST",
      { path },
    ),
  removeLocalModel: (path: string) =>
    send<{ ok: boolean; deleted_weights: boolean; detail?: string }>(
      "/api/local-models/remove",
      "POST",
      { path },
    ),
  importOllama: (tag: string) =>
    send<{ ok: boolean; local_engine?: LocalCatalog }>(
      "/api/local-models/import-ollama",
      "POST",
      { tag },
    ),
  loadLocalModel: (id: string) =>
    send<{ ok: boolean; loaded?: LocalCatalog["loaded"] }>(
      "/api/local-models/load",
      "POST",
      { id },
    ),
  unloadLocalModel: () =>
    send<{ ok: boolean }>("/api/local-models/unload", "POST", {}),
  setSessionTarget: (sessionId: string, targetId: string) =>
    send<{ ok: boolean }>(
      `/api/sessions/${encodeURIComponent(sessionId)}/target`,
      "POST",
      { target_id: targetId },
    ),
  projects: () => get<{ projects: Project[] }>("/api/projects"),
  openProject: (path: string) =>
    send<{
      path: string;
      session_id: string;
      needs_trust?: boolean;
      name?: string;
      permissions?: Record<string, unknown>;
    }>("/api/projects", "POST", { path }),
  trustProject: (path: string) =>
    send<{ ok: boolean; path: string; session_id: string }>(
      "/api/projects/trust",
      "POST",
      { path },
    ),
  sessions: (q = "") =>
    get<{ sessions: Session[] }>(
      `/api/sessions${q ? `?q=${encodeURIComponent(q)}` : ""}`,
    ),
  renameSession: (id: string, title: string) =>
    send<{ ok: boolean }>(`/api/sessions/${id}`, "PATCH", {
      workspace: "",
      title,
    }),
  deleteSession: (id: string) =>
    send<{ ok: boolean }>(`/api/sessions/${id}`, "DELETE"),
  doctor: () => get<DoctorReport>("/api/doctor"),
  goals: () => get<{ goals: Goal[] }>("/api/goals"),
  createGoal: (instruction: string, run: boolean, session_id?: string) =>
    send<Goal>("/api/goals", "POST", {
      instruction,
      run,
      session_id: session_id || null,
    }),
  runGoal: (id: string, session_id?: string) =>
    send<Goal>(`/api/goals/${id}/run`, "POST", {
      session_id: session_id || null,
    }),
  abandonGoal: (id: string) =>
    send<Goal>(`/api/goals/${id}/abandon`, "POST", {}),
  pauseGoal: (id: string) => send<Goal>(`/api/goals/${id}/pause`, "POST", {}),
  deleteGoal: (id: string) =>
    send<{ ok: boolean }>(`/api/goals/${id}`, "DELETE"),
  setMilestone: (goalId: string, milestoneId: string, status: string) =>
    send<Goal>(`/api/goals/${goalId}/milestones/${milestoneId}`, "POST", {
      status,
      detail: "",
    }),
  background: () => get<{ tasks: BackgroundTask[] }>("/api/background"),
  backgroundTask: (id: string) => get<BackgroundTask>(`/api/background/${id}`),
  startBackground: (name: string, command: string) =>
    send<BackgroundTask>("/api/background", "POST", { name, command }),
  stopBackground: (id: string) =>
    send<BackgroundTask>(`/api/background/${id}/stop`, "POST", {}),
  hooks: () => get<HookCatalog>("/api/hooks"),
  activateHook: (
    workspace: string,
    path: string,
    hash: string,
    enabled: boolean,
  ) =>
    send<HookCatalog>("/api/hooks/activation", "POST", {
      workspace,
      path,
      hash,
      enabled,
    }),
  mcpServers: () => get<NativeMcpCatalog>("/api/mcp/servers"),
  registerMcp: (definition: Record<string, unknown>, hash = "") =>
    send<NativeMcpCatalog>("/api/mcp/servers", "POST", { definition, hash }),
  activateMcp: (
    workspace: string,
    server: string,
    hash: string,
    enabled: boolean,
  ) =>
    send<NativeMcpCatalog>("/api/mcp/activation", "POST", {
      workspace,
      server,
      hash,
      enabled,
    }),
  removeMcp: (server: string, hash: string) =>
    send<NativeMcpCatalog>("/api/mcp/servers/delete", "POST", { server, hash }),
  worktrees: () =>
    get<{ workspace: string; worktrees: ManagedWorktree[] }>("/api/worktrees"),
  createWorktree: (workspace: string, reference: string) =>
    send<ManagedWorktree>("/api/worktrees", "POST", { workspace, reference }),
  inspectWorktree: (workspace: string, id: string) =>
    send<WorktreeInspection>("/api/worktrees/inspect", "POST", {
      workspace,
      id,
    }),
  reviewWorktreeRepair: (workspace: string, id: string) =>
    send<WorktreeRepairReview>("/api/worktrees/review-repair", "POST", {
      workspace,
      id,
    }),
  repairWorktree: (workspace: string, id: string, hash: string) =>
    send<ManagedWorktree>("/api/worktrees/repair", "POST", {
      workspace,
      id,
      hash,
    }),
  reviewWorktreeCopy: (workspace: string) =>
    send<WorktreeCopyReview>("/api/worktrees/review-changes", "POST", {
      workspace,
    }),
  copyWorktreeChanges: (workspace: string, hash: string) =>
    send<ManagedWorktree>("/api/worktrees/copy-changes", "POST", {
      workspace,
      hash,
    }),
  reviewWorktreeReturn: (workspace: string, id: string) =>
    send<WorktreeReturnReview>("/api/worktrees/review-return", "POST", {
      workspace,
      id,
    }),
  returnWorktreeChanges: (workspace: string, id: string, hash: string) =>
    send<ManagedWorktree>("/api/worktrees/return", "POST", {
      workspace,
      id,
      hash,
    }),
  worktreeRecovery: (workspace: string, id: string) =>
    send<WorktreeRecovery>("/api/worktrees/recovery", "POST", {
      workspace,
      id,
    }),
  restoreWorktree: (workspace: string, id: string, hash: string) =>
    send<ManagedWorktree>("/api/worktrees/restore", "POST", {
      workspace,
      id,
      hash,
    }),
  removeWorktree: (workspace: string, id: string, hash: string) =>
    send<ManagedWorktree>("/api/worktrees/remove", "POST", {
      workspace,
      id,
      hash,
    }),
  plugins: () => get<NativePluginCatalog>("/api/plugins"),
  previewPlugin: (source: { name: string } | { bundle: unknown }) =>
    send<PluginPreview>("/api/plugins/preview", "POST", source),
  installNativePlugin: (workspace: string, preview: PluginPreview) =>
    send<PluginChange>("/api/plugins/install", "POST", {
      workspace,
      bundle: preview.bundle,
      hash: preview.hash,
    }),
  removeNativePlugin: (workspace: string, name: string, hash: string) =>
    send<PluginChange>("/api/plugins/remove", "POST", {
      workspace,
      name,
      hash,
    }),
  taskCheckpoint: (taskId: string) =>
    get<{
      rewindable: boolean;
      checkpoint: { changes: number; paths: string[] } | null;
    }>(`/api/checkpoints/tasks/${taskId}`),
  forkSession: (id: string, eventId: number, title?: string) =>
    send<{
      fork: { id: string; title: string };
      original: { id: string };
      original_intact: boolean;
      forked_from_event: number;
    }>(`/api/sessions/${id}/fork`, "POST", {
      event_id: eventId,
      title: title || "",
    }),
  rewindTask: (taskId: string) =>
    send<{ ok: boolean; restored: string[] }>(
      `/api/checkpoints/tasks/${taskId}/restore`,
      "POST",
      {},
    ),
  historyPage: (id: string, before: number) =>
    get<HistoryPage>(`/api/sessions/${id}/events?view=window&before=${before}`),
  session: (id: string) =>
    get<SessionDetail>(`/api/sessions/${id}?view=window`),
  activateSession: (id: string) =>
    send<SessionDetail>(`/api/sessions/${id}/activate?view=window`, "POST", {}),
  currentJob: (id: string) =>
    get<{ job: Job | null }>(
      `/api/jobs/current?session_id=${encodeURIComponent(id)}&include_finished=true`,
    ),
  jobs: () => get<{ jobs: Job[] }>("/api/jobs?view=summary&limit=100"),
  createSession: (workspace: string, title = "") =>
    send<{ id: string; workspace: string }>("/api/sessions", "POST", {
      workspace,
      title,
    }),
  files: (path = ".") =>
    get<{
      entries: FileEntry[];
      workspace: string;
      path: string;
      parent: string;
    }>("/api/workspace/files?path=" + encodeURIComponent(path)),
  file: (path: string) =>
    get<{ content: string; path: string }>(
      "/api/workspace/file?path=" + encodeURIComponent(path),
    ),
  git: () =>
    get<{
      status: string;
      log: string;
      diff: string;
      porcelain?: string;
      files?: { path: string; label: string }[];
      repo?: boolean;
    }>("/api/workspace/git"),
  gitDiff: (path = "") =>
    get<{
      diff: string;
      staged: string;
      hunks: DiffHunk[];
      staged_hunks: DiffHunk[];
      untracked: boolean;
      binary: boolean;
      truncated: boolean;
    }>("/api/workspace/diff?path=" + encodeURIComponent(path)),
  gitAdd: (paths: string[]) =>
    send<{ ok: boolean }>("/api/workspace/git/add", "POST", {
      message: "",
      paths,
    }),
  gitCommit: (message: string, paths: string[] = []) =>
    send<{ ok: boolean }>("/api/workspace/git/commit", "POST", {
      message,
      paths,
    }),
  hunkAction: (path: string, hunk: DiffHunk, action: "accept" | "reject") =>
    send<{ ok: boolean; action: string; path: string }>(
      "/api/workspace/diff/hunk",
      "POST",
      { path, hunk, action },
    ),
  exec: (command: string, timeout = 60) =>
    send<ExecResult>("/api/workspace/exec", "POST", { command, timeout }),
  status: () =>
    get<{
      workspace: string;
      model: {
        default: string;
        provider: string;
        endpoint?: string;
        name?: string;
        api_key_env?: string;
        context_limit?: number;
      };
      permissions: { level: string; network?: boolean; mode?: string };
      onboarding?: { completed: boolean };
      routing?: Record<string, string | boolean>;
      trusted?: boolean;
    }>("/api/workspace/status"),
  /** Starts (or queues) a task. A cloud route that needs the user's consent
   * answers `needs_consent` (HTTP 409 or an IPC value); nothing is created
   * until the request is repeated with `handoff_consent: true`. */
  startJob: async (
    body: StartJobRequest,
  ): Promise<{ job: Job } | { consent: ConsentRequest }> => {
    try {
      const value = await send<Job | ConsentRequest>("/api/jobs", "POST", {
        purpose: "coder",
        ...body,
        images: body.images?.length ? body.images : undefined,
      });
      const consent = consentFrom(value);
      return consent ? { consent } : { job: value as Job };
    } catch (error) {
      const consent =
        error instanceof ApiError ? consentFrom(error.body) : null;
      if (consent) return { consent };
      throw error;
    }
  },
  job: (id: string) => get<Job>(`/api/jobs/${id}`),
  cancelJob: (id: string, only_if_queued = false) =>
    send<Job>(`/api/jobs/${id}/cancel`, "POST", { only_if_queued }),
  pauseJob: (id: string) => send<Job>(`/api/jobs/${id}/pause`, "POST", {}),
  resumeJob: (id: string) => send<Job>(`/api/jobs/${id}/resume`, "POST", {}),
  steerJob: (id: string, instruction: string, path?: string) =>
    send<Job>(`/api/jobs/${id}/steer`, "POST", {
      instruction,
      ...(path ? { path } : {}),
    }),
  noteJobEdit: (id: string, path: string, detail = "") =>
    send<Job>(`/api/jobs/${id}/note_edit`, "POST", { path, detail }),
  rewindJob: (id: string) =>
    send<{
      ok: boolean;
      restored: string[];
      note?: string;
    }>(`/api/jobs/${id}/rewind`, "POST", {}),
  approvals: (sessionId?: string) =>
    get<{ approvals: Approval[] }>(
      `/api/approvals${sessionId ? `?session_id=${encodeURIComponent(sessionId)}` : ""}`,
    ),
  decide: (id: string, decision: "approve" | "deny", sessionId?: string) =>
    send<Approval>(`/api/approvals/${id}`, "POST", {
      decision,
      session_id: sessionId,
    }),
  instructions: () =>
    get<{ content: string; exists: boolean }>("/api/workspace/instructions"),
  saveInstructions: (content: string) =>
    send<{ ok: boolean }>("/api/workspace/instructions", "PUT", { content }),
  skills: () =>
    get<{ skills: ProjectSkill[]; issues?: string[] }>("/api/workspace/skills"),
  saveSkill: (name: string, content: string, expectedHash?: string) =>
    send<{ ok: boolean }>("/api/workspace/skills", "PUT", {
      name,
      content,
      expected_hash: expectedHash,
    }),
  attach: (filename: string, text: string) =>
    send<{ path: string; kind?: string }>("/api/workspace/attach", "POST", {
      filename,
      text,
    }),
  attachImage: (filename: string, data_base64: string) =>
    send<{ path: string; mime: string; bytes: number; kind: string }>(
      "/api/workspace/attach-image",
      "POST",
      { filename, data_base64 },
    ),
  branchSession: (sessionId: string, title = "") =>
    send<{ id: string; parent_id: string }>(
      `/api/sessions/${sessionId}/branch`,
      "POST",
      { workspace: "", title },
    ),
  commands: () =>
    get<{
      commands: {
        name: string;
        description: string;
        arg_spec: string;
        alias: string;
        source: string;
      }[];
    }>("/api/commands"),
  runCommand: (
    name: string,
    args = "",
    sessionId?: string,
    options: { model?: string; purpose?: string; queue?: boolean } = {},
  ) =>
    send<CommandResult>("/api/commands/run", "POST", {
      name,
      args,
      session_id: sessionId,
      ...options,
    }),
};

export type CommandResult = {
  handled: boolean;
  text: string;
  kind:
    | "text"
    | "card"
    | "list"
    | "diff"
    | "approval"
    | "error"
    | "overlay"
    | "quit";
  icon: string;
  headline: string;
  body: string;
  items: { label: string; value: string }[];
  diff: string;
  path: string;
  approval_action: string;
  approval_reason: string;
  overlay: string;
  quit: boolean;
  passthrough: boolean;
  metadata: Record<string, unknown>;
};

export type PluginFile = {
  path: string;
  kind: string;
  hash: string;
  content?: string;
  status?: "unchanged" | "modified" | "missing" | "unreadable";
  error?: string | null;
};
export type PluginEntry = {
  name: string;
  version: string;
  description: string;
  hash: string;
  state?: "prepared" | "installed" | "removing";
  files: PluginFile[];
};
export type NativePluginCatalog = {
  format: "native-plugins-v1";
  workspace: string;
  trusted: boolean;
  read_only: boolean;
  installed: PluginEntry[];
  available: PluginEntry[];
  issues: string[];
  legacy: string[];
};
export type PluginPreview = {
  bundle: { name: string; version: string; description: string };
  hash: string;
  files: PluginFile[];
};
export type PluginChange = {
  catalog: NativePluginCatalog;
  result: { name: string; retained?: { path: string; reason: string }[] };
};
