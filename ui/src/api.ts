export type ModelInfo = {
  id: string;
  name: string;
  provider: string;
  endpoint: string;
};

export type Project = { id: string; path: string; name: string; last_opened: number };
export type EventRow = { ts: number; type: string; payload: Record<string, unknown>; session_id?: string };
export type FileEntry = { name: string; path: string; type: "file" | "dir" };

async function get<T>(path: string): Promise<T> {
  const res = await fetch(path);
  if (!res.ok) throw new Error(`${path} ${res.status}`);
  return res.json() as Promise<T>;
}

async function post<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${path} ${res.status}`);
  return res.json() as Promise<T>;
}

export const api = {
  health: () => get<{ ok: boolean; workspace: string }>("/api/health"),
  config: () => get<Record<string, unknown>>("/api/config"),
  models: () => get<{ models: ModelInfo[] }>("/api/models"),
  projects: () => get<{ projects: Project[] }>("/api/projects"),
  events: () => get<{ events: EventRow[] }>("/api/events?limit=120"),
  files: (path = ".") => get<{ entries: FileEntry[]; workspace: string }>("/api/workspace/files?path=" + encodeURIComponent(path)),
  file: (path: string) => get<{ content: string; path: string }>("/api/workspace/file?path=" + encodeURIComponent(path)),
  git: () => get<{ status: string; log: string; diff: string }>("/api/workspace/git"),
  status: () => get<{ workspace: string; model: { default: string; provider: string }; permissions: { level: string } }>("/api/workspace/status"),
  run: (task: string, workspace?: string) =>
    post<{ success: boolean; summary: string; plan: { goal: string; steps: { id: string; title: string; status: string }[] } }>(
      "/api/run",
      { task, workspace },
    ),
};
