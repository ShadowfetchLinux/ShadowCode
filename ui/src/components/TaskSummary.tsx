import { useEffect, useState } from "react";
import { FileDiff, FlaskConical, Timer } from "lucide-react";
import { formatDuration, type TaskActivity } from "../lib/activity";

export type DiffStat = { add: number; del: number } | null;

/** Final card for a finished task: changed files (with +/- from the diff API
 * when it has them), recorded checks with exit codes, duration, and a way to
 * review the changes. */
export function TaskSummary({
  activity,
  onReview,
  onRewind,
  diffStat,
}: {
  activity: TaskActivity;
  onReview: (path?: string) => void;
  onRewind?: () => void;
  diffStat?: (path: string) => Promise<DiffStat>;
}) {
  const [stats, setStats] = useState<Record<string, DiffStat>>({});
  const changed = activity.changed;
  useEffect(() => {
    if (!diffStat || !changed.length) return;
    let live = true;
    void Promise.all(
      changed.slice(0, 20).map(async (path) => {
        try {
          return [path, await diffStat(path)] as const;
        } catch {
          return [path, null] as const;
        }
      }),
    ).then((rows) => {
      if (live) setStats(Object.fromEntries(rows));
    });
    return () => {
      live = false;
    };
  }, [diffStat, changed.join("\n")]);
  const verification = activity.verification;
  const duration =
    activity.startedAt && activity.finishedAt
      ? formatDuration(activity.finishedAt - activity.startedAt)
      : null;
  const outcome = activity.finished?.cancelled
    ? "Stopped"
    : activity.finished?.success
      ? "Finished"
      : "Finished with problems";
  return (
    <section
      className={`task-summary ${activity.finished?.success ? "" : "is-bad"}`}
      aria-label="Task summary"
    >
      <header>
        <strong>{outcome}</strong>
        {duration && (
          <span className="dim">
            <Timer size={12} aria-hidden="true" /> {duration}
          </span>
        )}
      </header>
      <div className="task-summary-block">
        <h4>
          <FileDiff size={13} aria-hidden="true" /> Changed files
        </h4>
        {changed.length ? (
          <ul>
            {changed.map((path) => {
              const stat = stats[path];
              return (
                <li key={path}>
                  <button
                    type="button"
                    className="link"
                    onClick={() => onReview(path)}
                  >
                    {path}
                  </button>
                  {stat && (stat.add > 0 || stat.del > 0) && (
                    <span className="diff-stat">
                      <span className="add">+{stat.add}</span>{" "}
                      <span className="del">−{stat.del}</span>
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
        ) : (
          <p className="dim">No file changes were recorded.</p>
        )}
      </div>
      <div className="task-summary-block">
        <h4>
          <FlaskConical size={13} aria-hidden="true" /> Checks
        </h4>
        {verification?.status === "vendor_owned" ? (
          <p className="dim">
            {verification.note ||
              "The vendor agent ran and judged its own checks; ShadowCode did not verify them."}
          </p>
        ) : verification?.commands.length ? (
          <ul>
            {verification.commands.map((c, i) => (
              <li key={`${c.command}-${i}`} className={c.success ? "" : "bad"}>
                <code>{c.command}</code>{" "}
                <span className={c.success ? "ok" : "bad"}>
                  {c.timed_out
                    ? "timed out"
                    : c.exit_code == null
                      ? c.success
                        ? "passed"
                        : "failed"
                      : `exit ${c.exit_code}`}
                </span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="dim">No test or build command was run.</p>
        )}
        {verification?.presentedAs === "unverified" && (
          <p className="warn-text">
            The answer claims results that no recorded check confirms.
          </p>
        )}
      </div>
      <div className="row task-summary-actions">
        <button type="button" className="mini" onClick={() => onReview()}>
          Review changes
        </button>
        {onRewind && changed.length > 0 && (
          <button
            type="button"
            className="mini ghost"
            title="Undo every file change made by this task"
            onClick={onRewind}
          >
            Rewind
          </button>
        )}
      </div>
    </section>
  );
}
