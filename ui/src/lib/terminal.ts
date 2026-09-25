import { isNative, listen, request } from "./transport";

/** One of the user's own terminals in the drawer (GET/POST /api/terminals).
 * The engine keeps the shell and a bounded scrollback; the window reads
 * output by byte cursor after a `shadowcode:terminal` wake-up. */
export type TerminalInfo = {
  id: string;
  title: string;
  number: number;
  workspace: string;
  shell: string;
  cols: number;
  rows: number;
  created: number;
  exited: boolean;
  exit_code: number | null;
  cursor: number;
};

export type TerminalChunk = {
  id: string;
  /** Base64 of the raw bytes after `from`. */
  data: string;
  from: number;
  cursor: number;
  more: boolean;
  /** The requested cursor fell out of the scrollback. */
  truncated: boolean;
  exited: boolean;
  exit_code: number | null;
};

const enc = encodeURIComponent;

export const terminalApi = {
  list: () =>
    request<{ workspace: string; terminals: TerminalInfo[] }>("/api/terminals"),
  open: (cols: number, rows: number) =>
    request<TerminalInfo>("/api/terminals", "POST", { cols, rows }),
  input: (id: string, data: string) =>
    request<{ ok: boolean }>(`/api/terminals/${enc(id)}/input`, "POST", {
      data,
    }),
  resize: (id: string, cols: number, rows: number) =>
    request<{ cols: number; rows: number }>(
      `/api/terminals/${enc(id)}/resize`,
      "POST",
      { cols, rows },
    ),
  output: (id: string, after: number) =>
    request<TerminalChunk>(`/api/terminals/${enc(id)}/output?after=${after}`),
  close: (id: string) =>
    request<{ ok: boolean }>(`/api/terminals/${enc(id)}/close`, "POST", {}),
};

export function decodeBase64(data: string): Uint8Array {
  const text = atob(data);
  const bytes = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
  return bytes;
}

/** Keystrokes go out in order, one request at a time; what is typed while a
 * request is in flight is sent together next. `onError` hears a failed
 * send (the batch is dropped: the shell may be gone). */
export function inputQueue(
  send: (data: string) => Promise<unknown>,
  onError: (error: unknown) => void,
) {
  let pending = "";
  let busy = false;
  let closed = false;
  const LIMIT = 60_000;
  async function flush() {
    if (busy || closed) return;
    busy = true;
    try {
      while (pending && !closed) {
        // Stay under the engine's 64 KB write limit without splitting a
        // surrogate pair.
        let cut = Math.min(pending.length, LIMIT);
        const code = pending.charCodeAt(cut - 1);
        if (cut < pending.length && code >= 0xd800 && code <= 0xdbff) cut -= 1;
        const next = pending.slice(0, cut);
        pending = pending.slice(cut);
        try {
          await send(next);
        } catch (error) {
          pending = "";
          onError(error);
        }
      }
    } finally {
      busy = false;
    }
  }
  return {
    push(data: string) {
      if (closed || !data) return;
      pending += data;
      void flush();
    },
    close() {
      closed = true;
      pending = "";
    },
    /** For tests: resolves when nothing is queued or sending. */
    async idle() {
      while (busy || pending) await new Promise((r) => setTimeout(r, 1));
    },
  };
}

/** Reads output from `cursor` on, one read at a time; a wake-up during a
 * read runs one more. `write` gets each chunk's bytes in order. */
export function outputReader({
  id,
  read,
  write,
  onState,
  cursor = 0,
}: {
  id: string;
  read: (id: string, after: number) => Promise<TerminalChunk>;
  write: (bytes: Uint8Array) => void;
  onState?: (chunk: TerminalChunk) => void;
  cursor?: number;
}) {
  let position = cursor;
  let reading = false;
  let again = false;
  let stopped = false;
  async function pull() {
    if (stopped) return;
    if (reading) {
      again = true;
      return;
    }
    reading = true;
    try {
      do {
        again = false;
        const chunk = await read(id, position);
        if (stopped) return;
        if (chunk.data) write(decodeBase64(chunk.data));
        position = chunk.cursor;
        onState?.(chunk);
        if (chunk.more) again = true;
      } while (again && !stopped);
    } finally {
      reading = false;
    }
  }
  return {
    wake: () => pull(),
    stop() {
      stopped = true;
    },
    get cursor() {
      return position;
    },
  };
}

/** Terminal wake-ups: `{type, terminal_id}` for one terminal, or an untyped
 * engine wake-up (lag, reattach) that may hide any of them. */
export function subscribeTerminals(
  handler: (terminalId: string | null) => void,
): () => void {
  if (!isNative()) return () => undefined;
  const stops: (() => void)[] = [];
  let stopped = false;
  const keep = (p: Promise<() => void>) =>
    void p
      .then((stop) => (stopped ? stop() : stops.push(stop)))
      .catch(() => undefined);
  keep(
    listen("shadowcode:terminal", (payload) => {
      const id = (payload as { terminal_id?: unknown } | null)?.terminal_id;
      handler(typeof id === "string" ? id : null);
    }),
  );
  keep(
    listen("shadowcode:events", (payload) => {
      const type = (payload as { type?: unknown } | null)?.type;
      if (typeof type !== "string" || !type || type.startsWith("view."))
        handler(null);
    }),
  );
  return () => {
    stopped = true;
    stops.forEach((stop) => stop());
  };
}
