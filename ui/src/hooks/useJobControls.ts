import { useEffect, useState, type RefObject } from "react";
import { api, type Job } from "../api";
import { conversationJob } from "../lib/jobs";
import type { useConversation } from "./useConversation";
import type { useFeed } from "./useFeed";
import type { ToastKind } from "./useToasts";

/** Stop, queued follow-ups, approvals, rewind and fork for the open
 * conversation. */
export function useJobControls({
  conversation,
  feed,
  sessionId,
  selectedRef,
  submitting,
  submittingRef,
  switching,
  setRunningChoice,
  openSession,
  refresh,
  toast,
}: {
  conversation: ReturnType<typeof useConversation>;
  feed: ReturnType<typeof useFeed>;
  sessionId: string;
  selectedRef: RefObject<string>;
  submitting: boolean;
  submittingRef: RefObject<boolean>;
  switching: boolean;
  setRunningChoice: (id: string) => void;
  openSession: (id: string) => Promise<void>;
  refresh: () => Promise<void>;
  toast: (text: string, kind?: ToastKind) => void;
}) {
  const { job, busy } = conversation;
  const load = conversation.load;
  const jobs = feed.jobs;
  const [cancellingQueued, setCancellingQueued] = useState<string[]>([]);
  const [forking, setForking] = useState(false);
  const [retry, setRetry] = useState(0);

  async function stop() {
    if (!job || !busy) return;
    try {
      const next = await api.cancelJob(job.id);
      if (selectedRef.current === next.session_id) {
        const detail = await api.session(next.session_id);
        if (selectedRef.current === next.session_id) load(detail, next);
      }
      await refresh();
    } catch (e) {
      toast(String(e), "err");
    }
  }

  async function cancelQueued(queued: Job) {
    if (cancellingQueued.includes(queued.id)) return;
    setCancellingQueued((ids) => [...ids, queued.id]);
    try {
      const updated = await api.cancelJob(queued.id, true);
      feed.setJobs((items) =>
        items.map((item) => (item.id === updated.id ? updated : item)),
      );
      toast("Queued task cancelled.", "info");
    } catch (e) {
      toast(String(e), "err");
    } finally {
      setCancellingQueued((ids) => ids.filter((id) => id !== queued.id));
      void refresh().catch(() => undefined);
    }
  }

  /** Answer an approval, then read the feed at once (the engine also wakes
   * it when the tool resumes). */
  async function decide(id: string, decision: "approve" | "deny") {
    const approval = feed.approvals.find((a) => a.id === id);
    try {
      await api.decide(id, decision, approval?.session_id);
      await feed.refresh();
    } catch (e) {
      toast(String(e), "err");
    }
  }

  async function rewind(taskId: string) {
    if (busy) {
      toast("Stop the task before rewinding its files.", "info");
      return;
    }
    try {
      const result = await api.rewindTask(taskId);
      toast(`Restored ${result.restored.length} files`, "ok");
      await refresh();
    } catch (e) {
      toast(String(e), "err");
    }
  }

  async function fork(eventId: number) {
    setForking(true);
    try {
      const branch = await api.forkSession(sessionId, eventId);
      await openSession(branch.fork.id);
      toast("New conversation created from this point", "ok");
    } catch (error) {
      toast(String(error), "err");
    } finally {
      setForking(false);
    }
  }

  // Queued follow-ups can start after the visible job has finished. Reload
  // before attaching their stream so intervening milestones are retained.
  useEffect(() => {
    if (!sessionId || busy || submitting || switching) return;
    const sessionJobs = jobs.filter((item) => item.session_id === sessionId);
    const next = conversationJob(sessionJobs, job);
    if (!next || next.id === job?.id) return;
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    void Promise.all([api.session(sessionId), api.job(next.id)])
      .then(([detail, fullJob]) => {
        if (
          live &&
          selectedRef.current === sessionId &&
          !submittingRef.current
        ) {
          load(detail, fullJob, true);
          setRunningChoice(fullJob.model || fullJob.routing?.requested || "");
        }
      })
      .catch(() => {
        // The job list only changes when a job does: retry on a timer.
        if (live) timer = setTimeout(() => setRetry((n) => n + 1), 2000);
      });
    return () => {
      live = false;
      clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [jobs, sessionId, busy, submitting, switching, job, load, retry]);

  return {
    cancellingQueued,
    forking,
    stop,
    cancelQueued,
    decide,
    rewind,
    fork,
  };
}
