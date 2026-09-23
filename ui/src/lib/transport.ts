import { invoke as tauriInvoke, isTauri } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";

/** The window talks to the in-process engine through one bridge: IPC requests,
 * native commands (pickers, external links) and engine wake-up events. */
export type Bridge = {
  request(path: string, method: string, body: unknown): Promise<unknown>;
  invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
  listen(
    event: string,
    handler: (payload: unknown) => void,
  ): Promise<() => void>;
};

declare global {
  interface Window {
    __SHADOW_TEST_TRANSPORT__?: Bridge;
  }
}

const tauriBridge: Bridge = {
  request: (path, method, body) =>
    tauriInvoke("api", { request: { method, path, body: body ?? null } }),
  invoke: (command, args) => tauriInvoke(command, args),
  listen: (event, handler) =>
    tauriListen(event, (message) => handler(message.payload)),
};

/** A deterministic fake backend is honoured only in builds made with
 * VITE_SHADOW_TEST_TRANSPORT=1 (the Playwright suite). Production builds
 * replace the condition with `false`, so the global is never read. */
function testBridge(): Bridge | undefined {
  if (import.meta.env.VITE_SHADOW_TEST_TRANSPORT === "1")
    return window.__SHADOW_TEST_TRANSPORT__;
  return undefined;
}

function bridge(): Bridge {
  const fake = testBridge();
  if (fake) return fake;
  if (!isTauri())
    throw new Error(
      "ShadowCode's interface runs inside the desktop app. Start it with `shadowcode`.",
    );
  return tauriBridge;
}

export const isNative = () => Boolean(testBridge()) || isTauri();

/** Errors keep the backend body so callers can read structured answers such as
 * a consent request (`needs_consent`). */
export class ApiError extends Error {
  body: Record<string, unknown> | null;
  constructor(message: string, body: Record<string, unknown> | null = null) {
    super(message);
    this.name = "ApiError";
    this.body = body;
  }
}

function parseErrorBody(text: string): Record<string, unknown> | null {
  const start = text.indexOf("{");
  if (start < 0) return null;
  try {
    const value = JSON.parse(text.slice(start));
    return value && typeof value === "object" ? value : null;
  } catch {
    return null;
  }
}

export async function request<T>(
  path: string,
  method = "GET",
  body?: unknown,
): Promise<T> {
  try {
    return (await bridge().request(path, method, body)) as T;
  } catch (error) {
    if (error instanceof ApiError) throw error;
    const text = error instanceof Error ? error.message : String(error);
    const parsed = parseErrorBody(text);
    throw new ApiError(
      parsed && typeof parsed.error === "string" ? parsed.error : text,
      parsed,
    );
  }
}

export const invoke = <T>(command: string, args?: Record<string, unknown>) =>
  bridge().invoke(command, args) as Promise<T>;

export const listen = (event: string, handler: (payload: unknown) => void) =>
  bridge().listen(event, handler);

export const pickDirectory = () => invoke<string | null>("pick_directory");

export const pickLocalModel = (folder = false) =>
  invoke<string | null>("pick_local_model", { folder });

export const exportSession = (
  sessionId: string,
  format: "md" | "json" = "md",
) => invoke<string | null>("export_session", { sessionId, format });

export async function openExternal(url: string) {
  const parsed = new URL(url);
  if (!["https:", "http:"].includes(parsed.protocol))
    throw new Error("Only web links can be opened externally");
  await invoke("open_external", { url: parsed.href });
}
