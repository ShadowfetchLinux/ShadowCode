/** The drawer's Tools tab: things you use while working rather than
 * configure — goals, scheduled automations, starting from an issue,
 * background processes (dev servers, watchers) and worktrees. */
import { BackgroundTab, GoalsTab } from "./settings/AdvancedPanels";
import { WorktreeSettings } from "./WorktreeSettings";
import { AutomationsPanel } from "./AutomationsPanel";
import { IssuesPanel } from "./IssuesPanel";
import type { IssueLink } from "../lib/issues";
import "../tools.css";

export type ToolsView =
  "goals" | "automations" | "issues" | "background" | "worktrees";
export const TOOL_VIEWS: { id: ToolsView; label: string }[] = [
  { id: "goals", label: "Goals" },
  { id: "automations", label: "Automations" },
  { id: "issues", label: "Issues" },
  { id: "background", label: "Processes" },
  { id: "worktrees", label: "Worktrees" },
];

type Toast = (text: string, kind?: "ok" | "err" | "info") => void;

export function ToolsTab({
  view,
  onView,
  sessionId,
  busy,
  toast,
  onOpenSession,
  onOpenProject,
  onAskAgent,
  onIssueLinked,
  onOpenTerminal,
}: {
  view: ToolsView;
  onView: (view: ToolsView) => void;
  sessionId: string;
  busy: boolean;
  toast: Toast;
  onOpenSession: (id: string) => void;
  onOpenProject?: (path: string) => void;
  onAskAgent?: (prompt: string) => void;
  onIssueLinked: (link: IssueLink) => void;
  onOpenTerminal: () => void;
}) {
  return (
    <section className="tools-tab" aria-label="Tools">
      <div className="seg" role="group" aria-label="Tool">
        {TOOL_VIEWS.map((t) => (
          <button
            type="button"
            key={t.id}
            className={view === t.id ? "on" : ""}
            aria-pressed={view === t.id}
            onClick={() => onView(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div className="tools-body">
        {view === "goals" && (
          <GoalsTab
            sessionId={sessionId}
            onOpen={onOpenSession}
            toast={toast}
          />
        )}
        {view === "automations" && (
          <AutomationsPanel toast={toast} onOpenSession={onOpenSession} />
        )}
        {view === "issues" && (
          <IssuesPanel
            busy={busy}
            toast={toast}
            onStartTask={(task) => onAskAgent?.(task)}
            onLinked={onIssueLinked}
            onOpenTerminal={onOpenTerminal}
          />
        )}
        {view === "background" && <BackgroundTab toast={toast} />}
        {view === "worktrees" && (
          <WorktreeSettings
            onOpen={onOpenProject}
            onToast={(text, kind) => toast(text, kind)}
          />
        )}
      </div>
    </section>
  );
}
