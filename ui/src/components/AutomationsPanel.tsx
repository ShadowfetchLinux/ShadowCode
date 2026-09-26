/** Tools › Automations: prompts that run on a schedule, each run a normal
 * task in its own conversation (by default in a fresh worktree). The list
 * shows the next time and the last result; the editor previews the next
 * three times; the history shows status, duration and cost per run. */
import { useCallback, useEffect, useState } from "react";
import { Clock, History, Pause, Play, Square } from "lucide-react";
import { api } from "../api";
import {
  automationsApi,
  DAYS,
  defaultDraft,
  draftOf,
  formatCost,
  formatDuration,
  formatWhen,
  nextRunText,
  runText,
  TRIGGER_TEXT,
  withKind,
  type Automation,
  type AutomationDraft,
  type AutomationRun,
  type Schedule,
  type SchedulePreview,
} from "../lib/automations";
import { forgeApi } from "../lib/forge";
import { isReady, type PickerTarget } from "../lib/picker";
import { Empty } from "./cards";
import "../tools.css";

type Toast = (text: string, kind?: "ok" | "err" | "info") => void;

const nowSeconds = () => Date.now() / 1000;

export function AutomationsPanel({
  toast,
  onOpenSession,
}: {
  toast: Toast;
  onOpenSession: (id: string) => void;
}) {
  const [rows, setRows] = useState<Automation[] | null>(null);
  const [scheduler, setScheduler] = useState(true);
  const [error, setError] = useState("");
  const [editing, setEditing] = useState<{
    id: string;
    draft: AutomationDraft;
  } | null>(null);
  const [history, setHistory] = useState<string>("");
  const [runs, setRuns] = useState<AutomationRun[]>([]);
  const [pending, setPending] = useState("");
  const [repo, setRepo] = useState(false);
  const [now, setNow] = useState(nowSeconds);

  const load = useCallback(async () => {
    try {
      const list = await automationsApi.list();
      setRows(list.automations);
      setScheduler(list.scheduler);
      setError("");
      setNow(nowSeconds());
    } catch (e) {
      setError(String(e));
      setRows((current) => current ?? []);
    }
  }, []);
  const running = Boolean(rows?.some((a) => a.running_run));
  useEffect(() => {
    void load();
    const timer = setInterval(() => void load(), running ? 3000 : 20000);
    return () => clearInterval(timer);
  }, [load, running]);
  useEffect(() => {
    void forgeApi
      .overview()
      .then((o) => setRepo(Boolean(o.repo && o.has_commits !== false)))
      .catch(() => setRepo(false));
  }, []);
  const loadRuns = useCallback(
    async (id: string) => {
      try {
        setRuns((await automationsApi.get(id)).runs || []);
      } catch (e) {
        setRuns([]);
        toast(String(e), "err");
      }
    },
    [toast],
  );
  useEffect(() => {
    if (history) void loadRuns(history);
  }, [history, loadRuns, rows]);

  async function act(id: string, label: string, run: () => Promise<unknown>) {
    if (pending) return;
    setPending(`${id}:${label}`);
    try {
      await run();
      await load();
    } catch (e) {
      toast(String(e), "err");
    } finally {
      setPending("");
    }
  }

  if (editing)
    return (
      <AutomationEditor
        initial={editing.draft}
        existing={Boolean(editing.id)}
        onCancel={() => setEditing(null)}
        onSave={async (draft) => {
          if (editing.id) await automationsApi.update(editing.id, draft);
          else await automationsApi.create(draft);
          toast(editing.id ? "Automation saved" : "Automation created", "ok");
          setEditing(null);
          await load();
        }}
      />
    );

  return (
    <div className="automations">
      <div className="row">
        <p className="hint grow">
          Prompts that run on a schedule while ShadowCode is open, each in its
          own conversation.
        </p>
        <button
          type="button"
          className="mini primary-mini"
          onClick={() => setEditing({ id: "", draft: defaultDraft(repo) })}
        >
          New automation
        </button>
      </div>
      {!scheduler && (
        <p className="notice" role="status">
          Schedules run while the ShadowCode window or{" "}
          <code>shadowcode serve</code> is open.
        </p>
      )}
      {error && (
        <p className="notice bad" role="alert">
          {error}
        </p>
      )}
      {rows === null && (
        <div className="skel-rows">
          <span className="skel" />
          <span className="skel short" />
        </div>
      )}
      {rows?.length === 0 && !error && (
        <Empty
          title="No automations yet"
          body="For example: every weekday at 9:00, review yesterday's commits and list anything risky."
        />
      )}
      {rows?.map((a) => {
        const busy = pending.startsWith(`${a.id}:`);
        return (
          <article
            key={a.id}
            className={`automation ${a.paused ? "paused" : ""}`}
            aria-label={a.name}
          >
            <header>
              <strong>{a.name}</strong>
              <span className="hint">
                <Clock size={12} aria-hidden="true" /> {a.description}
              </span>
            </header>
            <p className="automation-next">
              <span>{nextRunText(a, now)}</span>
              {a.last_run && (
                <span className={`run-status ${a.last_run.status}`}>
                  Last: {runText(a.last_run)}
                </span>
              )}
            </p>
            <div className="row">
              {a.running_run ? (
                <button
                  type="button"
                  className="mini"
                  disabled={busy}
                  onClick={() =>
                    void act(a.id, "stop", () => automationsApi.stop(a.id))
                  }
                >
                  <Square size={12} aria-hidden="true" /> Stop
                </button>
              ) : (
                <button
                  type="button"
                  className="mini"
                  disabled={busy}
                  onClick={() =>
                    void act(a.id, "run", async () => {
                      await automationsApi.runNow(a.id);
                      toast(`Started ${a.name}`, "ok");
                    })
                  }
                >
                  <Play size={12} aria-hidden="true" /> Run now
                </button>
              )}
              <button
                type="button"
                className="mini"
                disabled={busy}
                onClick={() =>
                  void act(a.id, "pause", () =>
                    a.paused
                      ? automationsApi.resume(a.id)
                      : automationsApi.pause(a.id),
                  )
                }
              >
                {a.paused ? (
                  <>
                    <Play size={12} aria-hidden="true" /> Resume
                  </>
                ) : (
                  <>
                    <Pause size={12} aria-hidden="true" /> Pause
                  </>
                )}
              </button>
              <button
                type="button"
                className="mini"
                aria-expanded={history === a.id}
                onClick={() => setHistory(history === a.id ? "" : a.id)}
              >
                <History size={12} aria-hidden="true" /> History
              </button>
              <button
                type="button"
                className="mini"
                onClick={() => setEditing({ id: a.id, draft: draftOf(a) })}
              >
                Edit
              </button>
              <button
                type="button"
                className="mini danger-text"
                disabled={busy || Boolean(a.running_run)}
                onClick={() => {
                  if (
                    !window.confirm(
                      `Delete the automation "${a.name}" and its history? Conversations it created stay.`,
                    )
                  )
                    return;
                  void act(a.id, "delete", () => automationsApi.remove(a.id));
                }}
              >
                Delete
              </button>
            </div>
            {history === a.id && (
              <RunHistory runs={runs} now={now} onOpenSession={onOpenSession} />
            )}
          </article>
        );
      })}
    </div>
  );
}

