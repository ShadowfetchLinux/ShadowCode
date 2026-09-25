import type { RefObject } from "react";
import { api, type Project, type Session } from "../api";
import { projectOf, sidebarOrder, stepConversation } from "../lib/badges";
import { readPins } from "../lib/storage";
import { exportSession } from "../lib/transport";
import type { ToastKind } from "./useToasts";

/** The sidebar's conversation menu (Rename, Fork, Export, Delete) and the
 * keyboard moves between conversations (Alt+↑/↓, Ctrl+Tab). */
export function useConversationMenu(ctx: {
  sessions: Session[];
  projectPath: string;
  projects: Project[];
  sessionId: string;
  selectedRef: RefObject<string>;
  openSession: (id: string) => Promise<void>;
  newSession: () => Promise<void>;
  mostRecent: (exists: (id: string) => boolean) => string | null;
  refresh: () => Promise<void>;
  toast: (text: string, kind?: ToastKind) => void;
}) {
  const { sessions, toast } = ctx;

  /** Conversations in the sidebar's order. */
  function ordered() {
    const groups = [
      ...new Set([
        ctx.projectPath,
        ...ctx.projects.map((p) => p.path),
        ...sessions.map(projectOf),
      ]),
    ].filter(Boolean);
    return sidebarOrder(sessions, readPins(), groups);
  }
  function go(target: string | null) {
    if (target && target !== ctx.sessionId) void ctx.openSession(target);
  }

  async function act(
    action: "rename" | "fork" | "export" | "delete",
    session: Session,
    title?: string,
  ) {
    try {
      if (action === "rename") {
        if (!title) return;
        await api.renameSession(session.id, title);
      } else if (action === "fork") {
        const fork = await api.branchSession(session.id);
        await ctx.refresh();
        await ctx.openSession(fork.id);
        toast("Forked. The copy continues from the same history.", "ok");
        return;
      } else if (action === "export") {
        const saved = await exportSession(session.id, "md");
        if (saved) toast(`Exported to ${saved}`, "ok");
        return;
      } else {
        await api.deleteSession(session.id);
        if (session.id === ctx.selectedRef.current) {
          const next = sessions.find(
            (s) => s.id !== session.id && projectOf(s) === projectOf(session),
          );
          if (next) await ctx.openSession(next.id);
          else await ctx.newSession();
        }
      }
      await ctx.refresh();
    } catch (e) {
      toast(String(e), "err");
    }
  }

  return {
    act,
    previous: () => go(stepConversation(ordered(), ctx.sessionId, -1)),
    next: () => go(stepConversation(ordered(), ctx.sessionId, 1)),
    recent: () =>
      go(ctx.mostRecent((id) => sessions.some((s) => s.id === id))),
  };
}
