import { useMemo, useRef, type SetStateAction } from "react";
import { api } from "../api";
import type { ChatItem } from "../components/cards";
import type { RowActions } from "../components/shell/TranscriptRows";
import type { Fallback } from "../lib/allowance";
import { batchDiffStats } from "../lib/diffStats";
import type { Transcript } from "../lib/transcript";

type LimitItem = Extract<ChatItem, { kind: "limit" }>;
export type RowHandlers = {
  setTranscript: (update: SetStateAction<Transcript>) => void;
  reviewChanges: (path?: string) => void;
  rewind: (taskId: string) => Promise<void>;
  continueOnFallback: (item: LimitItem, choice: Fallback) => Promise<void>;
  chooseModel: () => void;
  openLocal: () => void;
  fork: (eventId: number) => Promise<void>;
};

/** Transcript row callbacks with stable identities (they call the latest
 * handlers), so memoized rows skip the window's re-renders. Changed-file
 * line counts from every task summary on screen go out in one request. */
export function useRowActions(handlers: RowHandlers): RowActions {
  const latest = useRef(handlers);
  latest.current = handlers;
  return useMemo(
    () => ({
      onToggleTool: (key: string) =>
        latest.current.setTranscript((s) => ({
          ...s,
          items: s.items.map((it) =>
            it.key === key && it.kind === "tool"
              ? { ...it, collapsed: it.collapsed === false }
              : it,
          ),
        })),
      diffStats: batchDiffStats((paths) =>
        api.diffStats(paths).then((r) => r.stats),
      ),
      onReview: (path?: string) => latest.current.reviewChanges(path),
      onRewind: (taskId: string) => void latest.current.rewind(taskId),
      onContinue: (item: LimitItem, choice: Fallback) =>
        void latest.current.continueOnFallback(item, choice),
      onChooseModel: () => latest.current.chooseModel(),
      onOpenLocal: () => latest.current.openLocal(),
      onFork: (eventId: number) => void latest.current.fork(eventId),
    }),
    [],
  );
}
