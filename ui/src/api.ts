export type ModelInfo = {
  id: string;
  name: string;
  provider: string;
  endpoint: string;
  detected?: boolean;
  metadata?: Record<string, unknown> & { capabilities?: Record<string, boolean>; detail?: string; detected?: boolean };
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

export type Project = { id: string; path: string; name: string; last_opened: number };
export type Session = {
  id: string;
  workspace: string;
  status: string;
  title?: string;
  updated_at: number;
  usage_json?: string;
  parent_id?: string;
};
export type EventRow = { ts: number; type: string; payload: Record<string, unknown>; session_id?: string; task_id?: string };
export type FileEntry = { name: string; path: string; type: "file" | "dir" };
export type PlanStep = { id: string; title: string; status: string; detail?: string };
export type Approval = {
  id: string;
  command?: string;
  reason?: string;
  tool?: string;
  pending?: boolean;
};
export type Job = {
  id: string;
  session_id: string;
  status: string;
  summary?: string;
  task?: string;
  usage?: Record<string, number>;
  result?: { success: boolean; summary: string; plan: { goal: string; steps: PlanStep[] }; usage?: Record<string, number> };
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
export type DiffHunk = { header: string; lines: { kind: string; text: string }[] };
export type ModelTestResult = {
  ok: boolean;
  latency_ms?: number;
  reply?: string;
  error?: string;
  model?: string;
  usage?: Record<string, number>;
  capabilities?: Record<string, unknown>;
};
export type ExecResult = { ok: boolean; command: string; stdout: string; stderr: string; exit_code: number; error?: string };

async function parseError(res: Response, path: string): Promise<string> {
  try {
    const data = await res.json();
    return String(data.detail || data.error || `${path} ${res.status}`);
  } catch {
    return `${path} ${res.status}`;
  }
}

async function get<T>(path: string): Promise<T> {
  const res = await fetch(path);
  if (!res.ok) throw new Error(await parseError(res, path));
  return res.json() as Promise<T>;
}

async function send<T>(path: string, method: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(await parseError(res, path));
  return res.json() as Promise<T>;
}

export const api = {
  health: () => get<Health>("/api/health"),
  onboarding: () =>
    get<{
      completed: boolean;
      suggested_workspace: string;
      providers: { id: string; provider: string; needs_key?: boolean; endpoint?: string; api_key_env?: string; name?: string }[];
      detected: DetectedProvider[];
      levels: string[];
      defaults: { provider: string; permission_level: string; theme: string };
    }>("/api/onboarding"),
  completeOnboarding: (body: Record<string, unknown>) => send<{ ok: boolean; workspace: string; session_id: string }>("/api/onboarding", "POST", body),
  config: () => get<Record<string, unknown>>("/api/config"),
  saveConfig: (values: Record<string, unknown>, api_key = "", api_key_env = "") =>
    send<Record<string, unknown>>("/api/config", "PUT", { values, api_key, api_key_env }),
  models: (refresh = false) => get<{ models: ModelInfo[] }>(`/api/models${refresh ? "?refresh=1" : ""}`),
  detectProviders: (refresh = false) => get<{ providers: DetectedProvider[] }>(`/api/providers/detect${refresh ? "?refresh=1" : ""}`),
  testModel: (body: { provider: string; name?: string; endpoint?: string; api_key_env?: string }) =>
    send<ModelTestResult>("/api/models/test", "POST", body),
  selectModel: (id: string, extra: { name?: string; provider?: string; endpoint?: string } = {}) =>
    send<Record<string, unknown>>("/api/models/select", "POST", { id, ...extra }),
  registerModel: (id: string, provider: string, name = "", endpoint = "") =>
    send<Record<string, unknown>>("/api/models/register", "POST", { id, provider, name, endpoint }),
  projects: () => get<{ projects: Project[] }>("/api/projects"),
  openProject: (path: string) =>
    send<{ path: string; session_id: string; needs_trust?: boolean; name?: string; permissions?: Record<string, unknown> }>(
      "/api/projects",
      "POST",
      { path },
    ),
  trustProject: (path: string) => send<{ ok: boolean; path: string; session_id: string }>("/api/projects/trust", "POST", { path }),
  sessions: () => get<{ sessions: Session[] }>("/api/sessions"),
  session: (id: string) => get<Session & { tasks: { id: string; prompt: string; summary?: string; status: string }[]; events: EventRow[] }>(`/api/sessions/${id}`),
  createSession: (workspace: string, title = "") => send<{ id: string; workspace: string }>("/api/sessions", "POST", { workspace, title }),
  events: (sessionId?: string) => get<{ events: EventRow[] }>(`/api/events?limit=240${sessionId ? `&session_id=${sessionId}` : ""}`),
  files: (path = ".") => get<{ entries: FileEntry[]; workspace: string; path: string; parent: string }>("/api/workspace/files?path=" + encodeURIComponent(path)),
  file: (path: string) => get<{ content: string; path: string }>("/api/workspace/file?path=" + encodeURIComponent(path)),
  git: () => get<{ status: string; log: string; diff: string; porcelain?: string; files?: { path: string; label: string }[]; repo?: boolean }>("/api/workspace/git"),
  gitDiff: (path = "") => get<{ diff: string; staged: string; hunks: DiffHunk[] }>("/api/workspace/diff?path=" + encodeURIComponent(path)),
  gitAdd: (paths: string[]) => send<{ ok: boolean }>("/api/workspace/git/add", "POST", { message: "", paths }),
  gitCommit: (message: string, paths: string[] = []) => send<{ ok: boolean }>("/api/workspace/git/commit", "POST", { message, paths }),
  hunkAction: (path: string, hunk: DiffHunk, action: "accept" | "reject") =>
    send<{ ok: boolean; action: string; path: string }>("/api/workspace/diff/hunk", "POST", { path, hunk, action }),
  exec: (command: string, timeout = 60) => send<ExecResult>("/api/workspace/exec", "POST", { command, timeout }),
  status: () =>
    get<{
      workspace: string;
      model: { default: string; provider: string; endpoint?: string; name?: string; api_key_env?: string };
      permissions: { level: string; network?: boolean };
      onboarding?: { completed: boolean };
    }>("/api/workspace/status"),
  startJob: (task: string, workspace?: string, session_id?: string, model?: string, purpose: string = "coder") =>
    send<Job>("/api/jobs", "POST", { task, workspace, session_id, model: model || undefined, purpose }),
  job: (id: string) => get<Job>(`/api/jobs/${id}`),
  cancelJob: (id: string) => send<Job>(`/api/jobs/${id}/cancel`, "POST", {}),
  cancelCurrent: (session_id?: string) => send<{ ok: boolean }>(`/api/run/cancel${session_id ? `?session_id=${session_id}` : ""}`, "POST", {}),
  approvals: () => get<{ approvals: Approval[] }>("/api/approvals"),
  decide: (id: string, decision: "approve" | "deny") => send<Approval>(`/api/approvals/${id}`, "POST", { decision }),
  instructions: () => get<{ content: string; exists: boolean }>("/api/workspace/instructions"),
  saveInstructions: (content: string) => send<{ ok: boolean }>("/api/workspace/instructions", "PUT", { content }),
  skills: () => get<{ skills: { name: string; content: string }[] }>("/api/workspace/skills"),
  saveSkill: (name: string, content: string) => send<{ ok: boolean }>("/api/workspace/skills", "PUT", { name, content }),
  attach: (filename: string, text: string) => send<{ path: string }>("/api/workspace/attach", "POST", { filename, text }),
  undo: () => send<{ ok: boolean; restored: string[] }>("/api/checkpoints/undo", "POST", {}),
  exportUrl: (sessionId: string, format: "md" | "json" = "md") => `/api/sessions/${sessionId}/export?format=${format}`,
  branchSession: (sessionId: string, title = "") =>
    send<{ id: string; parent_id: string }>(`/api/sessions/${sessionId}/branch`, "POST", { workspace: "", title }),
  sessionCost: (sessionId: string) =>
    get<{ session_id: string; usage: Record<string, number>; tasks: { task_id: string; prompt: string; status: string; usage: Record<string, number> }[] }>(
      `/api/sessions/${sessionId}/cost`,
    ),
  listPins: (sessionId: string) => get<{ pins: { id: number; session_id: string; label: string; body: string; ts: number }[] }>(`/api/sessions/${sessionId}/pins`),
  addPin: (sessionId: string, label: string, body: string) =>
    send<{ ok: boolean; id: number }>(`/api/sessions/${sessionId}/pins`, "POST", { name: label, content: body }),
  deletePin: (sessionId: string, pinId: number) =>
    send<{ ok: boolean }>(`/api/sessions/${sessionId}/pins/${pinId}`, "DELETE", {}),
  commands: () => get<{ commands: { name: string; description: string; arg_spec: string; alias: string; source: string }[] }>("/api/commands"),
  runCommand: (name: string, args = "", sessionId?: string) =>
    send<CommandResult>("/api/commands/run", "POST", { name, args, session_id: sessionId }),
};

export type CommandResult = {
  handled: boolean;
  text: string;
  kind: "text" | "card" | "list" | "diff" | "approval" | "error" | "overlay" | "quit";
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
