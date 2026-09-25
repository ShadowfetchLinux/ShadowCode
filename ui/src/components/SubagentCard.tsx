import { useState } from "react";
import {
  Bot,
  Check,
  ChevronRight,
  CircleAlert,
  ExternalLink,
  LoaderCircle,
} from "lucide-react";
import { Markdown } from "./Markdown";
import { statusLabel, type SubagentRun } from "../lib/subagents";

/** A subagent run inside the parent transcript: one line while collapsed,
 * its result, changed files and a link to its own conversation when open. */
export function SubagentCard({
  run,
  onOpen,
}: {
  run: SubagentRun;
  /** Open the child's conversation (hidden from the sidebar). */
  onOpen?: (sessionId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const running = run.status === "running" || run.status === "queued";
  const ok = run.status === "completed";
  const tone = running ? "running" : ok ? "ok" : "warn";
  const label = run.description || run.prompt;
  const bodyId = `subagent-${run.runId}`;
  return (
    <div className={`subagent-card tone-${tone}`}>
      <button
        type="button"
        className="subagent-head"
        aria-expanded={open}
        aria-controls={bodyId}
        onClick={() => setOpen((v) => !v)}
      >
        <ChevronRight
          size={14}
          aria-hidden="true"
          className={`subagent-chevron ${open ? "is-open" : ""}`}
        />
        <Bot size={14} aria-hidden="true" />
        <strong className="subagent-name">@{run.agent}</strong>
        <span className="subagent-label">{label}</span>
        <span className="subagent-meta">
          {run.mode === "write" ? "worktree" : "read-only"}
          {run.files.length > 0 &&
            ` · ${run.files.length} file${run.files.length === 1 ? "" : "s"}`}
          {run.applied && " · applied"}
        </span>
        <span className={`subagent-status tone-${tone}`} role="status">
          {running ? (
            <LoaderCircle size={13} className="spin" aria-hidden="true" />
          ) : ok ? (
            <Check size={13} aria-hidden="true" />
          ) : (
            <CircleAlert size={13} aria-hidden="true" />
          )}
          {statusLabel(run)}
        </span>
      </button>
      {open && (
        <div className="subagent-body" id={bodyId}>
          {run.prompt && (
            <p className="subagent-prompt">
              <span>Task</span> {run.prompt}
            </p>
          )}
          {run.summary && !running && (
            <div className="subagent-summary">
              <Markdown>{run.summary}</Markdown>
            </div>
          )}
          {run.error && run.error !== run.summary && (
            <p className="subagent-error">{run.error}</p>
          )}
          {run.files.length > 0 && (
            <ul className="subagent-files" aria-label="Changed files">
              {run.files.map((f) => (
                <li key={f.path}>
                  <code>{f.path}</code>
                  <span className="subagent-stat">
                    {f.binary ? (
                      "binary"
                    ) : (
                      <>
                        <span className="add">+{f.additions}</span>{" "}
                        <span className="del">−{f.deletions}</span>
                      </>
                    )}
                  </span>
                </li>
              ))}
              {run.filesTruncated && <li>More files not listed</li>}
            </ul>
          )}
          {run.mode === "write" && !running && (
            <p className="subagent-note">
              {run.applied
                ? "The parent agent applied these changes to the project."
                : run.patch
                  ? "Changes were made in an isolated worktree. The parent agent reviews the diff and applies it with your usual edit approval."
                  : "No changes to apply."}
            </p>
          )}
          {run.notes.map((note) => (
            <p key={note} className="subagent-note">
              {note}
            </p>
          ))}
          <div className="subagent-foot">
            <span>
              {[
                run.model,
                run.steps ? `${run.steps} steps` : "",
                run.tokens ? `${run.tokens.toLocaleString()} tokens` : "",
                run.durationS != null ? `${run.durationS}s` : "",
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
            {onOpen && run.sessionId && (
              <button
                type="button"
                className="ghost"
                onClick={() => onOpen(run.sessionId)}
              >
                <ExternalLink size={13} aria-hidden="true" /> Open transcript
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
