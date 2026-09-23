import { useCallback, useEffect, useState } from "react";
import {
  api,
  type Health,
  type HookCatalog,
  type NativeMcpCatalog,
  type NativePluginCatalog,
} from "../../api";
import { AdvancedTools } from "../AdvancedTools";
import { McpSettings } from "../McpSettings";
import { PluginSettings } from "../PluginSettings";
import { WorktreeSettings } from "../WorktreeSettings";
import {
  BackgroundTab,
  GoalsTab,
  HealthTab,
  SkillsTab,
} from "./AdvancedPanels";

export type AdvancedTab =
  | "skills"
  | "goals"
  | "background"
  | "health"
  | "mcp"
  | "plugins"
  | "hooks"
  | "worktrees"
  | "guardian"
  | "vendors";

export const ADVANCED_TABS: { id: AdvancedTab; label: string }[] = [
  { id: "skills", label: "Skills" },
  { id: "goals", label: "Goals" },
  { id: "background", label: "Background" },
  { id: "health", label: "Health" },
  { id: "mcp", label: "MCP" },
  { id: "plugins", label: "Plugins" },
  { id: "hooks", label: "Hooks" },
  { id: "worktrees", label: "Worktrees" },
  { id: "guardian", label: "Guardian" },
  { id: "vendors", label: "Vendor tools" },
];

type Toast = (text: string, kind: "ok" | "err" | "info") => void;

/** Settings › Advanced: project tools and integrations that stay out of the
 * default workflow. */
export function AdvancedPage({
  cfg,
  tab,
  onTab,
  health,
  sessionId,
  busy,
  onSave,
  onToast,
  onOpenProject,
  onOpenSession,
  onSkillsChanged,
  onUseSkill,
}: {
  cfg: Record<string, unknown>;
  tab: AdvancedTab;
  onTab: (tab: AdvancedTab) => void;
  health: Health | null;
  sessionId: string;
  busy: boolean;
  onSave: (values: Record<string, unknown>) => Promise<void>;
  onToast: Toast;
  onOpenProject?: (path: string) => void;
  onOpenSession: (id: string) => void;
  onSkillsChanged: () => Promise<void>;
  onUseSkill: (name: string) => void;
}) {
  const toast = (text: string, kind: "ok" | "err" | "info" = "info") =>
    onToast(text, kind);
  return (
    <section className="settings-page">
      <h3>Advanced</h3>
      <div
        className="advanced-tabs"
        role="tablist"
        aria-label="Advanced sections"
      >
        {ADVANCED_TABS.map((t) => (
          <button
            type="button"
            role="tab"
            id={`advanced-tab-${t.id}`}
            aria-selected={tab === t.id}
            aria-controls="advanced-panel"
            key={t.id}
            className={tab === t.id ? "on" : ""}
            onClick={() => onTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div
        className="advanced-panel"
        id="advanced-panel"
        role="tabpanel"
        aria-labelledby={`advanced-tab-${tab}`}
      >
        {tab === "skills" && (
          <SkillsTab
            toast={toast}
            onChanged={onSkillsChanged}
            onUse={onUseSkill}
            busy={busy}
          />
        )}
        {tab === "goals" && (
          <GoalsTab
            sessionId={sessionId}
            onOpen={onOpenSession}
            toast={toast}
          />
        )}
        {tab === "background" && <BackgroundTab toast={toast} />}
        {tab === "health" && <HealthTab health={health} />}
        {tab === "mcp" && <McpPanel onToast={onToast} />}
        {tab === "plugins" && (
          <PluginsPanel onToast={onToast} onNavigate={(next) => onTab(next)} />
        )}
        {tab === "hooks" && <HooksPanel onToast={onToast} />}
        {tab === "worktrees" && (
          <WorktreeSettings onOpen={onOpenProject} onToast={onToast} />
        )}
        {tab === "guardian" && (
          <GuardianPanel
            cfg={cfg}
            onSave={onSave}
            onOpenProject={onOpenProject}
          />
        )}
        {tab === "vendors" && <VendorToolsPanel cfg={cfg} onSave={onSave} />}
      </div>
    </section>
  );
}

function McpPanel({ onToast }: { onToast: Toast }) {
  const [catalog, setCatalog] = useState<NativeMcpCatalog | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    void api
      .mcpServers()
      .then(setCatalog)
      .catch((e) => setError(String(e)));
  }, []);
  if (error) return <p role="alert">{error}</p>;
  if (!catalog) return <p role="status">Loading MCP definitions…</p>;
  return (
    <McpSettings catalog={catalog} onChange={setCatalog} onToast={onToast} />
  );
}

function PluginsPanel({
  onToast,
  onNavigate,
}: {
  onToast: Toast;
  onNavigate: (section: "hooks" | "mcp") => void;
}) {
  const [catalog, setCatalog] = useState<NativePluginCatalog | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    void api
      .plugins()
      .then(setCatalog)
      .catch((e) => setError(String(e)));
  }, []);
  if (error) return <p role="alert">{error}</p>;
  if (!catalog) return <p role="status">Loading project plugins…</p>;
  return (
    <PluginSettings
      catalog={catalog}
      onChange={setCatalog}
      onToast={onToast}
      onNavigate={onNavigate}
    />
  );
}

