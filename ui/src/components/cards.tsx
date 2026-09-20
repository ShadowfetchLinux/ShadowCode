import type { Approval, CommandResult } from "../api";

export type ChatItem =
  | { kind: "user"; text: string }
  | { kind: "agent"; text: string; who?: string }
  | {
      kind: "tool";
      tool: string;
      ok?: boolean;
      text: string;
      live?: boolean;
      icon?: string;
      headline?: string;
      fullOutput?: string;
      collapsed?: boolean;
      taskId?: string;
      callId?: string;
      path?: string;
    };

const MUTATING = new Set([
  "write_file",
  "edit_file",
  "delete_file",
  "move_file",
  "apply_patch",
]);

export function isMutatingTool(tool: string): boolean {
  return MUTATING.has(tool);
}

/** Codex-style collapsed one-liner. Click to expand; expanded cards expose
 *  Rewind (per-task file undo) and Review diff (jump to the Changes tab). */
export function OpCard({
  item,
  onToggle,
  onRewind,
  onReviewDiff,
}: {
  item: Extract<ChatItem, { kind: "tool" }>;
  onToggle: () => void;
  onRewind?: (taskId: string) => void;
  onReviewDiff?: (path: string) => void;
}) {
  const open = item.collapsed === false;
  const canRewind =
    Boolean(item.taskId) &&
    isMutatingTool(item.tool) &&
    item.ok !== false &&
    !item.live;
  const canDiff =
    Boolean(item.path) &&
    isMutatingTool(item.tool) &&
    item.ok !== false &&
    !item.live;
  return (
    <div
      className={`op-card ${item.ok === false ? "bad" : ""} ${open ? "open" : ""} ${item.live ? "live" : ""}`}
    >
      <header
        role="button"
        tabIndex={0}
        aria-expanded={open}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onToggle();
          }
        }}
      >
        <span className="op-icon">
          {item.icon || (item.ok === false ? "✗" : item.ok ? "✓" : "●")}
        </span>
        <span className="op-headline">
          {item.headline || item.tool}
          {item.live ? " · running" : ""}
        </span>
        <span className="op-chev">{open ? "▾" : "▸"}</span>
      </header>
      {open && (
        <div className="op-body">
          {item.fullOutput ? (
            <pre className="op-full">
              {item.fullOutput.slice(0, 4000)}
              {item.fullOutput.length > 4000 ? "\n… (truncated)" : ""}
            </pre>
          ) : (
            <p className="hint op-none">No output.</p>
          )}
          {(canRewind || canDiff) && (
            <div className="op-actions">
              {canDiff && item.path && onReviewDiff && (
                <button
                  type="button"
                  className="mini"
                  onClick={() => onReviewDiff(item.path as string)}
                >
                  Review diff
                </button>
              )}
              {canRewind && item.taskId && onRewind && (
                <button
                  type="button"
                  className="mini"
                  title="Undo every file change made by this task"
                  onClick={() => onRewind(item.taskId as string)}
                >
                  ↶ Rewind
                </button>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function ApprovalCard({
  approval,
  onDecide,
}: {
  approval: Approval;
  onDecide: (id: string, decision: "approve" | "deny") => void;
}) {
  return (
    <div className="approval" data-approval-id={approval.id}>
      <div className="approval-head">
        <span className="approval-kind">{approval.tool || "permission"}</span>
        <span className="approval-title">Allow ShadowCode to run this?</span>
      </div>
      <pre className="code">{approval.command || approval.reason}</pre>
      {approval.command && approval.reason && (
        <p className="hint">{approval.reason}</p>
      )}
      <div className="row approval-actions">
        <button
          type="button"
          className="ghost"
          onClick={() => onDecide(approval.id, "deny")}
        >
          Deny
        </button>
        <button
          type="button"
          className="primary"
          onClick={() => onDecide(approval.id, "approve")}
        >
          Allow
        </button>
      </div>
    </div>
  );
}

export function CommandCardView({ card }: { card: CommandResult }) {
  if (card.kind === "text" && card.text) {
    return (
      <div className="msg-agent">
        <div className="who">Command</div>
        {card.text}
      </div>
    );
  }
  if (card.kind === "error") {
    return (
      <div className="tool-card bad">
        <header>
          <span>
            {card.icon || "✗"} {card.headline}
          </span>
          <span>error</span>
        </header>
        <pre>{card.body}</pre>
      </div>
    );
  }
  if (card.kind === "diff") {
    return (
      <div className="tool-card">
        <header>
          <span>diff · {card.path}</span>
        </header>
        {card.diff
          .split("\n")
          .slice(0, 200)
          .map((line, i) => (
            <div
              key={i}
              className={`diff-line ${line.startsWith("+") ? "diff-add" : line.startsWith("-") ? "diff-del" : "diff-ctx"}`}
            >
              {line}
            </div>
          ))}
      </div>
    );
  }
  if (card.kind === "list") {
    return (
      <div className="tool-card">
        <header>
          <span>
            {card.icon || "◆"} {card.headline}
          </span>
        </header>
        {card.body && <pre className="plan">{card.body}</pre>}
        {card.items.map((item, i) => (
          <div key={i} className="status-row">
            <span>{item.label}</span>
            <code>{item.value}</code>
          </div>
        ))}
      </div>
    );
  }
  return (
    <div className="tool-card">
      <header>
        <span>
          {card.icon || "◆"} {card.headline}
        </span>
      </header>
      {card.body && <pre>{card.body}</pre>}
    </div>
  );
}

export function Empty({ title, body }: { title: string; body?: string }) {
  return (
    <div className="empty">
      <h3>{title}</h3>
      {body && <p>{body}</p>}
    </div>
  );
}
