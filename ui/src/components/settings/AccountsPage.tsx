import { useCallback, useEffect, useRef, useState } from "react";
import { RefreshCw } from "lucide-react";
import { api, type LoginProgress, type VendorStatus } from "../../api";
import { Dialog } from "../Dialog";
import { listen } from "../../lib/transport";
import { usageDetailLines, usageLabel } from "../../lib/picker";

const ORDER = ["codex", "claude", "cursor", "antigravity", "grok"];

export const vendorId = (key: string) => key.replace(/^cli[-:]/, "");

type Login = {
  vendor: string;
  state: "running" | "done" | "failed" | "unsupported";
  lines: string[];
  detail?: string;
};

/** Settings › Accounts: the official CLI of each subscription, its sign-in
 * state, models and usage. Sign-in and sign-out run the vendors' own commands. */
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
            password and never reads vendor tokens.
          </p>
        </div>
        <button
          type="button"
          className="ghost"
          disabled={loading}
          onClick={() => void load(true).then(onChanged)}
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
