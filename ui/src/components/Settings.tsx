import { useEffect, useState } from "react";
import { api, type DetectedProvider, type McpServer, type ProviderInfo } from "../api";

type Section = "model" | "permissions" | "appearance" | "hooks" | "mcp" | "plugins";
const SECTIONS: { id: Section; label: string }[] = [
  { id: "model", label: "Model" },
  { id: "permissions", label: "Permissions" },
  { id: "appearance", label: "Appearance" },
  { id: "hooks", label: "Hooks" },
  { id: "mcp", label: "MCP" },
  { id: "plugins", label: "Plugins" },
];

export function Settings({
  cfg,
  onClose,
  onSave,
  onToast,
  initialSection = "model",
}: {
  cfg: Record<string, unknown>;
  onClose: () => void;
  onSave: (values: Record<string, unknown>, apiKey: string, apiKeyEnv: string) => Promise<void>;
  onToast: (text: string, kind: "ok" | "err" | "info") => void;
  initialSection?: Section;
}) {
  const model = (cfg.model || {}) as Record<string, string>;
  const permissions = (cfg.permissions || {}) as Record<string, string | boolean>;
  const ui = (cfg.ui || {}) as Record<string, string | number | boolean>;
  const [section, setSection] = useState<Section>(initialSection);

  // Model
  const [provider, setProvider] = useState(model.provider || "mock");
  const [name, setName] = useState(model.name || "");
  const [endpoint, setEndpoint] = useState(model.endpoint || "");
  const [keyEnv, setKeyEnv] = useState(model.api_key_env || "OPENAI_API_KEY");
  const [apiKey, setApiKey] = useState("");
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [detected, setDetected] = useState<DetectedProvider[]>([]);
  const [testState, setTestState] = useState<{ busy: boolean; text: string; ok?: boolean }>({ busy: false, text: "" });
  // Permissions
  const [level, setLevel] = useState(String(permissions.level || "workspace"));
  const [network, setNetwork] = useState(Boolean(permissions.network));
  const [approveDangerous, setApproveDangerous] = useState(permissions.require_approval_for_dangerous !== false);
  // Appearance
  const [theme, setTheme] = useState(String(ui.theme || "light"));
  const [ability, setAbility] = useState(String(ui.ability || "none"));
  const [notify, setNotify] = useState(ui.notify !== false);
  const [notifyAfter, setNotifyAfter] = useState(Number(ui.notify_after_sec ?? 4));
  // Hooks / MCP / plugins
  const [hooks, setHooks] = useState<{ name: string; events: string[]; builtin: boolean }[]>([]);
  const [hookDirs, setHookDirs] = useState<string[]>([]);
  const [servers, setServers] = useState<McpServer[]>([]);
  const [newServer, setNewServer] = useState({ name: "", target: "" });
  const [plugins, setPlugins] = useState<{ installed: { name: string; version: string; description: string }[]; available: { name: string; installed: boolean }[] }>({ installed: [], available: [] });

  useEffect(() => {
    void api.providers().then((d) => setProviders(d.providers)).catch(() => setProviders([]));
    void api.detectProviders().then((d) => setDetected(d.providers)).catch(() => setDetected([]));
    void api.hooks().then((d) => { setHooks(d.hooks); setHookDirs(d.dirs); }).catch(() => undefined);
    void api.mcpServers().then((d) => setServers(d.servers)).catch(() => undefined);
    void api.plugins().then(setPlugins).catch(() => undefined);
  }, []);

  const preset = providers.find((p) => p.id === provider);
  const detectedFor = detected.find((d) => d.provider === provider && d.running);

  function pickProvider(next: string) {
    setProvider(next);
    const p = providers.find((x) => x.id === next);
    if (p) {
      setEndpoint(p.endpoint);
      setKeyEnv(p.api_key_env || "OPENAI_API_KEY");
    }
    const hit = detected.find((d) => d.provider === next && d.running && d.models.length);
    if (hit) setName(hit.models[0].id);
  }

  async function testConnection() {
    setTestState({ busy: true, text: "" });
    try {
      const result = await api.testModel({ provider, name, endpoint, api_key_env: keyEnv });
      if (result.ok) setTestState({ busy: false, ok: true, text: `OK in ${result.latency_ms}ms — “${(result.reply || "").slice(0, 60)}”` });
      else setTestState({ busy: false, ok: false, text: result.error || "test failed" });
    } catch (err) {
      setTestState({ busy: false, ok: false, text: String(err) });
    }
  }

  async function save() {
    if (testState.text && testState.ok === false) onToast("Saving anyway — last test failed", "info");
    await onSave(
      {
        model: { default: name || provider, provider, endpoint, name, api_key_env: keyEnv },
        permissions: { level, network, require_approval_for_dangerous: approveDangerous },
        ui: { theme, ability, notify, notify_after_sec: notifyAfter },
      },
      apiKey,
      keyEnv,
    );
  }

  async function addServer() {
    if (!newServer.name.trim() || !newServer.target.trim()) return;
    const target = newServer.target.trim();
    const entry: McpServer = target.startsWith("http") ? { name: newServer.name.trim(), url: target, command: null } : { name: newServer.name.trim(), command: target.split(/\s+/), url: null };
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
      const saved = await api.saveMcpServers(servers.filter((s) => s.name !== nameToRemove));
      setServers(saved.servers);
    } catch (err) {
      onToast(String(err), "err");
    }
  }

  return (
    <div className="modal-back" onClick={onClose}>
      <div className="modal settings" onClick={(e) => e.stopPropagation()}>
        <nav className="settings-nav">
          <h2>Settings</h2>
          {SECTIONS.map((s) => (
            <button type="button" key={s.id} className={section === s.id ? "on" : ""} onClick={() => setSection(s.id)}>{s.label}</button>
          ))}
        </nav>
        <div className="settings-body">
          {section === "model" && (
            <section>
              <h3>Model</h3>
              <p className="hint">Pick a provider, then a model id. Local servers are detected automatically. Keys are stored in <code>secrets.env</code> (mode 600), never in YAML.</p>
              <div className="field">
                <label>Provider</label>
                <select value={provider} onChange={(e) => pickProvider(e.target.value)}>
                  {providers.length === 0 && <option value={provider}>{provider}</option>}
                  {providers.map((p) => (
                    <option key={p.id} value={p.id}>{p.label}{p.running ? " — running" : ""}</option>
                  ))}
                </select>
              </div>
              {detectedFor && detectedFor.models.length > 0 && (
                <div className="field">
                  <label>Installed models</label>
                  <div className="chip-row">
                    {detectedFor.models.map((m) => (
                      <button type="button" key={m.id} className={`chip ${m.id === name ? "on" : ""}`} onClick={() => setName(m.id)}>
                        {m.id}{m.detail ? <span className="dim"> {m.detail}</span> : null}
                      </button>
                    ))}
                  </div>
                </div>
              )}
              <div className="field">
                <label>Model id</label>
                <input value={name} onChange={(e) => setName(e.target.value)} placeholder="any id your provider accepts" />
              </div>
              <div className="field">
                <label>Endpoint</label>
                <input value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder={preset?.endpoint || "https://host/v1"} />
              </div>
              {(preset?.needs_key || provider === "openai_compatible") && (
                <>
                  <div className="field">
                    <label>API key env var</label>
                    <input value={keyEnv} onChange={(e) => setKeyEnv(e.target.value)} />
                  </div>
                  <div className="field">
                    <label>API key</label>
                    <input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder="leave blank to keep the current secret" />
                  </div>
                </>
              )}
              <div className="row">
                <button type="button" className="ghost" disabled={testState.busy} onClick={() => void testConnection()}>{testState.busy ? "Testing…" : "Test connection"}</button>
                {testState.text && <span className={testState.ok ? "health-ok" : "health-bad"}>{testState.text}</span>}
              </div>
            </section>
          )}

          {section === "permissions" && (
            <section>
              <h3>Permissions</h3>
              <div className="field">
                <label>Level</label>
                <select value={level} onChange={(e) => setLevel(e.target.value)}>
                  <option value="read_only">Read only — inspect, never write</option>
                  <option value="workspace">Workspace — edit and run inside the project (recommended)</option>
                  <option value="elevated">Elevated — dangerous commands after you approve</option>
                </select>
              </div>
              <label className="check"><input type="checkbox" checked={approveDangerous} onChange={(e) => setApproveDangerous(e.target.checked)} /> Ask before dangerous commands (rm -rf, git push --force, sudo…)</label>
              <label className="check"><input type="checkbox" checked={network} onChange={(e) => setNetwork(e.target.checked)} /> Allow network access from tools</label>
            </section>
          )}

          {section === "appearance" && (
            <section>
              <h3>Appearance</h3>
              <div className="field">
                <label>Theme</label>
                <div className="seg">
                  {["light", "dark"].map((t) => (
                    <button type="button" key={t} className={theme === t ? "on" : ""} onClick={() => setTheme(t)}>{t}</button>
                  ))}
                </div>
              </div>
              <label className="check"><input type="checkbox" checked={notify} onChange={(e) => setNotify(e.target.checked)} /> Desktop notification when a task finishes</label>
              <div className="field inline">
                <label>…only if it ran longer than</label>
                <input type="number" min={0} step={1} value={notifyAfter} onChange={(e) => setNotifyAfter(Number(e.target.value))} /> <span className="dim">seconds</span>
              </div>
              <div className="field">
                <label>Abilities</label>
                <select value={ability} onChange={(e) => setAbility(e.target.value)}>
                  <option value="none">none</option>
                  <option value="computer_use">Computer Use</option>
                  <option value="custom">Custom</option>
                </select>
                <p className="hint">Abilities live here so the composer stays minimal.</p>
              </div>
            </section>
          )}

          {section === "hooks" && (
            <section>
              <h3>Hooks</h3>
              <p className="hint">Deterministic lifecycle hooks fire at fixed points in the loop. Project hooks go in <code>{hookDirs[0] || ".shadowcode/hooks/"}</code> as <code>*.py</code> files exporting <code>register(registry)</code>.</p>
              <div className="list">
                {hooks.map((h) => (
                  <div className="item static" key={h.name}>
                    <strong>{h.name}{h.builtin ? <span className="dim"> · built-in</span> : null}</strong>
                    <span>{h.events.join(", ")}</span>
                  </div>
                ))}
                {hooks.length === 0 && <p className="hint">No hooks registered.</p>}
              </div>
            </section>
          )}

          {section === "mcp" && (
            <section>
              <h3>MCP servers</h3>
              <p className="hint">Tools from these servers are offered to the model. Use a URL for HTTP/SSE or a command line for stdio.</p>
              <div className="list">
                {servers.map((s) => (
                  <div className="item static" key={s.name}>
                    <strong>{s.name}</strong>
                    <span>{s.url || (s.command || []).join(" ")}</span>
                    <button type="button" className="mini danger-text" onClick={() => void removeServer(s.name)}>Remove</button>
                  </div>
                ))}
                {servers.length === 0 && <p className="hint">No MCP servers configured.</p>}
              </div>
              <div className="field inline">
                <input value={newServer.name} onChange={(e) => setNewServer({ ...newServer, name: e.target.value })} placeholder="name" />
                <input value={newServer.target} onChange={(e) => setNewServer({ ...newServer, target: e.target.value })} placeholder="http://127.0.0.1:7431/sse  or  npx -y some-mcp" />
                <button type="button" className="ghost" onClick={() => void addServer()}>Add</button>
              </div>
              <p className="hint">Expose ShadowCode itself: <code>shadow mcp serve</code> · <code>shadow mcp register</code>.</p>
            </section>
          )}

          {section === "plugins" && (
            <section>
              <h3>Plugins</h3>
              <div className="list">
                {plugins.available.map((p) => {
                  const meta = plugins.installed.find((m) => m.name === p.name);
                  return (
                    <div className="item static" key={p.name}>
                      <strong>{p.name}{meta ? <span className="dim"> · {meta.version}</span> : null}</strong>
                      <span>{meta?.description || "Not installed"}</span>
                      {p.installed ? (
                        <button type="button" className="mini danger-text" onClick={() => void api.removePlugin(p.name).then(() => api.plugins().then(setPlugins))}>Remove</button>
                      ) : (
                        <button type="button" className="mini" onClick={() => void api.installPlugin(p.name).then(() => api.plugins().then(setPlugins)).catch((err) => onToast(String(err), "err"))}>Install</button>
                      )}
                    </div>
                  );
                })}
                {plugins.available.length === 0 && <p className="hint">No plugins in the registry.</p>}
              </div>
            </section>
          )}

          <div className="row end settings-foot">
            <button type="button" className="ghost" onClick={onClose}>Close</button>
            {(section === "model" || section === "permissions" || section === "appearance") && (
              <button type="button" className="primary" onClick={() => void save()}>Save</button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
