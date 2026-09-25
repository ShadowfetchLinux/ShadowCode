/** The drawer's Tools tab: things you use while working rather than
 * configure — goals, background processes (dev servers, watchers) and
 * worktrees. They used to sit in Settings › Advanced. */
import { BackgroundTab, GoalsTab } from "./settings/AdvancedPanels";
import { WorktreeSettings } from "./WorktreeSettings";
import "../tools.css";

export type ToolsView = "goals" | "background" | "worktrees";
export const TOOL_VIEWS: { id: ToolsView; label: string }[] = [
  { id: "goals", label: "Goals" },
  { id: "background", label: "Processes" },
  { id: "worktrees", label: "Worktrees" },
];

type Toast = (text: string, kind?: "ok" | "err" | "info") => void;

export function ToolsTab({
  view,
  onView,
  sessionId,
  toast,
  onOpenSession,
  onOpenProject,
}: {
  view: ToolsView;
  onView: (view: ToolsView) => void;
  sessionId: string;
  toast: Toast;
  onOpenSession: (id: string) => void;
  onOpenProject?: (path: string) => void;
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
          <GoalsTab sessionId={sessionId} onOpen={onOpenSession} toast={toast} />
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
