import { useEffect, useId, useState, type ReactNode } from "react";
import { ExternalLink, RefreshCw, X } from "lucide-react";
import { Dialog } from "./Dialog";
import type { AllowanceResponse, AllowanceRow, LimitsConfig } from "../api";
import {
  accountKey,
  allowanceLevel,
  allowanceSummary,
  formatUsd,
  planName,
} from "../lib/allowance";
import {
  relativeTime,
  resetTime,
  shortName,
  type PickerTarget,
} from "../lib/picker";

/** Status-bar entry: a dot coloured by the worst state among ready sources
 * (low or at the plan limit → warning). */
export function AllowanceButton({
  data,
  open,
  onOpen,
}: {
  data: AllowanceResponse | null;
  open: boolean;
  onOpen: () => void;
}) {
  const level = allowanceLevel(data?.rows);
  const summary = allowanceSummary(data?.rows);
  return (
    <button
      type="button"
      className="allowance-trigger"
      aria-haspopup="dialog"
      aria-expanded={open}
      title={summary || "What each way of running a model has left"}
      onClick={onOpen}
    >
      <span className={`allowance-dot ${level}`} aria-hidden="true" />
      Allowance
      {summary && <span className="sr-only">: {summary}</span>}
    </button>
  );
}

const tone = (state: string) =>
  state === "low" || state === "limit_reached"
    ? "warn"
    : state === "ok"
      ? "ok"
      : "muted";

/** The Allowance dialog: every subscription, OpenRouter and this computer,
 * with what each reports. Nothing is estimated. */
export function AllowancePanel({
  data,
  loading,
  error,
  planLimit,
  now,
  onRefresh,
  onClose,
  onOpenAccounts,
  onOpenLocal,
}: {
  data: AllowanceResponse | null;
  loading: boolean;
  error?: string;
  /** "When a plan runs out" control, shown on the local row. */
  planLimit?: ReactNode;
  /** Unix seconds for relative times (tests pin it). */
  now?: number;
  onRefresh: () => void;
  onClose: () => void;
  /** Settings › Accounts, focused on this vendor ("claude", "openrouter"). */
  onOpenAccounts: (vendor: string) => void;
  onOpenLocal: () => void;
}) {
  const uid = useId();
  return (
    <Dialog
      label="Allowance"
      className="modal allowance-panel"
      onClose={onClose}
    >
      <div
        className="allowance-body"
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.stopPropagation();
            onClose();
          }
        }}
      >
        <header className="allowance-head">
          <div>
            <h2 id={`${uid}-title`}>Allowance</h2>
            <p className="hint">
              What each way of running a model has left, as each source reports
              it.
            </p>
          </div>
          <button
            type="button"
            className="ghost"
            disabled={loading}
            onClick={onRefresh}
          >
            <RefreshCw
              size={14}
              aria-hidden="true"
              className={loading ? "spin" : ""}
            />{" "}
            {loading ? "Checking…" : "Refresh"}
          </button>
          <button
            type="button"
            className="icon-btn"
            aria-label="Close Allowance"
            onClick={onClose}
          >
            <X size={16} aria-hidden="true" />
          </button>
        </header>
        {error && (
          <p className="health-bad" role="alert">
            {error}
          </p>
        )}
        {!data && !error && <p role="status">Checking allowance…</p>}
        {data && (
          <ul className="allowance-list" aria-labelledby={`${uid}-title`}>
            {data.rows.map((row) => (
              <li key={row.id}>
                <AllowanceItem
                  row={row}
                  now={now}
                  planLimit={row.id === "local" ? planLimit : undefined}
                  onOpenAccounts={onOpenAccounts}
                  onOpenLocal={onOpenLocal}
                />
              </li>
            ))}
          </ul>
        )}
      </div>
    </Dialog>
  );
}

