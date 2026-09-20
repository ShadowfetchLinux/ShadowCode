import { request } from "./lib/transport";

export type ModelInfo = {
  id: string;
  name: string;
  provider: string;
  endpoint: string;
  context_limit?: number;
  detected?: boolean;
  metadata?: Record<string, unknown> & {
    capabilities?: Record<string, boolean>;
    detail?: string;
    detected?: boolean;
  };
};

export type DetectedModel = {
  id: string;
  name: string;
  size_bytes: number;
  context_limit: number;
  capabilities: Record<string, boolean>;
  detail: string;
};

export type DetectedProvider = {
  provider: string;
  label: string;
  endpoint: string;
  running: boolean;
  latency_ms: number;
  models: DetectedModel[];
  detail: string;
};

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
export type Job = {
  id: string;
  workspace: string;
  event_cursor: number;
  started_at: number;
  finished_at?: number;
  session_id: string;
  status: string;
  summary?: string;
  task?: string;
  usage?: Record<string, number>;
  model?: string;
  routing?: RoutingDecision | null;
  result?: {
    success: boolean;
    summary: string;
    plan: { goal: string; steps: PlanStep[] };
    usage?: Record<string, number>;
  };
};
export type Health = {
  ok: boolean;
  version?: string;
  workspace: string;
  provider?: { ok: boolean; name: string; detail: string };
  tools?: Record<string, { ok: boolean; path?: string; detail?: string }>;
  model?: Record<string, unknown>;
  onboarding?: { completed: boolean };
};
export type DiffHunk = {
  header: string;
  lines: { kind: string; text: string }[];
};
export type ModelTestResult = {
  ok: boolean;
  latency_ms?: number;
  reply?: string;
  error?: string;
  model?: string;
  usage?: Record<string, number>;
  capabilities?: Record<string, unknown>;
};
export type ExecResult = {
  ok: boolean;
  command: string;
  stdout: string;
  stderr: string;
  exit_code: number;
  error?: string;
};
export type ProviderInfo = {
  id: string;
  label: string;
  endpoint: string;
  api_key_env: string;
  needs_key: boolean;
  local: boolean;
  running: boolean;
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
};
export type RoutingView = {
  enabled: boolean;
  default: string;
  table: Record<string, string>;
  config: Record<string, string | boolean>;
  default_name?: string;
  decisions?: Record<string, RoutingDecision>;
  models?: ModelInfo[];
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
export type McpCatalog =
  NativeMcpCatalog | { format?: undefined; servers: McpServer[] };
export type UpdateInfo = {
  current: string;
  latest: string;
  tag: string;
  update_available: boolean;
  url: string;
  source: string;
  error: string;
};

const get = <T>(path: string) => request<T>(path);

async function send<T>(
  path: string,
  method: string,
  body?: unknown,
): Promise<T> {
  return request<T>(path, method, body);
}

export const api = {
  health: () => get<Health>("/api/health"),
  onboarding: () =>
    get<{
      completed: boolean;
      suggested_workspace: string;
      providers: {
        id: string;
        provider: string;
        needs_key?: boolean;
        endpoint?: string;
        api_key_env?: string;
        name?: string;
      }[];
      detected: DetectedProvider[];
      levels: string[];
      defaults: { provider: string; permission_level: string; theme: string };
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
  models: (refresh = false) =>
    get<{ models: ModelInfo[] }>(`/api/models${refresh ? "?refresh=1" : ""}`),
  detectProviders: (refresh = false) =>
    get<{ providers: DetectedProvider[] }>(
      `/api/providers/detect${refresh ? "?refresh=1" : ""}`,
    ),
  testModel: (body: {
    provider: string;
    name?: string;
    endpoint?: string;
    api_key_env?: string;
  }) => send<ModelTestResult>("/api/models/test", "POST", body),
  selectModel: (
    id: string,
    extra: { name?: string; provider?: string; endpoint?: string } = {},
  ) =>
    send<Record<string, unknown>>("/api/models/select", "POST", {
      id,
      ...extra,
    }),
  registerModel: (id: string, provider: string, name = "", endpoint = "") =>
    send<Record<string, unknown>>("/api/models/register", "POST", {
      id,
      provider,
      name,
      endpoint,
    }),
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
  version: () => get<{ name: string; version: string }>("/api/version"),
  updateCheck: () => get<UpdateInfo>("/api/update/check"),
  providers: () => get<{ providers: ProviderInfo[] }>("/api/providers"),
  routing: () => get<RoutingView>("/api/routing"),
  saveRouting: (values: Record<string, string | boolean>) =>
    send<RoutingView>("/api/routing", "PUT", {
      values,
      api_key: "",
      api_key_env: "",
    }),
  doctor: () => get<DoctorReport>("/api/doctor"),
  doctorFix: () =>
    send<{ applied: string[]; report: DoctorReport }>(
      "/api/doctor/fix",
      "POST",
      {},
    ),
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
  mcpServers: () => get<McpCatalog>("/api/mcp/servers"),
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
  saveMcpServers: (servers: McpServer[]) =>
    send<{ servers: McpServer[] }>("/api/mcp/servers", "PUT", {
      values: { servers },
      api_key: "",
      api_key_env: "",
    }),
  plugins: () =>
    get<{
      installed: { name: string; version: string; description: string }[];
      available: { name: string; installed: boolean }[];
    }>("/api/plugins"),
  installPlugin: (name: string) =>
    send<{ name: string }>(`/api/plugins/${name}/install`, "POST", {}),
  removePlugin: (name: string) =>
    send<{ ok: boolean }>(`/api/plugins/${name}/remove`, "POST", {}),
  taskCheckpoint: (taskId: string) =>
    get<{
      rewindable: boolean;
      checkpoint: { changes: number; paths: string[] } | null;
    }>(`/api/checkpoints/tasks/${taskId}`),
  rewindTask: (taskId: string) =>
    send<{ ok: boolean; restored: string[] }>(
      `/api/checkpoints/tasks/${taskId}/restore`,
      "POST",
      {},
    ),
  session: (id: string) => get<SessionDetail>(`/api/sessions/${id}`),
  activateSession: (id: string) =>
    send<SessionDetail>(`/api/sessions/${id}/activate`, "POST", {}),
  currentJob: (id: string) =>
    get<{ job: Job | null }>(
      `/api/jobs/current?session_id=${encodeURIComponent(id)}&include_finished=true`,
    ),
  jobs: () => get<{ jobs: Job[] }>("/api/jobs"),
  createSession: (workspace: string, title = "") =>
    send<{ id: string; workspace: string }>("/api/sessions", "POST", {
      workspace,
      title,
    }),
  events: (sessionId?: string) =>
    get<{ events: EventRow[] }>(
      `/api/events?limit=240${sessionId ? `&session_id=${sessionId}` : ""}`,
    ),
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
      permissions: { level: string; network?: boolean };
      onboarding?: { completed: boolean };
      routing?: Record<string, string | boolean>;
    }>("/api/workspace/status"),
  startJob: (
    task: string,
    workspace?: string,
    session_id?: string,
    model?: string,
    purpose: string = "coder",
  ) =>
    send<Job>("/api/jobs", "POST", {
      task,
      workspace,
      session_id,
      model: model || undefined,
      purpose,
    }),
  job: (id: string) => get<Job>(`/api/jobs/${id}`),
  cancelJob: (id: string) => send<Job>(`/api/jobs/${id}/cancel`, "POST", {}),
  cancelCurrent: (session_id?: string) =>
    send<{ ok: boolean }>(
      `/api/run/cancel${session_id ? `?session_id=${session_id}` : ""}`,
      "POST",
      {},
    ),
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
    send<{ path: string }>("/api/workspace/attach", "POST", { filename, text }),
  undo: () =>
    send<{ ok: boolean; restored: string[] }>(
      "/api/checkpoints/undo",
      "POST",
      {},
    ),
  exportUrl: (sessionId: string, format: "md" | "json" = "md") =>
    `/api/sessions/${sessionId}/export?format=${format}`,
  branchSession: (sessionId: string, title = "") =>
    send<{ id: string; parent_id: string }>(
      `/api/sessions/${sessionId}/branch`,
      "POST",
      { workspace: "", title },
    ),
  sessionCost: (sessionId: string) =>
    get<{
      session_id: string;
      usage: Record<string, number>;
      tasks: {
        task_id: string;
        prompt: string;
        status: string;
        usage: Record<string, number>;
      }[];
    }>(`/api/sessions/${sessionId}/cost`),
  listPins: (sessionId: string) =>
    get<{
      pins: {
        id: number;
        session_id: string;
        label: string;
        body: string;
        ts: number;
      }[];
    }>(`/api/sessions/${sessionId}/pins`),
  addPin: (sessionId: string, label: string, body: string) =>
    send<{ ok: boolean; id: number }>(
      `/api/sessions/${sessionId}/pins`,
      "POST",
      { name: label, content: body },
    ),
  deletePin: (sessionId: string, pinId: number) =>
    send<{ ok: boolean }>(
      `/api/sessions/${sessionId}/pins/${pinId}`,
      "DELETE",
      {},
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
