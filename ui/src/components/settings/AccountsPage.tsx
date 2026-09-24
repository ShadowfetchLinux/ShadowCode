import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type FormEvent,
} from "react";
import { RefreshCw } from "lucide-react";
import {
  api,
  type LoginProgress,
  type OpenRouterStatus,
  type VendorStatus,
} from "../../api";
import { Dialog } from "../Dialog";
import { listen } from "../../lib/transport";
import { relativeTime, usageDetailLines, usageLabel } from "../../lib/picker";

const ORDER = ["codex", "claude", "cursor", "antigravity", "grok"];

export const vendorId = (key: string) => key.replace(/^cli[-:]/, "");

type Login = {
  vendor: string;
  state: "running" | "done" | "failed" | "unsupported";
  lines: string[];
  detail?: string;
};

/** Settings › Accounts: the official CLI of each subscription, its sign-in
 * state, models and usage. Sign-in and sign-out run the vendors' own commands.
 * API keys (OpenRouter, billed per token) follow in their own card. */
export function AccountsPage({
  focusVendor,
  onChanged,
  onToast,
}: {
  focusVendor?: string;
  /** Called after connect/disconnect/refresh so the picker reloads. */
  onChanged: () => void;
  onToast: (text: string, kind: "ok" | "err" | "info") => void;
}) {
  const [vendors, setVendors] = useState<Record<string, VendorStatus> | null>(
    null,
  );
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [login, setLogin] = useState<Login | null>(null);
  const [confirm, setConfirm] = useState<VendorStatus | null>(null);
  const [checks, setChecks] = useState(0);
  const rows = useRef<Record<string, HTMLElement | null>>({});

  const load = useCallback(async (refresh = false) => {
    setLoading(true);
    setError("");
    try {
      const result = await api.accounts(refresh);
      const next: Record<string, VendorStatus> = {};
      for (const [key, value] of Object.entries(result.vendors || {}))
        next[vendorId(key)] = value;
      setVendors(next);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => {
    if (!focusVendor || !vendors) return;
    const row = rows.current[focusVendor];
    row?.scrollIntoView?.({ block: "nearest" });
    row?.querySelector<HTMLElement>("[data-primary]")?.focus();
  }, [focusVendor, vendors]);

  const running = login?.state === "running" ? login.vendor : null;
  // Follow a running sign-in: engine events wake the reader; a slow poll covers
  // missed wake-ups.
  useEffect(() => {
    if (!running) return;
    let live = true;
    let reading = false;
    async function read() {
      if (!live || reading) return;
      reading = true;
      try {
        const progress: LoginProgress = await api.loginProgress(running!);
        if (!live) return;
        setLogin((current) =>
          current && current.vendor === running
            ? {
                ...current,
                lines: progress.lines?.length ? progress.lines : current.lines,
                state: progress.done
                  ? progress.done.ok
                    ? "done"
                    : "failed"
                  : "running",
                detail: progress.done?.detail || current.detail,
              }
            : current,
        );
        if (progress.done) {
          await refreshOne(running!);
          onChanged();
        }
      } catch {
        // The progress route is optional; keep the instructions visible and
        // let Refresh confirm the result.
      } finally {
        reading = false;
      }
    }
    let stop: (() => void) | undefined;
    void listen("shadowcode:events", () => void read())
      .then((unlisten) => {
        if (live) stop = unlisten;
        else unlisten();
      })
      .catch(() => undefined);
    const timer = setInterval(() => void read(), 1500);
    void read();
    return () => {
      live = false;
      clearInterval(timer);
      stop?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [running]);

  async function refreshOne(vendor: string) {
    setBusy((b) => ({ ...b, [vendor]: true }));
    try {
      const status = await api.refreshAccount(vendor);
      setVendors((current) => ({ ...(current || {}), [vendor]: status }));
    } catch (e) {
      onToast(String(e), "err");
    } finally {
      setBusy((b) => ({ ...b, [vendor]: false }));
    }
  }

  async function connect(vendor: string) {
    setBusy((b) => ({ ...b, [vendor]: true }));
    try {
      const result = await api.connectAccount(vendor);
      if (result.state === "unsupported")
        setLogin({
          vendor,
          state: "unsupported",
          lines: [],
          detail:
            result.hint || result.note || "Sign in with the vendor's own app.",
        });
      else
        setLogin({
          vendor,
          state: "running",
          lines: result.lines || [],
          detail: result.note,
        });
    } catch (e) {
      onToast(String(e), "err");
    } finally {
      setBusy((b) => ({ ...b, [vendor]: false }));
    }
  }

  async function cancelLogin(vendor: string) {
    try {
      await api.cancelLogin(vendor);
      setLogin((l) =>
        l ? { ...l, state: "failed", detail: "Sign-in cancelled." } : l,
      );
    } catch (e) {
      onToast(String(e), "err");
    }
  }

  async function disconnect(status: VendorStatus, vendor: string) {
    setConfirm(null);
    setBusy((b) => ({ ...b, [vendor]: true }));
    try {
      const result = await api.disconnectAccount(vendor);
      onToast(result.note || `Signed out of ${status.label}.`, "ok");
      await refreshOne(vendor);
      onChanged();
    } catch (e) {
      onToast(String(e), "err");
    } finally {
      setBusy((b) => ({ ...b, [vendor]: false }));
    }
  }

  const keys = vendors
    ? [
        ...ORDER.filter((k) => vendors[k]),
        ...Object.keys(vendors).filter((k) => !ORDER.includes(k)),
      ]
    : [];
  return (
    <section className="settings-page">
      <div className="settings-page-head">
        <div>
          <h3>Accounts</h3>
          <p className="hint">
            Subscriptions run through each vendor's official command-line tool.
            Signing in opens the vendor's own page; ShadowCode never asks for a
            password and never reads vendor tokens. API keys are billed per
            token by the provider and stay on this computer.
          </p>
        </div>
        <button
          type="button"
          className="ghost"
          disabled={loading}
          onClick={() => {
            setChecks((n) => n + 1);
            void load(true).then(onChanged);
          }}
        >
          <RefreshCw size={14} aria-hidden="true" />{" "}
          {loading ? "Checking…" : "Check all"}
        </button>
      </div>
      {error && (
        <p className="health-bad" role="alert">
          {error}
        </p>
      )}
      {!vendors && !error && <p role="status">Checking accounts…</p>}
      {keys.map((key) => {
        const status = vendors![key];
        const ready = status.state === "ready";
        const apiKey = /api.?key/i.test(status.account?.auth_mode || "");
        // The account line already names the plan; don't repeat it below.
        const usageLines = usageDetailLines(status.usage).filter(
          (line) => !(status.account?.plan && / plan$/.test(line)),
        );
        const models = status.models || [];
        const thisLogin = login?.vendor === key ? login : null;
        return (
          <article
            className="account-card"
            key={key}
            ref={(el) => {
              rows.current[key] = el;
            }}
            aria-labelledby={`account-${key}`}
          >
            <header>
              <h4 id={`account-${key}`}>{status.product || status.label}</h4>
              <span className={`avail avail-${status.availability}`}>
                {status.availability_label}
              </span>
              {status.version && <span className="dim">{status.version}</span>}
            </header>
            {/* Ready rows: the header already shows state and version; the
                account line says who is signed in. Other states explain why. */}
            {!ready && status.detail && <p>{status.detail}</p>}
            {ready &&
              status.account &&
              (status.account.email || status.account.plan) && (
                <p>
                  {[
                    status.account.email,
                    status.account.plan &&
                      `${status.account.plan[0].toUpperCase()}${status.account.plan.slice(1)} plan`,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </p>
              )}
            {apiKey && (
              <p className="warn-text">API key login · billed per token</p>
            )}
            {!ready && status.fix && <p className="hint">{status.fix}</p>}
            {status.error && <p className="health-bad">{status.error}</p>}
            {ready && (
              <div className="account-usage">
                {/* With reported windows the list below says it all; the
                    one-line summary is for rows without them. */}
                <p>
                  <strong>Usage</strong>
                  {status.usage?.windows?.length
                    ? ""
                    : // The usage link sits right below; drop the label's
                      // "· Open … usage" pointer to it.
                      `: ${usageLabel(status.usage, "cloud")
                        .replace(/ · Open .* usage$/, "")
                        .replace(/^Usage unavailable/, "unavailable")}`}
                </p>
                {usageLines.length > 0 && (
                  <ul>
                    {usageLines.map((line) => (
                      <li key={line}>{line}</li>
                    ))}
                  </ul>
                )}
                {status.usage_note &&
                  !usageLines.includes(status.usage_note) && (
                    <p className="hint">{status.usage_note}</p>
                  )}
                {status.usage?.provider_usage_url && (
                  <a
                    href={status.usage.provider_usage_url}
                    target="_blank"
                    rel="noreferrer"
                  >
                    Open {status.product || status.label} usage
                  </a>
                )}
              </div>
            )}
            {ready && models.length > 0 && (
              <details className="account-models">
                <summary>
                  {models.length} model{models.length === 1 ? "" : "s"}{" "}
                  available
                </summary>
                <ul>
                  {models.map((m) => (
                    <li key={m.id}>
                      {m.label || m.id}
                      {m.is_default && <span className="dim"> · default</span>}
                      {m.vision && <span className="dim"> · vision</span>}
                    </li>
                  ))}
                </ul>
              </details>
            )}
            {thisLogin && (
              <LoginPanel
                login={thisLogin}
                onCancel={() => void cancelLogin(key)}
              />
            )}
            <div className="row">
              {!ready && status.state !== "not_installed" && (
                <button
                  type="button"
                  className="primary"
                  data-primary
                  disabled={busy[key] || thisLogin?.state === "running"}
                  onClick={() => void connect(key)}
                >
                  {thisLogin?.state === "running" ? "Signing in…" : "Connect"}
                </button>
              )}
              <button
                type="button"
                className="ghost"
                data-primary={
                  ready || status.state === "not_installed" ? true : undefined
                }
                disabled={busy[key]}
                onClick={() => void refreshOne(key).then(onChanged)}
              >
                {busy[key] ? "Checking…" : "Refresh"}
              </button>
              {ready && (
                <button
                  type="button"
                  className="ghost danger-text"
                  disabled={busy[key]}
                  onClick={() => setConfirm(status)}
                >
                  Disconnect
                </button>
              )}
            </div>
          </article>
        );
      })}
      <OpenRouterCard
        focus={focusVendor === "openrouter"}
        checks={checks}
        onChanged={onChanged}
        onToast={onToast}
      />
      {confirm && (
        <Dialog
          label={`Disconnect ${confirm.label}`}
          className="modal modal-sm"
          onClose={() => setConfirm(null)}
        >
          <h2>Disconnect {confirm.label}?</h2>
          <p>
            {confirm.shared_cli_note ||
              `This signs out the ${confirm.binary || confirm.label} command-line tool for your whole user account, not just ShadowCode.`}
          </p>
          {confirm.logout_command?.length ? (
            <p className="hint">
              Runs <code>{confirm.logout_command.join(" ")}</code>
            </p>
          ) : null}
          <div className="row end">
            <button
              type="button"
              className="ghost"
              onClick={() => setConfirm(null)}
            >
              Cancel
            </button>
            <button
              type="button"
              className="primary danger"
              onClick={() => void disconnect(confirm, vendorId(confirm.id))}
            >
              Disconnect
            </button>
          </div>
        </Dialog>
      )}
    </section>
  );
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** "$0.0042", "$1.20": small per-token spend stays visible. */
export function formatUsd(value: number): string {
  if (!Number.isFinite(value)) return "$0.00";
  const abs = Math.abs(value);
  return `$${abs > 0 && abs < 0.01 ? value.toFixed(4) : value.toFixed(2)}`;
}

/** Accounts › OpenRouter: a pay-per-token API key saved on this computer.
 * The key is write-only here; the engine never sends it back. */
function OpenRouterCard({
  focus,
  checks,
  onChanged,
  onToast,
}: {
  /** Opened from the picker (connect("openrouter")): scroll here and focus. */
  focus: boolean;
  /** Bumped by "Check all" to reread the status. */
  checks: number;
  onChanged: () => void;
  onToast: (text: string, kind: "ok" | "err" | "info") => void;
}) {
  const [status, setStatus] = useState<OpenRouterStatus | null>(null);
  const [loadError, setLoadError] = useState("");
  const [key, setKey] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<"" | "save" | "refresh" | "remove">("");
  const [confirm, setConfirm] = useState(false);
  const card = useRef<HTMLElement>(null);
  const focusAfter = useRef(focus);
  const uid = useId().replace(/:/g, "");

  useEffect(() => {
    let live = true;
    api
      .openrouterStatus()
      .then((next) => {
        if (!live) return;
        setStatus(next);
        setLoadError("");
      })
      .catch((e) => live && setLoadError(message(e)));
    return () => {
      live = false;
    };
  }, [checks]);

  useEffect(() => {
    if (focus) focusAfter.current = true;
  }, [focus]);
  // Move focus to the card's main control once it is on screen: after the
  // picker sent the user here, and after save/remove replaced the controls.
  useEffect(() => {
    if (!status || !focusAfter.current) return;
    focusAfter.current = false;
    card.current?.scrollIntoView?.({ block: "nearest" });
    card.current?.querySelector<HTMLElement>("[data-primary]")?.focus();
  }, [status]);

  async function save(event: FormEvent) {
    event.preventDefault();
    if (!key.trim() || busy) return;
    setBusy("save");
    setError("");
    try {
      const next = await api.setOpenrouterKey(key);
      setKey("");
      focusAfter.current = true;
      setStatus(next);
      onToast("OpenRouter key saved.", "ok");
      onChanged();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy("");
    }
  }

  async function refresh() {
    setBusy("refresh");
    try {
      setStatus(await api.refreshOpenrouter());
      onChanged();
    } catch (e) {
      onToast(message(e), "err");
    } finally {
      setBusy("");
    }
  }

  async function remove() {
    setConfirm(false);
    setBusy("remove");
    try {
      const next = await api.removeOpenrouterKey();
      focusAfter.current = true;
      setStatus(next);
      onToast("OpenRouter key removed from this computer.", "ok");
      onChanged();
    } catch (e) {
      onToast(message(e), "err");
    } finally {
      setBusy("");
    }
  }

  const offline = Boolean(status?.offline);
  const keysUrl = status?.keys_url || "https://openrouter.ai/keys";
  const info = status?.key;
  const [availability, availabilityText] = !status
    ? ["", ""]
    : offline
      ? ["unavailable", "Offline"]
      : !status.key_set
        ? ["sign_in", "Add API key"]
        : status.key_error
          ? ["sign_in", "Check key"]
          : ["ready", "Ready"];
  return (
    <article
      className="account-card"
      ref={card}
      aria-labelledby={`${uid}-title`}
    >
      <header>
        <h4 id={`${uid}-title`}>OpenRouter</h4>
        <span className="cap-badge">API key</span>
        {availabilityText && (
          <span className={`avail avail-${availability}`}>
            {availabilityText}
          </span>
        )}
      </header>
      {loadError && <p className="health-bad">{loadError}</p>}
      {!status && !loadError && <p role="status">Checking OpenRouter…</p>}
      {offline && <p className="warn-text">Offline mode: OpenRouter is off.</p>}
      {status && !status.key_set && (
        <form className="openrouter-form" onSubmit={(e) => void save(e)}>
          <p>
            Pay per token for hundreds of models. Create a key at{" "}
            <a href={keysUrl} target="_blank" rel="noreferrer">
              openrouter.ai/keys
            </a>
            ; ShadowCode stores it only on this computer.
          </p>
          <div className="field">
            <label htmlFor={`${uid}-key`}>OpenRouter API key</label>
            <input
              id={`${uid}-key`}
              type="password"
              autoComplete="off"
              spellCheck={false}
              data-primary
              value={key}
              disabled={offline || busy === "save"}
              aria-invalid={error ? true : undefined}
              aria-describedby={error ? `${uid}-error` : undefined}
              placeholder="sk-or-…"
              onChange={(e) => {
                setKey(e.target.value);
                if (error) setError("");
              }}
            />
          </div>
          {error && (
            <p className="health-bad" id={`${uid}-error`} role="alert">
              {error}
            </p>
          )}
          <div className="row">
            <button
              type="submit"
              className="primary"
              disabled={offline || !key.trim() || Boolean(busy)}
            >
              {busy === "save" ? "Checking key…" : "Save"}
            </button>
          </div>
        </form>
      )}
      {status?.key_set && (
        <>
          <p className="warn-text">Billed per token by OpenRouter</p>
          {info && (
            <>
              <p>Key: {info.label}</p>
              <p>
                Credits used: {formatUsd(info.usage)}
                {info.limit != null &&
                  ` of ${formatUsd(info.limit)} limit${
                    info.limit_remaining != null
                      ? ` (${formatUsd(info.limit_remaining)} left)`
                      : ""
                  }`}
              </p>
              {info.is_free_tier && (
                <p className="hint">
                  Free tier: add credits on OpenRouter to use paid models.
                </p>
              )}
            </>
          )}
          {status.key_error && <p className="health-bad">{status.key_error}</p>}
          <p>
            {status.models} model{status.models === 1 ? "" : "s"} (
            {status.tool_models} support tools)
            {status.fetched_at ? (
              <span className="dim">
                {" "}
                · updated {relativeTime(status.fetched_at)}
              </span>
            ) : null}
          </p>
          {status.activity_url && (
            <a href={status.activity_url} target="_blank" rel="noreferrer">
              Open OpenRouter activity
            </a>
          )}
          <div className="row">
            <button
              type="button"
              className="ghost"
              data-primary
              disabled={offline || Boolean(busy)}
              onClick={() => void refresh()}
            >
              {busy === "refresh" ? "Refreshing…" : "Refresh models"}
            </button>
            <button
              type="button"
              className="ghost danger-text"
              disabled={offline || Boolean(busy)}
              onClick={() => setConfirm(true)}
            >
              Remove key
            </button>
          </div>
        </>
      )}
      {confirm && (
        <Dialog
          label="Remove OpenRouter key"
          className="modal modal-sm"
          onClose={() => setConfirm(false)}
        >
          <h2>Remove OpenRouter key?</h2>
          <p>
            ShadowCode deletes the key from this computer and OpenRouter models
            stop working until you add a key again. The key stays valid on
            OpenRouter; revoke it there if you no longer need it.
          </p>
          <div className="row end">
            <button
              type="button"
              className="ghost"
              onClick={() => setConfirm(false)}
            >
              Cancel
            </button>
            <button
              type="button"
              className="primary danger"
              onClick={() => void remove()}
            >
              Remove key
            </button>
          </div>
        </Dialog>
      )}
    </article>
  );
}

const URL_RE = /(https?:\/\/[^\s<>"')]+)/g;
const CODE_RE = /\b([A-Z0-9]{4,5}-[A-Z0-9]{4,5})\b/g;

/** A login output line with links and selectable device codes. */
export function LoginLine({ line }: { line: string }) {
  const parts = line.split(URL_RE);
  return (
    <>
      {parts.map((part, i) =>
        i % 2 === 1 ? (
          <a key={i} href={part} target="_blank" rel="noreferrer">
            {part}
          </a>
        ) : (
          part.split(CODE_RE).map((piece, j) =>
            j % 2 === 1 ? (
              <code key={`${i}-${j}`} className="device-code">
                {piece}
              </code>
            ) : (
              <span key={`${i}-${j}`}>{piece}</span>
            ),
          )
        ),
      )}
    </>
  );
}

function LoginPanel({
  login,
  onCancel,
}: {
  login: Login;
  onCancel: () => void;
}) {
  return (
    <div className="login-panel" role="status" aria-live="polite">
      {login.state === "running" && (
        <p>
          Finish signing in on the page the vendor opened. Links and codes
          appear here as the command prints them.
        </p>
      )}
      {login.state === "done" && <p className="health-ok">Signed in.</p>}
      {login.state === "failed" && (
        <p className="health-bad">
          {login.detail || "Sign-in did not finish."}
        </p>
      )}
      {login.state === "unsupported" && <p>{login.detail}</p>}
      {login.lines.length > 0 && (
        <pre className="login-lines">
          {login.lines.map((line, i) => (
            <div key={i}>
              <LoginLine line={line} />
            </div>
          ))}
        </pre>
      )}
      {login.state === "running" && (
        <button type="button" className="mini ghost" onClick={onCancel}>
          Cancel sign-in
        </button>
      )}
    </div>
  );
}