function HooksPanel({ onToast }: { onToast: Toast }) {
  const [catalog, setCatalog] = useState<HookCatalog>({ hooks: [], dirs: [] });
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const refresh = useCallback(async () => {
    setBusy(true);
    setError("");
    try {
      setCatalog(await api.hooks());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  async function activate(path: string, hash: string, enabled: boolean) {
    setBusy(true);
    setError("");
    try {
      setCatalog(
        await api.activateHook(catalog.workspace || "", path, hash, enabled),
      );
      onToast(
        enabled
          ? "Hook enabled for new tasks in this project"
          : "Hook disabled for new tasks",
        "ok",
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <p className="hint">
        Review a command before enabling it for new tasks in this project. Hooks
        run as your user with a timeout and can change files. Definitions live
        in <code>.shadowcode/hooks/*.yaml</code>.
      </p>
      <button
        type="button"
        className="ghost hook-refresh"
        disabled={busy}
        onClick={() => void refresh()}
      >
        Refresh hooks
      </button>
      {error && (
        <p className="hint error" role="alert">
          {error}
        </p>
      )}
      {catalog.format === "command-v1" && !catalog.trusted && (
        <p className="hint">Trust this project before enabling a command.</p>
      )}
      <div className="list">
        {catalog.hooks.map((h) => (
          <div className="item static hook-entry" key={h.path || h.name}>
            <strong>
              {h.name}
              {h.builtin ? <span className="dim"> · built-in</span> : null}
            </strong>
            <span>{h.events.join(", ")}</span>
            {h.command !== undefined && (
              <>
                <span className="dim">
                  {h.description || h.path} · {h.timeout_sec}s
                  {h.path_suffix ? ` · paths ending in ${h.path_suffix}` : ""}
                </span>
                <pre className="hook-command">{h.command}</pre>
                <button
                  type="button"
                  className="ghost"
                  disabled={busy || (!h.enabled && !catalog.trusted)}
                  onClick={() => void activate(h.path!, h.hash!, !h.enabled)}
                >
                  {h.enabled ? `Disable ${h.name}` : `Enable ${h.name}`}
                </button>
              </>
            )}
          </div>
        ))}
        {catalog.hooks.length === 0 && !busy && (
          <p className="hint">No hook definitions found.</p>
        )}
        {catalog.issues?.map((issue) => (
          <p className="hint" key={issue}>
            {issue}
          </p>
        ))}
        {catalog.approved
          ?.filter(
            (a) => !catalog.hooks.some((h) => h.path === a.path && h.enabled),
          )
          .map((a) => (
            <div className="item static hook-entry" key={a.path}>
              <strong>Review required: {a.path}</strong>
              <span>
                The enabled definition changed or is unavailable. Restore it,
                review and enable its current contents, or disable it before
                starting another task.
              </span>
              <button
                type="button"
                className="ghost"
                disabled={busy}
                aria-label={`Disable unavailable hook ${a.path}`}
                onClick={() => void activate(a.path, a.hash, false)}
              >
                Disable unavailable hook
              </button>
            </div>
          ))}
      </div>
    </>
  );
}

function GuardianPanel({
  cfg,
  onSave,
  onOpenProject,
}: {
  cfg: Record<string, unknown>;
  onSave: (values: Record<string, unknown>) => Promise<void>;
  onOpenProject?: (path: string) => void;
}) {
  const guardian = (cfg.guardian || {}) as Record<string, unknown>;
  const [enabled, setEnabled] = useState(Boolean(guardian.enabled));
  const [intervalSec, setIntervalSec] = useState(
    Number(guardian.interval_sec ?? 3600),
  );
  const [saving, setSaving] = useState(false);
  return (
    <>
      <AdvancedTools onOpen={onOpenProject} />
      <section className="settings-section advanced-card guardian-schedule">
        <h4>Guardian schedule</h4>
        <label className="check">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
          />{" "}
          Enable scheduled diagnostics
        </label>
        <div className="field">
          <label htmlFor="guardian-interval">Check every (seconds)</label>
          <input
            id="guardian-interval"
            type="number"
            min={60}
            max={86400}
            value={intervalSec}
            onChange={(e) => setIntervalSec(Number(e.target.value))}
          />
        </div>
        <p className="hint">
          Guardian is off by default and does not generate patches or run tests.
          Shell isolation uses bubblewrap when available.
        </p>
        <div className="row end">
          <button
            type="button"
            className="primary"
            disabled={saving}
            onClick={() => {
              setSaving(true);
              void onSave({
                guardian: {
                  enabled,
                  interval_sec: intervalSec,
                  allow_prepare_patch: Boolean(guardian.allow_prepare_patch),
                },
              }).finally(() => setSaving(false));
            }}
          >
            {saving ? "Saving…" : "Save schedule"}
          </button>
        </div>
      </section>
    </>
  );
}

const BINARIES: [string, string, string][] = [
  ["codex_binary", "Codex", "codex"],
  ["claude_binary", "Claude Code", "claude"],
  ["cursor_binary", "Cursor", "cursor-agent"],
  ["antigravity_binary", "Antigravity", "agy"],
  ["grok_binary", "Grok", "grok"],
];

function VendorToolsPanel({
  cfg,
  onSave,
}: {
  cfg: Record<string, unknown>;
  onSave: (values: Record<string, unknown>) => Promise<void>;
}) {
  const cli = (cfg.cli_agents || {}) as Record<string, unknown>;
  const [enabled, setEnabled] = useState(cli.enabled !== false);
  const [binaries, setBinaries] = useState<Record<string, string>>(() =>
    Object.fromEntries(
      BINARIES.map(([key, , fallback]) => [key, String(cli[key] || fallback)]),
    ),
  );
  const [approval, setApproval] = useState(
    Number(cli.approval_timeout_sec ?? 600),
  );
  const [stall, setStall] = useState(Number(cli.stall_timeout_sec ?? 900));
  const [saving, setSaving] = useState(false);
  return (
    <section className="settings-section advanced-card">
      <p className="hint">
        Subscriptions run through the official command-line tools of Codex,
        Claude Code, Cursor, Antigravity and Grok. Sign-in stays with those
        tools; ShadowCode never reads or stores their credentials.
      </p>
      <label className="check">
        <input
          type="checkbox"
          checked={enabled}
          onChange={(e) => setEnabled(e.target.checked)}
        />{" "}
        Offer subscriptions in the model picker
      </label>
      {BINARIES.map(([key, label]) => (
        <div className="field" key={key}>
          <label htmlFor={`cli-${key}`}>{label} command</label>
          <input
            id={`cli-${key}`}
            value={binaries[key]}
            onChange={(e) =>
              setBinaries((b) => ({ ...b, [key]: e.target.value }))
            }
          />
        </div>
      ))}
      <div className="field">
        <label htmlFor="cli-approval">Approval timeout (seconds)</label>
        <input
          id="cli-approval"
          type="number"
          min={10}
          max={86400}
          value={approval}
          onChange={(e) => setApproval(Number(e.target.value))}
        />
      </div>
      <div className="field">
        <label htmlFor="cli-stall">Stall timeout (seconds)</label>
        <input
          id="cli-stall"
          type="number"
          min={30}
          max={86400}
          value={stall}
          onChange={(e) => setStall(Number(e.target.value))}
        />
      </div>
      <div className="row end">
        <button
          type="button"
          className="primary"
          disabled={saving}
          onClick={() => {
            setSaving(true);
            void onSave({
              cli_agents: {
                enabled,
                ...binaries,
                approval_timeout_sec: approval,
                stall_timeout_sec: stall,
              },
            }).finally(() => setSaving(false));
          }}
        >
          {saving ? "Saving…" : "Save"}
        </button>
      </div>
    </section>
  );
}
