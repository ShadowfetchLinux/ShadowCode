import { request } from "./transport";

/** Settings › Voice (`voice:` in config.yaml). */
export type VoiceSettings = {
  engine: "local" | "openrouter";
  model: string;
  language: string;
  openrouter_model: string;
  voice_commands: boolean;
  live_preview: boolean;
  max_seconds: number;
};

export type VoiceModel = {
  id: string;
  name: string;
  bytes: number;
  license: string;
  english_only: boolean;
  summary: string;
  installed: boolean;
  active: boolean;
  progress: {
    state: string;
    done: number;
    total: number;
    error?: string | null;
  } | null;
};

export type VoiceStatus = {
  config: VoiceSettings;
  models: VoiceModel[];
  languages: { code: string; name: string }[];
  /** The chosen engine can run now. */
  ready: boolean;
  /** Why it cannot, in words for the user. */
  blocked: string | null;
  cpu_supported: boolean;
  whisper_version: string;
  openrouter_key: boolean;
  offline: boolean;
  recording: boolean;
};

/** Polled while listening. */
export type VoiceRecording = {
  active: boolean;
  engine?: string;
  device?: string;
  level?: number;
  seconds?: number;
  max_seconds?: number;
  /** The length limit was reached; the window stops the recording. */
  full?: boolean;
  error?: string | null;
  /** Live preview text (local engine). */
  partial?: string;
};

export type VoiceResult = {
  text: string;
  engine: string;
  seconds: number;
  ms: number;
  /** Set when there is no text, to say why. */
  message?: string | null;
};

export const voiceApi = {
  status: () => request<VoiceStatus>("/api/voice/status"),
  save: (values: Partial<VoiceSettings>) =>
    request<{ ok: boolean; config: VoiceSettings }>(
      "/api/voice/config",
      "POST",
      values,
    ),
  install: (model: string) =>
    request<{ ok: boolean; started: boolean }>(
      "/api/voice/models/install",
      "POST",
      { model },
    ),
  remove: (model: string) =>
    request<{ ok: boolean; removed: boolean }>(
      "/api/voice/models/remove",
      "POST",
      { model },
    ),
  start: () =>
    request<{ ok: boolean; engine: string; device: string }>(
      "/api/voice/start",
      "POST",
      {},
    ),
  recording: () => request<VoiceRecording>("/api/voice/recording"),
  stop: () => request<VoiceResult>("/api/voice/stop", "POST", {}),
  cancel: () =>
    request<{ ok: boolean; cancelled: boolean }>(
      "/api/voice/cancel",
      "POST",
      {},
    ),
};

/** Insert `text` into `value` over the selection `[start, end)`, adding a
 * space where the dictated words would otherwise touch the neighbouring
 * text. Returns the new value and where the caret goes. Pure, for tests. */
export function insertAtCursor(
  value: string,
  start: number,
  end: number,
  text: string,
): { value: string; caret: number } {
  const from = Math.max(0, Math.min(start, value.length));
  const to = Math.max(from, Math.min(end, value.length));
  const before = value.slice(0, from);
  const after = value.slice(to);
  let insert = text;
  if (before && !/\s$/.test(before) && /^[^\s.,;:!?)\]}]/.test(insert))
    insert = ` ${insert}`;
  if (after && !/^\s/.test(after) && !/\s$/.test(insert)) insert = `${insert} `;
  return {
    value: before + insert + after,
    caret: before.length + insert.length,
  };
}

/** "0:07" */
export function formatSeconds(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}