function AllowanceItem({
  row,
  now,
  planLimit,
  onOpenAccounts,
  onOpenLocal,
}: {
  row: AllowanceRow;
  now?: number;
  planLimit?: ReactNode;
  onOpenAccounts: (vendor: string) => void;
  onOpenLocal: () => void;
}) {
  const percent =
    typeof row.remaining_percent === "number"
      ? Math.round(Math.min(100, Math.max(0, row.remaining_percent)))
      : null;
  const needsAccount =
    row.kind === "subscription" &&
    (row.state === "sign_in" || row.state === "not_installed");
  const needsKey = row.id === "openrouter" && row.state === "no_key";
  const windows = (row.windows || []).map((w) =>
    [
      w.label,
      w.remaining_percent != null
        ? `${Math.round(w.remaining_percent)}% left`
        : "",
      w.resets_at ? resetTime(w.resets_at, now) : "",
    ]
      .filter(Boolean)
      .join(" · "),
  );
  return (
    <article
      className={`allowance-row tone-${tone(row.state)}`}
      aria-label={row.product}
    >
      <header>
        <h3>{row.product}</h3>
        <span className={`allowance-headline tone-${tone(row.state)}`}>
          {row.headline}
        </span>
      </header>
      {percent !== null && (
        <div
          className="allowance-meter"
          role="meter"
          aria-label={`${row.product} remaining`}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
          aria-valuetext={`${percent}% left`}
        >
          <span style={{ width: `${percent}%` }} />
        </div>
      )}
      {(windows.length > 0 || row.plan) && (
        <ul className="allowance-details">
          {row.plan && <li>{planName(row.plan)}</li>}
          {windows.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      )}
      {row.id === "openrouter" && row.used != null && row.limit != null && (
        <p className="dim">{formatUsd(row.used)} used on this key</p>
      )}
      {row.note && <p className="dim">{row.note}</p>}
      {row.last_checked ? (
        <p className="dim">
          Last checked {relativeTime(row.last_checked, now)}
        </p>
      ) : null}
      {planLimit}
      {(row.usage_url || needsAccount || needsKey || row.state === "none") && (
        <div className="row allowance-actions">
          {needsAccount && (
            <button
              type="button"
              className="mini"
              aria-label={`Open Accounts for ${row.product}`}
              onClick={() => onOpenAccounts(accountKey(row))}
            >
              Open Accounts
            </button>
          )}
          {needsKey && (
            <button
              type="button"
              className="mini"
              aria-label="Add a key for OpenRouter"
              onClick={() => onOpenAccounts("openrouter")}
            >
              Add a key
            </button>
          )}
          {row.state === "none" && (
            <button type="button" className="mini" onClick={onOpenLocal}>
              Open Local models
            </button>
          )}
          {row.usage_url && (
            <a
              className="link"
              href={row.usage_url}
              aria-label={`Open usage page for ${row.product}`}
            >
              Open usage page <ExternalLink size={11} aria-hidden="true" />
            </a>
          )}
        </div>
      )}
    </article>
  );
}

/** "When a plan runs out": continue the conversation on a local model, or
 * ask. Saved as config `limits` (on_limit, fallback_model). */
export function PlanLimitControl({
  limits,
  localTargets,
  automaticName,
  onSave,
  onOpenLocal,
}: {
  limits: LimitsConfig;
  /** Ready local picker rows. */
  localTargets: PickerTarget[];
  /** The engine's automatic pick (Allowance › On this computer › fallback). */
  automaticName?: string;
  onSave: (limits: LimitsConfig) => Promise<void>;
  onOpenLocal?: () => void;
}) {
  const uid = useId();
  const [value, setValue] = useState<LimitsConfig>(limits);
  const [saving, setSaving] = useState(false);
  // Saved elsewhere (Settings or the Allowance panel): follow it.
  useEffect(() => {
    setValue(limits);
  }, [limits.on_limit, limits.fallback_model]);
  const onLimit = value.on_limit === "ask" ? "ask" : "local";
  const chosen = localTargets.find((t) => t.id === value.fallback_model);
  const name = chosen
    ? shortName(chosen)
    : value.fallback_model
      ? ""
      : automaticName || "";
  async function save(next: LimitsConfig) {
    const previous = value;
    setValue({ ...value, ...next });
    setSaving(true);
    try {
      await onSave(next);
    } catch {
      setValue(previous);
    } finally {
      setSaving(false);
    }
  }
  return (
    <fieldset className="plan-limit mode-options" aria-busy={saving}>
      <legend>When a plan runs out</legend>
      <label className="mode-option">
        <input
          type="radio"
          name={`${uid}-on-limit`}
          checked={onLimit === "local"}
          onChange={() => void save({ on_limit: "local" })}
        />
        <span>
          <strong>Continue on {name || "a local model"}</strong>
          <small>The same conversation keeps going on this computer.</small>
        </span>
      </label>
      <label className="mode-option">
        <input
          type="radio"
          name={`${uid}-on-limit`}
          checked={onLimit === "ask"}
          onChange={() => void save({ on_limit: "ask" })}
        />
        <span>
          <strong>Ask me</strong>
          <small>Stop and let me choose another model.</small>
        </span>
      </label>
      {onLimit === "local" &&
        (localTargets.length ? (
          <div className="plan-limit-model">
            <label htmlFor={`${uid}-model`}>Local model</label>
            <select
              id={`${uid}-model`}
              value={chosen ? chosen.id : ""}
              onChange={(e) => void save({ fallback_model: e.target.value })}
            >
              <option value="">
                {`Automatic${!value.fallback_model && automaticName ? ` (${automaticName})` : ""}`}
              </option>
              {localTargets.map((t) => (
                <option key={t.id} value={t.id}>
                  {shortName(t)}
                </option>
              ))}
            </select>
          </div>
        ) : (
          <p className="hint">
            No local model is ready yet.{" "}
            {onOpenLocal && (
              <button type="button" className="link" onClick={onOpenLocal}>
                Open Local models
              </button>
            )}
          </p>
        ))}
    </fieldset>
  );
}
