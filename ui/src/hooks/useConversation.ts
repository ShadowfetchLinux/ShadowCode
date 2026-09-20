import { useCallback, useEffect, useRef, useState } from "react";
import { api, type EventRow, type Job, type SessionDetail } from "../api";
import { applyEvent, emptyTranscript, replay } from "../lib/transcript";

export const isActive = (job: Job | null) =>
  !!job && ["queued", "running", "cancelling"].includes(job.status);

export function useConversation(onComplete: () => void) {
  const [transcript, setTranscript] = useState(emptyTranscript);
  const [job, setJob] = useState<Job | null>(null);
  const [connection, setConnection] = useState<"connected" | "reconnecting">(
    "connected",
  );
  const generation = useRef(0);
  const cursor = useRef(0);
  const complete = useRef(onComplete);
  complete.current = onComplete;

  const load = useCallback((detail: SessionDetail, active: Job | null) => {
    generation.current++;
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
    setTranscript(state);
    setJob(active);
    setConnection("connected");
  }, []);
  const start = useCallback((next: Job) => {
    cursor.current = next.event_cursor || 0;
    setJob(next);
    setTranscript((s) => ({ ...s, stage: "UNDERSTAND", plan: [], usage: {} }));
  }, []);

  useEffect(() => {
    if (!isActive(job)) return;
    const id = job!.id;
    const current = generation.current;
    let closed = false;
    let polling = false;
    let recovering = false;
    const source = new EventSource(
      `/api/jobs/${id}/events?after=${cursor.current}`,
    );
    function finish(done: Job) {
      if (closed || current !== generation.current) return;
      closed = true;
      source.close();
      setJob(done);
      setConnection("connected");
      setTranscript((s) => {
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
        setTranscript((s) => applyEvent(s, row));
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
            setTranscript(replay(detail.events));
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
  }, [job?.id, job?.status]);

  return {
    transcript,
    setTranscript,
    job,
    setJob,
    connection,
    load,
    start,
    busy: isActive(job),
  };
}
