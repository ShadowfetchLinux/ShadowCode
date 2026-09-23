import { invoke, isTauri } from "@tauri-apps/api/core";

export const isNative = () => isTauri();

export async function request<T>(
  path: string,
  method = "GET",
  body?: unknown,
): Promise<T> {
  if (isNative()) {
    try {
      return await invoke<T>("api", {
        request: { method, path, body: body ?? null },
      });
    } catch (error) {
      throw new Error(String(error));
    }
  }
  const response = await fetch(path, {
    method,
    ...(body === undefined
      ? {}
      : {
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        }),
  });
  if (!response.ok) {
    let message = `${path} ${response.status}`;
    try {
      const error = await response.json();
      message = String(error.detail || error.error || message);
    } catch {
      /* The server can return a non-JSON transport failure. */
    }
    throw new Error(message);
  }
  return response.json() as Promise<T>;
}

export const pickDirectory = () => invoke<string | null>("pick_directory");

export const pickLocalModel = (folder = false) =>
  invoke<string | null>("pick_local_model", { folder });

export async function exportSession(
  sessionId: string,
  format: "md" | "json" = "md",
) {
  if (isNative())
    return invoke<string | null>("export_session", { sessionId, format });
  window.open(
    `/api/sessions/${encodeURIComponent(sessionId)}/export?format=${format}`,
    "_blank",
    "noopener",
  );
  return null;
}

export async function openExternal(url: string) {
  const parsed = new URL(url);
  if (!["https:", "http:"].includes(parsed.protocol))
    throw new Error("Only web links can be opened externally");
  if (isNative()) await invoke("open_external", { url: parsed.href });
  else window.open(parsed.href, "_blank", "noopener,noreferrer");
}
