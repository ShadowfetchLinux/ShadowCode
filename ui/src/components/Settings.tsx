import { AdvancedTools } from "./AdvancedTools";
import { Dialog } from "./Dialog";
import { WorktreeSettings } from "./WorktreeSettings";
import { PluginSettings } from "./PluginSettings";
import { McpSettings } from "./McpSettings";
import { isNative } from "../lib/transport";
import { useEffect, useState } from "react";
import {
  api,
  type DetectedProvider,
  type McpServer,
  type NativeMcpCatalog,
  type ProviderInfo,
  type HookCatalog,
  type LegacyPluginCatalog,
  type NativePluginCatalog,
} from "../api";

type Section =
  | "model"
  | "permissions"
  | "appearance"
  | "hooks"
  | "mcp"
  | "plugins"
  | "worktrees"
  | "advanced";
const SECTIONS: { id: Section; label: string }[] = [
  { id: "model", label: "Model" },
  { id: "permissions", label: "Permissions" },
  { id: "appearance", label: "Appearance" },
  { id: "hooks", label: "Hooks" },
  { id: "mcp", label: "MCP" },
  { id: "plugins", label: "Plugins" },
  { id: "worktrees", label: "Worktrees" },
  { id: "advanced", label: "Advanced" },
];

export function Settings({
  cfg,
  onClose,
  onSave,
  onToast,
  initialSection = "model",
  onOpenProject,
}: {
  cfg: Record<string, unknown>;
  onClose: () => void;
  onSave: (
    values: Record<string, unknown>,
    apiKey: string,
    apiKeyEnv: string,
  ) => Promise<void>;
  onToast: (text: string, kind: "ok" | "err" | "info") => void;
  initialSection?: Section;
  onOpenProject?: (path: string) => void;
}) {
  const model = (cfg.model || {}) as Record<string, string>;
  const permissions = (cfg.permissions || {}) as Record<
    string,
    string | boolean
  >;
  const ui = (cfg.ui || {}) as Record<string, string | number | boolean>;
  const [section, setSection] = useState<Section>(initialSection);

  // Model
  const [provider, setProvider] = useState(model.provider || "mock");
  const [name, setName] = useState(model.name || "");
  const [endpoint, setEndpoint] = useState(model.endpoint || "");
  const [keyEnv, setKeyEnv] = useState(model.api_key_env || "OPENAI_API_KEY");
  const [contextLimit, setContextLimit] = useState(
    Number(model.context_limit) || 16384,
  );
  const [keepAlive, setKeepAlive] = useState(model.keep_alive || "30m");
  const [apiKey, setApiKey] = useState("");
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [detected, setDetected] = useState<DetectedProvider[]>([]);
  const [testState, setTestState] = useState<{
    busy: boolean;
    text: string;
    ok?: boolean;
  }>({ busy: false, text: "" });
  // Permissions
  const [level, setLevel] = useState(String(permissions.level || "workspace"));
  const [network, setNetwork] = useState(Boolean(permissions.network));
  const [approveDangerous, setApproveDangerous] = useState(
    permissions.require_approval_for_dangerous !== false,
  );
  // Appearance
  const [theme, setTheme] = useState(String(ui.theme || "light"));
  const [ability, setAbility] = useState(String(ui.ability || "none"));
  const guardianCfg = (cfg.guardian || {}) as Record<
    string,
    string | number | boolean
  >;
  const [guardianEnabled, setGuardianEnabled] = useState(
    Boolean(guardianCfg.enabled),
  );
  const [guardianInterval, setGuardianInterval] = useState(
    Number(guardianCfg.interval_sec ?? 3600),
  );
  const cliCfg = (cfg.cli_agents || {}) as Record<
    string,
    string | number | boolean
  >;
  const [cliEnabled, setCliEnabled] = useState(cliCfg.enabled !== false);
  const [cliClaude, setCliClaude] = useState(cliCfg.claude_enabled !== false);
  const [cliCodexBin, setCliCodexBin] = useState(
    String(cliCfg.codex_binary || "codex"),
  );
  const [cliGrokBin, setCliGrokBin] = useState(
    String(cliCfg.grok_binary || "grok"),
  );
  const [cliClaudeBin, setCliClaudeBin] = useState(
    String(cliCfg.claude_binary || "claude"),
  );
  const [cliApprovalSec, setCliApprovalSec] = useState(
    Number(cliCfg.approval_timeout_sec ?? 600),
  );
  const [cliStallSec, setCliStallSec] = useState(
    Number(cliCfg.stall_timeout_sec ?? 900),
  );
  const [notify, setNotify] = useState(ui.notify !== false);
  const [notifyAfter, setNotifyAfter] = useState(
    Number(ui.notify_after_sec ?? 4),
  );
  // Hooks / MCP / plugins
  const [hookCatalog, setHookCatalog] = useState<HookCatalog>({
    hooks: [],
    dirs: [],
  });
  const [hookError, setHookError] = useState("");
  const [hookBusy, setHookBusy] = useState(false);
  const [servers, setServers] = useState<McpServer[]>([]);
  const [nativeMcp, setNativeMcp] = useState<NativeMcpCatalog | null>(null);
  const [mcpError, setMcpError] = useState("");
  const [newServer, setNewServer] = useState({ name: "", target: "" });
  const [plugins, setPlugins] = useState<LegacyPluginCatalog>({
    installed: [],
    available: [],
  });
  const [nativePlugins, setNativePlugins] =
    useState<NativePluginCatalog | null>(null);
  const [pluginError, setPluginError] = useState("");
  function updatePlugins(value: LegacyPluginCatalog | NativePluginCatalog) {
    if (value.format === "native-plugins-v1") setNativePlugins(value);
    else setPlugins(value);
  }

  useEffect(() => {
    void api
      .providers()
      .then((d) => setProviders(d.providers))
      .catch(() => setProviders([]));
    void api
      .detectProviders()
      .then((d) => setDetected(d.providers))
      .catch(() => setDetected([]));
    void api
      .hooks()
      .then(setHookCatalog)
      .catch((error) => setHookError(String(error)));
    void api
      .mcpServers()
      .then((d) =>
        d.format === "native-mcp-v1" ? setNativeMcp(d) : setServers(d.servers),
      )
      .catch((error) => setMcpError(String(error)));
    void api
      .plugins()
      .then(updatePlugins)
      .catch((error) => setPluginError(String(error)));
  }, []);

  async function activateHook(path: string, hash: string, enabled: boolean) {
    setHookBusy(true);
    setHookError("");
    try {
      setHookCatalog(
        await api.activateHook(
          hookCatalog.workspace || "",
          path,
          hash,
          enabled,
        ),
      );
      onToast(
        enabled
          ? "Hook enabled for new tasks in this project"
          : "Hook disabled for new tasks",
        "ok",
      );
    } catch (error) {
      setHookError(String(error));
    } finally {
      setHookBusy(false);
    }
  }

  async function refreshHooks() {
    setHookBusy(true);
    setHookError("");
    try {
      setHookCatalog(await api.hooks());
    } catch (error) {
      setHookError(String(error));
    } finally {
      setHookBusy(false);
    }
  }

  const preset = providers.find((p) => p.id === provider);
  const detectedFor = detected.find(
    (d) => d.provider === provider && d.running,
  );

  function pickProvider(next: string) {
    setProvider(next);
    const p = providers.find((x) => x.id === next);
    if (p) {
      setEndpoint(p.endpoint);
      setKeyEnv(p.api_key_env || "OPENAI_API_KEY");
    }
    const hit = detected.find(
      (d) => d.provider === next && d.running && d.models.length,
    );
    if (hit) setName(hit.models[0].id);
  }

  async function testConnection() {
    setTestState({ busy: true, text: "" });
    try {
      if (apiKey.trim()) await api.saveConfig({}, apiKey.trim(), keyEnv);
      const result = await api.testModel({
        provider,
        name,
        endpoint,
        api_key_env: keyEnv,
      });
      if (result.ok)
        setTestState({
          busy: false,
          ok: true,
          text: `OK in ${result.latency_ms}ms — “${(result.reply || "").slice(0, 60)}”`,
        });
      else
        setTestState({
          busy: false,
          ok: false,
          text: result.error || "test failed",
        });
    } catch (err) {
      setTestState({ busy: false, ok: false, text: String(err) });
    }
  }

  async function save() {
    if (testState.text && testState.ok === false)
      onToast("Saving anyway — last test failed", "info");
    try {
      await onSave(
        {
          model: {
            default: name || provider,
            provider,
            endpoint,
            name,
            api_key_env: keyEnv,
            context_limit: contextLimit,
            keep_alive: keepAlive,
          },
          permissions: {
            level,
            network,
            require_approval_for_dangerous: approveDangerous,
          },
          ui: { theme, ability, notify, notify_after_sec: notifyAfter },
          guardian: {
            enabled: guardianEnabled,
            interval_sec: guardianInterval,
            allow_prepare_patch: Boolean(guardianCfg.allow_prepare_patch),
          },
          cli_agents: {
            enabled: cliEnabled,
            claude_enabled: cliClaude,
            codex_binary: cliCodexBin,
            grok_binary: cliGrokBin,
            claude_binary: cliClaudeBin,
            approval_timeout_sec: cliApprovalSec,
            stall_timeout_sec: cliStallSec,
          },
        },
        apiKey,
        keyEnv,
      );
    } catch (err) {
      onToast(String(err), "err");
    }
  }

  async function addServer() {
    if (!newServer.name.trim() || !newServer.target.trim()) return;
    const target = newServer.target.trim();
    const entry: McpServer = target.startsWith("http")
      ? { name: newServer.name.trim(), url: target, command: null }
      : {
          name: newServer.name.trim(),
          command: target.split(/\s+/),
          url: null,
        };
    const next = [...servers.filter((s) => s.name !== entry.name), entry];
    try {
      const saved = await api.saveMcpServers(next);
      setServers(saved.servers);
      setNewServer({ name: "", target: "" });
      onToast(`MCP server ${entry.name} saved`, "ok");
    } catch (err) {
      onToast(String(err), "err");
    }
  }

  async function removeServer(nameToRemove: string) {
    try {
      const saved = await api.saveMcpServers(
        servers.filter((s) => s.name !== nameToRemove),
      );
      setServers(saved.servers);
    } catch (err) {
      onToast(String(err), "err");
    }
  }

  return (
    <Dialog label="Settings" className="modal settings" onClose={onClose}>
      <nav className="settings-nav">
        <h2>Settings</h2>
        {SECTIONS.filter((s) => s.id !== "worktrees" || isNative()).map((s) => (
          <button
            type="button"
            key={s.id}
            className={section === s.id ? "on" : ""}
            onClick={() => setSection(s.id)}
          >
            {s.label}
          </button>
        ))}
      </nav>
      <div className="settings-body">
        {section === "model" && (
          <section>
            <h3>Model</h3>
            <p className="hint">
              Pick a provider, then a model id. Local servers are detected
              automatically. Keys are stored in <code>secrets.env</code> (mode
              600), never in YAML.
            </p>
            <div className="field">
              <label htmlFor="settings-field-1">Provider</label>
              <select
                id="settings-field-1"
                value={provider}
                onChange={(e) => pickProvider(e.target.value)}
              >
                {providers.length === 0 && (
                  <option value={provider}>{provider}</option>
                )}
                {providers.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.label}
                    {p.running ? " — running" : ""}
                  </option>
                ))}
              </select>
            </div>
            {detectedFor && detectedFor.models.length > 0 && (
              <div className="field">
                <span className="hint">Installed models</span>
                <div className="chip-row">
                  {detectedFor.models.map((m) => (
                    <button
                      type="button"
                      key={m.id}
                      className={`chip ${m.id === name ? "on" : ""}`}
                      onClick={() => setName(m.id)}
                    >
                      {m.id}
                      {m.detail ? (
                        <span className="dim"> {m.detail}</span>
                      ) : null}
                    </button>
                  ))}
                </div>
              </div>
            )}
            <div className="field">
              <label htmlFor="settings-field-2">Model id</label>
              <input
                id="settings-field-2"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="any id your provider accepts"
              />
            </div>
            <div className="field">
              <label htmlFor="settings-field-3">Endpoint</label>
              <input
                id="settings-field-3"
                value={endpoint}
                onChange={(e) => setEndpoint(e.target.value)}
                placeholder={preset?.endpoint || "https://host/v1"}
              />
            </div>
            <div className="field">
              <label htmlFor="model-context">Context window (tokens)</label>
              <input
                id="model-context"
                type="number"
                min={1024}
                max={4000000}
                step={1024}
                value={contextLimit}
                onChange={(e) => setContextLimit(Number(e.target.value))}
              />
              <p className="hint">
                Match your model and available memory. Larger windows use more
                memory; 16,384 is a practical starting point for local models.
              </p>
            </div>
            {provider === "ollama" && (
              <div className="field">
                <label htmlFor="model-residency">Keep model loaded</label>
                <select
                  id="model-residency"
                  value={keepAlive}
                  onChange={(e) => setKeepAlive(e.target.value)}
                >
                  {!["0", "5m", "30m", "1h", "-1"].includes(keepAlive) && (
                    <option value={keepAlive}>{keepAlive}</option>
                  )}
                  <option value="0">Unload after each reply</option>
                  <option value="5m">5 minutes</option>
                  <option value="30m">30 minutes</option>
                  <option value="1h">1 hour</option>
                  <option value="-1">Until Ollama stops</option>
                </select>
                <p className="hint">
                  Keeping the model loaded reduces startup delays between turns
                  and reserves its memory.
                </p>
              </div>
            )}
            {(preset?.needs_key || provider === "openai_compatible") && (
              <>
                <div className="field">
                  <label htmlFor="settings-field-4">API key env var</label>
                  <input
                    id="settings-field-4"
                    value={keyEnv}
                    onChange={(e) => setKeyEnv(e.target.value)}
                  />
                </div>
                <div className="field">
                  <label htmlFor="settings-field-5">API key</label>
                  <input
                    id="settings-field-5"
                    type="password"
                    value={apiKey}
                    onChange={(e) => setApiKey(e.target.value)}
                    placeholder="leave blank to keep the current secret"
                  />
                </div>
              </>
            )}
            <div className="row">
              <button
                type="button"
                className="ghost"
                disabled={testState.busy}
                onClick={() => void testConnection()}
              >
                {testState.busy ? "Testing…" : "Test connection"}
              </button>
              {testState.text && (
                <span className={testState.ok ? "health-ok" : "health-bad"}>
                  {testState.text}
                </span>
              )}
            </div>
          </section>
        )}

        {section === "permissions" && (
          <section>
            <h3>Permissions</h3>
            <div className="field">
              <label htmlFor="settings-field-6">Level</label>
              <select
                id="settings-field-6"
                value={level}
                onChange={(e) => setLevel(e.target.value)}
              >
                <option value="read_only">
                  Read only — inspect, never write
                </option>
                <option value="workspace">
                  Workspace — edit and run inside the project (recommended)
                </option>
                <option value="elevated">
                  Elevated — dangerous commands after you approve
                </option>
              </select>
            </div>
            <label className="check">
              <input
                type="checkbox"
                checked={approveDangerous}
                onChange={(e) => setApproveDangerous(e.target.checked)}
              />{" "}
              Ask before dangerous commands (rm -rf, git push --force, sudo…)
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={network}
                onChange={(e) => setNetwork(e.target.checked)}
              />{" "}
              Allow network access from tools
            </label>
          </section>
        )}

        {section === "appearance" && (
          <section>
            <h3>Appearance</h3>
            <div className="field">
              <span className="hint">Theme</span>
              <div className="seg">
                {["light", "dark"].map((t) => (
                  <button
                    type="button"
                    key={t}
                    className={theme === t ? "on" : ""}
                    onClick={() => setTheme(t)}
                  >
                    {t}
                  </button>
                ))}
              </div>
            </div>
            <label className="check">
              <input
                type="checkbox"
                checked={notify}
                onChange={(e) => {
                  setNotify(e.target.checked);
                  if (
                    e.target.checked &&
                    typeof Notification !== "undefined" &&
                    Notification.permission === "default"
                  )
                    void Notification.requestPermission();
                }}
              />{" "}
              Desktop notification when a task finishes
            </label>
            <div className="field inline">
              <label>…only if it ran longer than</label>
              <input
                id="settings-field-7"
                aria-label="Notify after seconds"
                type="number"
                min={0}
                step={1}
                value={notifyAfter}
                onChange={(e) => setNotifyAfter(Number(e.target.value))}
              />{" "}
              <span className="dim">seconds</span>
            </div>
            <div className="field">
              <label htmlFor="settings-field-8">Abilities</label>
              <select
                id="settings-field-8"
                value={ability}
                onChange={(e) => setAbility(e.target.value)}
              >
                <option value="none">none</option>
                <option value="computer_use">Computer Use</option>
                <option value="custom">Custom</option>
              </select>
              <p className="hint">
                Abilities live here so the composer stays minimal.
              </p>
            </div>
          </section>
        )}

        {section === "hooks" && (
          <section>
            <h3>Hooks</h3>
            <button
              type="button"
              className="ghost hook-refresh"
              disabled={hookBusy}
              onClick={() => void refreshHooks()}
            >
              Refresh hooks
            </button>
            {hookCatalog.format === "command-v1" ? (
              <p className="hint">
                Review a command before enabling it for new tasks in this
                project. Hooks run as your user with a timeout and can change
                files. Plan and Review modes keep them inactive. Definitions
                live in <code>.shadowcode/hooks/*.yaml</code>.
              </p>
            ) : (
              <p className="hint">
                Deterministic lifecycle hooks fire at fixed points in the loop.
                Project hooks go in{" "}
                <code>{hookCatalog.dirs[0] || ".shadowcode/hooks/"}</code> as{" "}
                <code>*.py</code> files exporting{" "}
                <code>register(registry)</code>.
              </p>
            )}
            {hookError && (
              <p className="hint error" role="alert">
                {hookError}
              </p>
            )}
            {hookCatalog.format === "command-v1" && !hookCatalog.trusted && (
              <p className="hint">
                Trust this project before enabling a command.
              </p>
            )}
            <div className="list">
              {hookCatalog.hooks.map((h) => (
                <div className="item static hook-entry" key={h.path || h.name}>
                  <strong>
                    {h.name}
                    {h.builtin ? (
                      <span className="dim"> · built-in</span>
                    ) : null}
                  </strong>
                  <span>{h.events.join(", ")}</span>
                  {h.command !== undefined && (
                    <>
                      <span className="dim">
                        {h.description || h.path} · {h.timeout_sec}s
                        {h.path_suffix
                          ? ` · paths ending in ${h.path_suffix}`
                          : ""}
                      </span>
                      <pre className="hook-command">{h.command}</pre>
                      <button
                        type="button"
                        className="ghost"
                        disabled={
                          hookBusy || (!h.enabled && !hookCatalog.trusted)
                        }
                        onClick={() =>
                          void activateHook(h.path!, h.hash!, !h.enabled)
                        }
                      >
                        {h.enabled ? `Disable ${h.name}` : `Enable ${h.name}`}
                      </button>
                    </>
                  )}
                </div>
              ))}
              {hookCatalog.hooks.length === 0 && (
                <p className="hint">No hook definitions found.</p>
              )}
              {hookCatalog.issues?.map((issue) => (
                <p className="hint" key={issue}>
                  {issue}
                </p>
              ))}
              {hookCatalog.approved
                ?.filter(
                  (a) =>
                    !hookCatalog.hooks.some(
                      (h) => h.path === a.path && h.enabled,
                    ),
                )
                .map((a) => (
                  <div className="item static hook-entry" key={a.path}>
                    <strong>Review required: {a.path}</strong>
                    <span>
                      The enabled definition changed or is unavailable. Restore
                      it, review and enable its current contents, or disable it
                      before starting another Build task.
                    </span>
                    <button
                      type="button"
                      className="ghost"
                      disabled={hookBusy}
                      aria-label={`Disable unavailable hook ${a.path}`}
                      onClick={() => void activateHook(a.path, a.hash, false)}
                    >
                      Disable unavailable hook
                    </button>
                  </div>
                ))}
            </div>
          </section>
        )}

        {section === "mcp" && nativeMcp && (
          <McpSettings
            catalog={nativeMcp}
            onChange={setNativeMcp}
            onToast={onToast}
          />
        )}
        {section === "mcp" && mcpError && <p role="alert">{mcpError}</p>}
        {section === "mcp" && !nativeMcp && !mcpError && isNative() && (
          <p role="status">Loading MCP definitions…</p>
        )}
        {section === "mcp" && !nativeMcp && !mcpError && !isNative() && (
          <section>
            <h3>MCP servers</h3>
            <p className="hint">
              Tools from these servers are offered to the model. Use a URL for
              HTTP/SSE or a command line for stdio.
            </p>
            <div className="list">
              {servers.map((s) => (
                <div className="item static" key={s.name}>
                  <strong>{s.name}</strong>
                  <span>{s.url || (s.command || []).join(" ")}</span>
                  <button
                    type="button"
                    className="mini danger-text"
                    onClick={() => void removeServer(s.name)}
                  >
                    Remove
                  </button>
                </div>
              ))}
              {servers.length === 0 && (
                <p className="hint">No MCP servers configured.</p>
              )}
            </div>
            <div className="field inline">
              <input
                value={newServer.name}
                onChange={(e) =>
                  setNewServer({ ...newServer, name: e.target.value })
                }
                placeholder="name"
              />
              <input
                value={newServer.target}
                onChange={(e) =>
                  setNewServer({ ...newServer, target: e.target.value })
                }
                placeholder="http://127.0.0.1:7431/sse  or  npx -y some-mcp"
              />
              <button
                type="button"
                className="ghost"
                onClick={() => void addServer()}
              >
                Add
              </button>
            </div>
            <p className="hint">
              Expose ShadowCode itself: <code>shadow mcp serve</code> ·{" "}
              <code>shadow mcp register</code>.
            </p>
          </section>
        )}

        {section === "plugins" && nativePlugins && (
          <PluginSettings
            catalog={nativePlugins}
            onChange={(value) => {
              setNativePlugins(value);
              void api
                .hooks()
                .then(setHookCatalog)
                .catch((error) => setHookError(String(error)));
              void api
                .mcpServers()
                .then((d) => {
                  if (d.format === "native-mcp-v1") setNativeMcp(d);
                })
                .catch((error) => setMcpError(String(error)));
            }}
            onToast={onToast}
            onNavigate={(next) => {
              setSection(next);
              if (next === "hooks") void refreshHooks();
              else
                void api
                  .mcpServers()
                  .then((d) => {
                    if (d.format === "native-mcp-v1") setNativeMcp(d);
                  })
                  .catch((error) => setMcpError(String(error)));
            }}
          />
        )}
        {section === "plugins" && !nativePlugins && (
          <section>
            <h3>Plugins</h3>
            {pluginError && (
              <p role="alert" className="error">
                {pluginError}
              </p>
            )}
            <div className="list">
              {plugins.available.map((p) => {
                const meta = plugins.installed.find((m) => m.name === p.name);
                return (
                  <div className="item static" key={p.name}>
                    <strong>
                      {p.name}
                      {meta ? (
                        <span className="dim"> · {meta.version}</span>
                      ) : null}
                    </strong>
                    <span>{meta?.description || "Not installed"}</span>
                    {p.installed ? (
                      <button
                        type="button"
                        className="mini danger-text"
                        onClick={() =>
                          void api
                            .removePlugin(p.name)
                            .then(() => api.plugins().then(updatePlugins))
                            .catch((err) => onToast(String(err), "err"))
                        }
                      >
                        Remove
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="mini"
                        onClick={() =>
                          void api
                            .installPlugin(p.name)
                            .then(() => api.plugins().then(updatePlugins))
                            .catch((err) => onToast(String(err), "err"))
                        }
                      >
                        Install
                      </button>
                    )}
                  </div>
                );
              })}
              {plugins.available.length === 0 && !pluginError && (
                <p className="hint">
                  {isNative()
                    ? "Loading project plugins…"
                    : "No plugins in the registry."}
                </p>
              )}
            </div>
          </section>
        )}

        {section === "worktrees" && isNative() && (
          <WorktreeSettings onOpen={onOpenProject} onToast={onToast} />
        )}

        {section === "advanced" && (
          <div className="stack gap">
            <h3>Advanced</h3>
            {isNative() ? (
              <AdvancedTools onOpen={onOpenProject} />
            ) : (
              <p className="hint">
                Workspace preparation and Guardian diagnostics require the
                native desktop.
              </p>
            )}
            <section className="settings-section advanced-card guardian-schedule">
              <h3>Guardian schedule</h3>
              <label className="check">
                <input
                  type="checkbox"
                  checked={guardianEnabled}
                  onChange={(e) => setGuardianEnabled(e.target.checked)}
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
                  value={guardianInterval}
                  onChange={(e) => setGuardianInterval(Number(e.target.value))}
                />
              </div>
              <p className="hint">
                Scheduled checks run while the headless server is running.
                Guardian is off by default and does not generate patches or run
                tests.
              </p>
              <p className="hint">
                Shell isolation uses bubblewrap when available. Approved
                commands can write to the live project.
              </p>
            </section>
            <section className="settings-section advanced-card">
              <h3>Vendor CLI agents</h3>
              <p className="hint">
                Drive Claude, Codex, or Grok by spawning the official CLI. Login
                stays with <code>claude auth login</code>, <code>codex login</code>,
                or <code>grok login</code>. ShadowCode never reads or stores
                those credentials. Anthropic forbids third-party clients from
                using Pro/Max OAuth tokens directly (enforced 2026); driving the
                official <code>claude</code> binary with your own login is
                currently tolerated but not guaranteed.
              </p>
              <label className="check">
                <input
                  type="checkbox"
                  checked={cliEnabled}
                  onChange={(e) => setCliEnabled(e.target.checked)}
                />{" "}
                Enable vendor CLI backends
              </label>
              <label className="check">
                <input
                  type="checkbox"
                  checked={cliClaude}
                  onChange={(e) => setCliClaude(e.target.checked)}
                />{" "}
                Enable Claude adapter
              </label>
              <div className="field">
                <label htmlFor="cli-codex-bin">Codex binary</label>
                <input
                  id="cli-codex-bin"
                  value={cliCodexBin}
                  onChange={(e) => setCliCodexBin(e.target.value)}
                />
              </div>
              <div className="field">
                <label htmlFor="cli-grok-bin">Grok binary</label>
                <input
                  id="cli-grok-bin"
                  value={cliGrokBin}
                  onChange={(e) => setCliGrokBin(e.target.value)}
                />
              </div>
              <div className="field">
                <label htmlFor="cli-claude-bin">Claude binary</label>
                <input
                  id="cli-claude-bin"
                  value={cliClaudeBin}
                  onChange={(e) => setCliClaudeBin(e.target.value)}
                />
              </div>
              <div className="field">
                <label htmlFor="cli-approval">Approval timeout (seconds)</label>
                <input
                  id="cli-approval"
                  type="number"
                  min={10}
                  max={86400}
                  value={cliApprovalSec}
                  onChange={(e) => setCliApprovalSec(Number(e.target.value))}
                />
              </div>
              <div className="field">
                <label htmlFor="cli-stall">Stall timeout (seconds)</label>
                <input
                  id="cli-stall"
                  type="number"
                  min={30}
                  max={86400}
                  value={cliStallSec}
                  onChange={(e) => setCliStallSec(Number(e.target.value))}
                />
              </div>
            </section>
          </div>
        )}

        <div className="row end settings-foot">
          <button type="button" className="ghost" onClick={onClose}>
            Close
          </button>
          {(section === "model" ||
            section === "permissions" ||
            section === "appearance" ||
            section === "advanced") && (
            <button
              type="button"
              className="primary"
              onClick={() => void save()}
            >
              Save
            </button>
          )}
        </div>
      </div>
    </Dialog>
  );
}