function RunHistory({
  runs,
  now,
  onOpenSession,
}: {
  runs: AutomationRun[];
  now: number;
  onOpenSession: (id: string) => void;
}) {
  if (!runs.length) return <p className="hint">No runs yet.</p>;
  return (
    <ul className="run-history" aria-label="Run history">
      {runs.map((run) => {
        const cost = formatCost(run);
        const duration = formatDuration(run.duration);
        return (
          <li key={run.id}>
            <div className="row">
              <span className={`run-status ${run.status}`}>{runText(run)}</span>
              <span className="hint">
                {formatWhen(run.scheduled_for || run.started_at, now)} ·{" "}
                {TRIGGER_TEXT[run.trigger] || run.trigger}
                {duration ? ` · ${duration}` : ""}
                {cost ? ` · ${cost}` : ""}
              </span>
              {run.session_id && (
                <button
                  type="button"
                  className="link-btn"
                  onClick={() => onOpenSession(run.session_id!)}
                >
                  Open conversation
                </button>
              )}
            </div>
            {(run.detail || run.summary) && (
              <p className="hint run-detail">
                {run.detail || run.summary?.slice(0, 300)}
              </p>
            )}
          </li>
        );
      })}
    </ul>
  );
}

export function AutomationEditor({
  initial,
  existing,
  onSave,
  onCancel,
}: {
  initial: AutomationDraft;
  existing: boolean;
  onSave: (draft: AutomationDraft) => Promise<void>;
  onCancel: () => void;
}) {
  const [draft, setDraft] = useState(initial);
  const [preview, setPreview] = useState<SchedulePreview | null>(null);
  const [targets, setTargets] = useState<PickerTarget[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const set = <K extends keyof AutomationDraft>(
    key: K,
    value: AutomationDraft[K],
  ) => setDraft((d) => ({ ...d, [key]: value }));
  const option = <K extends keyof AutomationDraft["options"]>(
    key: K,
    value: AutomationDraft["options"][K],
  ) => setDraft((d) => ({ ...d, options: { ...d.options, [key]: value } }));
  const schedule = (next: Schedule) => set("schedule", next);

  useEffect(() => {
    void api
      .picker()
      .then((p) => setTargets(p.targets.filter(isReady)))
      .catch(() => setTargets([]));
  }, []);
  const scheduleKey = JSON.stringify([draft.schedule, draft.timezone]);
  useEffect(() => {
    let current = true;
    const timer = setTimeout(
      () =>
        void automationsApi
          .preview(draft.schedule, draft.timezone)
          .then((p) => current && setPreview(p))
          .catch((e) => current && setPreview({ ok: false, error: String(e) })),
      250,
    );
    return () => {
      current = false;
      clearTimeout(timer);
    };
    // The key covers the schedule and time zone.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scheduleKey]);

  const s = draft.schedule;
  const canSave =
    draft.name.trim() && draft.prompt.trim() && preview?.ok !== false;
  return (
    <form
      className="automation-editor"
      aria-label={existing ? "Edit automation" : "New automation"}
      onSubmit={(e) => {
        e.preventDefault();
        if (!canSave || saving) return;
        setSaving(true);
        setError("");
        void onSave(draft)
          .catch((err) => setError(String(err)))
          .finally(() => setSaving(false));
      }}
    >
      <label>
        Name
        <input
          value={draft.name}
          maxLength={80}
          placeholder="Morning review"
          onChange={(e) => set("name", e.target.value)}
        />
      </label>
      <label>
        What should it do?
        <textarea
          rows={5}
          value={draft.prompt}
          placeholder="Review the commits since yesterday and list anything that looks risky."
          onChange={(e) => set("prompt", e.target.value)}
        />
      </label>
      <div className="row">
        <label className="grow">
          Model
          <select
            value={draft.model}
            onChange={(e) => set("model", e.target.value)}
          >
            <option value="">The project's model</option>
            {draft.model && !targets.some((t) => t.id === draft.model) && (
              <option value={draft.model}>{draft.model}</option>
            )}
            {targets.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Mode
          <select
            value={draft.mode}
            onChange={(e) =>
              set("mode", e.target.value as AutomationDraft["mode"])
            }
          >
            <option value="code">Code (can edit files)</option>
            <option value="plan">Plan (read-only)</option>
            <option value="ask">Ask (read-only)</option>
          </select>
        </label>
      </div>

      <fieldset>
        <legend>When</legend>
        <div className="row">
          <select
            aria-label="Repeat"
            value={s.kind}
            onChange={(e) =>
              schedule(withKind(s, e.target.value as Schedule["kind"]))
            }
          >
            <option value="hourly">Every hour</option>
            <option value="daily">Every day</option>
            <option value="weekdays">Weekdays (Mon–Fri)</option>
            <option value="weekly">Every week</option>
            <option value="cron">Custom (cron)</option>
          </select>
          {s.kind === "hourly" && (
            <label className="inline">
              at minute
              <input
                aria-label="Minute past the hour"
                type="number"
                min={0}
                max={59}
                value={s.minute}
                onChange={(e) =>
                  schedule({
                    kind: "hourly",
                    minute: Math.max(
                      0,
                      Math.min(59, Number(e.target.value) || 0),
                    ),
                  })
                }
              />
            </label>
          )}
          {s.kind === "weekly" && (
            <select
              aria-label="Day of the week"
              value={s.day}
              onChange={(e) => schedule({ ...s, day: Number(e.target.value) })}
            >
              {DAYS.map((day, index) => (
                <option key={day} value={index}>
                  {day}
                </option>
              ))}
            </select>
          )}
          {"time" in s && (
            <input
              aria-label="Time"
              type="time"
              value={s.time}
              onChange={(e) => schedule({ ...s, time: e.target.value })}
            />
          )}
          {s.kind === "cron" && (
            <input
              aria-label="Cron expression"
              className="mono"
              value={s.expr}
              placeholder="minute hour day month weekday"
              onChange={(e) => schedule({ kind: "cron", expr: e.target.value })}
            />
          )}
          <select
            aria-label="Time zone"
            value={draft.timezone}
            onChange={(e) =>
              set("timezone", e.target.value as AutomationDraft["timezone"])
            }
          >
            <option value="local">This computer's time</option>
            <option value="utc">UTC</option>
          </select>
        </div>
        <SchedulePreviewText preview={preview} />
      </fieldset>

      <fieldset>
        <legend>How it runs</legend>
        <label className="check">
          <input
            type="radio"
            name="checkout"
            checked={draft.options.checkout === "worktree"}
            onChange={() => option("checkout", "worktree")}
          />{" "}
          In a fresh worktree (your checkout is never touched)
        </label>
        <label className="check">
          <input
            type="radio"
            name="checkout"
            checked={draft.options.checkout === "main"}
            onChange={() => option("checkout", "main")}
          />{" "}
          In the project folder (waits for other tasks there)
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.options.permission === "read_only"}
            onChange={(e) =>
              option("permission", e.target.checked ? "read_only" : "project")
            }
          />{" "}
          Read-only, even in Code mode
        </label>
        <p className="hint">
          Runs never get more permission than this project has.
        </p>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.options.on_approval === "wait"}
            onChange={(e) =>
              option("on_approval", e.target.checked ? "wait" : "stop")
            }
          />{" "}
          When it asks for approval, wait for me (otherwise the run stops)
        </label>
        <div className="row">
          <label className="inline">
            Stop after
            <input
              aria-label="Time limit in minutes"
              type="number"
              min={1}
              max={1440}
              value={draft.options.max_runtime_minutes}
              onChange={(e) =>
                option(
                  "max_runtime_minutes",
                  Math.max(1, Math.min(1440, Number(e.target.value) || 1)),
                )
              }
            />
            minutes
          </label>
          <label className="inline">
            Catch up missed runs within
            <input
              aria-label="Catch-up window in minutes"
              type="number"
              min={0}
              max={10080}
              value={draft.options.catch_up_minutes}
              onChange={(e) =>
                option(
                  "catch_up_minutes",
                  Math.max(0, Math.min(10080, Number(e.target.value) || 0)),
                )
              }
            />
            minutes
          </label>
        </div>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.options.notify}
            onChange={(e) => option("notify", e.target.checked)}
          />{" "}
          Notify me when it finishes
        </label>
      </fieldset>
      {error && (
        <p className="notice bad" role="alert">
          {error}
        </p>
      )}
      <div className="row end">
        <button type="button" className="mini" onClick={onCancel}>
          Cancel
        </button>
        <button
          type="submit"
          className="mini primary-mini"
          disabled={!canSave || saving}
        >
          {saving ? "Saving…" : existing ? "Save" : "Create automation"}
        </button>
      </div>
    </form>
  );
}

export function SchedulePreviewText({
  preview,
}: {
  preview: SchedulePreview | null;
}) {
  if (!preview) return <p className="hint">Working out the next times…</p>;
  if (!preview.ok)
    return (
      <p className="hint error" role="alert">
        {preview.error}
      </p>
    );
  return (
    <p className="hint" aria-label="Next runs">
      {preview.description}. Next:{" "}
      {preview.next.map((t) => formatWhen(t, preview.now)).join(", ")}
    </p>
  );
}
