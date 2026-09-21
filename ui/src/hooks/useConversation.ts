import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type SetStateAction,
} from "react";
import {
  api,
  type EventRow,
  type Job,
  type SessionDetail,
  type HistoryPage,
} from "../api";
import {
  applyEvent,
  emptyTranscript,
  replay,
  type Transcript,
} from "../lib/transcript";
import { jobEvents } from "../lib/jobEvents";
import { isNative } from "../lib/transport";

export const isActive = (job: Job | null) =>
  !!job && ["queued", "running", "paused", "cancelling"].includes(job.status);

export function useConversation(onComplete: () => void) {
  const [liveTranscript, setLiveTranscript] = useState(emptyTranscript);
  const [job, setJob] = useState<Job | null>(null);
  const [connection, setConnection] = useState<"connected" | "reconnecting">(
    "connected",
  );
  const [history, setHistory] = useState<{
    page: HistoryPage;
    state: Transcript;
    before: number;
    newer: number[];
  } | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyError, setHistoryError] = useState("");
  const [historyBase, setHistoryBase] = useState<{
    first_cursor: number;
    has_older: boolean;
  } | null>(null);
  const [streamEpoch, setStreamEpoch] = useState(0);
  const session = useRef("");
  const historyRequest = useRef(0);
  const historyPending = useRef(false);
  const clearHistory = useCallback(() => {
    historyRequest.current++;
    historyPending.current = false;
    setHistory(null);
    setHistoryLoading(false);
    setHistoryError("");
  }, []);
  const generation = useRef(0);
  const cursor = useRef(0);
  const complete = useRef(onComplete);
  complete.current = onComplete;

  const load = useCallback(
    (detail: SessionDetail, active: Job | null, preserveHistory = false) => {
      generation.current++;
      setStreamEpoch((epoch) => epoch + 1);
      if (!preserveHistory || session.current !== detail.id) clearHistory();
      session.current = detail.id;
      setHistoryBase(detail.history_page || null);
      const state = replay(detail.events);
      if (active && !isActive(active)) {
        state.stage = active.status.toUpperCase();
        if (
          active.summary &&
          !state.items.some(
            (item) => item.kind === "agent" && item.text === active.summary,
          )
        )
          state.items.push({
            kind: "agent",
            text: active.summary,
            who: active.status === "completed" ? "Result" : "Needs attention",
          });
      }
      cursor.current = detail.event_cursor || state.cursor;
      setLiveTranscript(state);
      setJob(active);
      setConnection("connected");
    },
    [clearHistory],
  );
  const start = useCallback(
    (next: Job) => {
      clearHistory();
      session.current = next.session_id;
      cursor.current = next.event_cursor || 0;
      setJob(next);
      setLiveTranscript((s) => ({
        ...s,
        stage: "UNDERSTAND",
        plan: [],
        usage: {},
      }));
    },
    [clearHistory],
  );

  useEffect(() => {
    if (!isActive(job)) return;
    const id = job!.id;
    const current = generation.current;
    let closed = false;
    let polling = false;
    let recovering = false;
    const source = jobEvents(id, cursor.current);
    function finish(done: Job) {
      if (closed || current !== generation.current) return;
      closed = true;
      source.close();
      setJob(done);
      setConnection("connected");
      setLiveTranscript((s) => {
        const items = [...s.items];
        if (
          done.summary &&
          !items.some((i) => i.kind === "agent" && i.text === done.summary)
        )
          items.push({
            kind: "agent",
            text: done.summary,
            who: done.status === "completed" ? "Result" : "Needs attention",
          });
        return {
          ...s,
          items,
          usage: done.usage || s.usage,
          stage: done.status.toUpperCase(),
        };
      });
      complete.current();
      if (
        !isNative() &&
        document.hidden &&
        typeof Notification !== "undefined" &&
        Notification.permission === "granted"
      ) {
        try {
          new Notification("ShadowCode · " + done.status, {
            body: done.summary?.slice(0, 140),
            icon: "/icon.svg",
          });
        } catch {
          /* desktop may block notifications */
        }
      }
    }
    source.onopen = () => {
      if (!closed) {
        recovering = false;
        setConnection("connected");
      }
    };
    source.onmessage = (event) => {
      if (closed || current !== generation.current) return;
      try {
        const row = JSON.parse(event.data) as EventRow;
        if (row.type === "job.done") {
          finish(row.payload as Job);
          return;
        }
        cursor.current = Math.max(cursor.current, row.id || 0);
        if (
          row.type === "agent.started" &&
          (!job?.task_id || row.task_id === job.task_id)
        ) {
          setJob((current) =>
            current?.id === id && current.status === "queued"
              ? {
                  ...current,
                  status: "running",
                  started_at: row.ts || current.started_at,
                }
              : current,
          );
        }
        setLiveTranscript((s) => applyEvent(s, row));
      } catch {
        /* malformed events do not tear down a working connection */
      }
    };
    source.onerror = () => {
      if (!closed) {
        recovering = true;
        setConnection("reconnecting");
      }
    };
    // EventSource retries with Last-Event-ID. Poll as a fallback for a restarted
    // server or a proxy that terminates the final event, without reporting idle.
    const timer = setInterval(async () => {
      if (closed || polling || !recovering) return;
      polling = true;
      try {
        const latest = await api.job(id);
        if (!isActive(latest)) {
          const detail = await api.session(latest.session_id);
          if (!closed && current === generation.current) {
            setLiveTranscript(replay(detail.events));
            setHistoryBase(detail.history_page || null);
            finish(latest);
          }
        }
      } catch {
        /* reconnecting state remains visible */
      } finally {
        polling = false;
      }
    }, 2500);
    return () => {
      closed = true;
      source.close();
      clearInterval(timer);
    };
  }, [job?.id, job?.status, streamEpoch]);

  const pageHistory = useCallback(
    async (direction: "older" | "newer") => {
      if (historyPending.current || !session.current) return;
      const before =
        direction === "older"
          ? history?.page.first_cursor || historyBase?.first_cursor || 0
          : history?.newer.at(-1) || 0;
      if (!before) {
        if (direction === "newer") clearHistory();
        return;
      }
      const request = ++historyRequest.current;
      const selected = session.current;
      historyPending.current = true;
      setHistoryLoading(true);
      setHistoryError("");
      try {
        const page = await api.historyPage(selected, before);
        if (request !== historyRequest.current || selected !== session.current)
          return;
        setHistory({
          page,
          state: replay(page.events),
          before,
          newer:
            direction === "older"
              ? [...(history?.newer || []), history?.before || 0]
              : (history?.newer || []).slice(0, -1),
        });
      } catch (error) {
        if (request === historyRequest.current) setHistoryError(String(error));
      } finally {
        if (request === historyRequest.current) {
          historyPending.current = false;
          setHistoryLoading(false);
        }
      }
    },
    [history, historyBase, clearHistory],
  );
  const setTranscript = useCallback(
    (update: SetStateAction<Transcript>) => {
      if (history)
        setHistory((current) =>
          current
            ? {
                ...current,
                state:
                  typeof update === "function" ? update(current.state) : update,
              }
            : current,
        );
      else setLiveTranscript(update);
    },
    [history],
  );
  return {
    transcript: history
      ? { ...liveTranscript, items: history.state.items }
      : liveTranscript,
    setTranscript,
    history: {
      enabled: Boolean(historyBase),
      viewing: Boolean(history),
      firstCursor: history?.page.first_cursor || 0,
      hasOlder: history
        ? history.page.has_older
        : Boolean(historyBase?.has_older),
      loading: historyLoading,
      error: historyError,
      older: () => pageHistory("older"),
      newer: () => pageHistory("newer"),
      latest: clearHistory,
    },
    job,
    setJob,
    connection,
    load,
    start,
    busy: isActive(job),
  };
}
