import { listen } from "@tauri-apps/api/event";
import type { EventRow, Job } from "../api";
import { isNative, request } from "./transport";

export interface JobStream {
  onopen: (() => void) | null;
  onmessage: ((event: { data: string }) => void) | null;
  onerror: (() => void) | null;
  close(): void;
}
type Page = { events: EventRow[]; job: Job };
type Dependencies = {
  read: (after: number) => Promise<Page>;
  subscribe: (wake: () => void) => Promise<() => void>;
};

/** Notifications wake the reader; only ordered, durable rows update the UI. */
export function nativeJobStream(after: number, deps: Dependencies): JobStream {
  let closed = false;
  let reading = false;
  let again = false;
  let connected = false;
  let cursor = after;
  let unsubscribe: (() => void) | undefined;
  let scheduled: ReturnType<typeof setTimeout> | undefined;
  const source: JobStream = {
    onopen: null,
    onmessage: null,
    onerror: null,
    close() {
      if (closed) return;
      closed = true;
      clearInterval(poll);
      clearTimeout(scheduled);
      unsubscribe?.();
    },
  };
  async function drain() {
    if (closed) return;
    if (reading) {
      again = true;
      return;
    }
    reading = true;
    try {
      do {
        again = false;
        const page = await deps.read(cursor);
        if (closed) return;
        if (!connected) {
          connected = true;
          source.onopen?.();
        }
        for (const event of page.events) {
          if (closed) return;
          if (event.id && event.id > cursor) {
            source.onmessage?.({ data: JSON.stringify(event) });
            cursor = event.id;
          }
        }
        const active = ["queued", "running", "cancelling"].includes(
          page.job.status,
        );
        if (!active && cursor >= (page.job.event_cursor || 0)) {
          source.onmessage?.({
            data: JSON.stringify({ type: "job.done", payload: page.job }),
          });
          source.close();
          return;
        }
        // The IPC service caps each page at 512 events. Catch up before waiting
        // for another notification, including when completion was already saved.
        again ||= page.events.length >= 512;
      } while (again && !closed);
    } catch {
      if (!closed) {
        connected = false;
        source.onerror?.();
      }
    } finally {
      reading = false;
    }
  }
  function wake() {
    if (closed || scheduled) return;
    scheduled = setTimeout(() => {
      scheduled = undefined;
      void drain();
    }, 20);
  }
  const poll = setInterval(wake, 2500);
  void deps
    .subscribe(wake)
    .then((stop) => {
      if (closed) stop();
      else {
        unsubscribe = stop;
        wake();
      }
    })
    .catch(() => {
      if (!closed) source.onerror?.();
    });
  // A failed listener still has durable replay and polling recovery.
  wake();
  return source;
}

export function jobEvents(id: string, after: number): JobStream {
  if (!isNative())
    return new EventSource(
      `/api/jobs/${id}/events?after=${after}`,
    ) as unknown as JobStream;
  return nativeJobStream(after, {
    read: (cursor) =>
      request<Page>(`/api/jobs/${id}/events?after=${cursor}&limit=512`),
    subscribe: (wake) => listen("shadowcode:events", wake),
  });
}
