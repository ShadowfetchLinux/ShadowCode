import { useEffect, useMemo, useState } from "react";
import { api, type DetectedProvider } from "../api";
import { isNative, pickDirectory } from "../lib/transport";
import { FlowGuide } from "./FlowGuide";
import { Sparkles, Cpu, ShieldCheck } from "lucide-react";

type Pick = {
  provider: string;
  model: string;
  endpoint: string;
  api_key_env: string;
};

/** One screen, one click. Pick a detected model → Test & start. */
export function Onboarding({ onDone }: { onDone: () => void }) {
  const [showGuide, setShowGuide] = useState(false);
  const [workspace, setWorkspace] = useState("");
  const [detected, setDetected] = useState<DetectedProvider[]>([]);
  const [presets, setPresets] = useState<
    {
      id: string;
      provider: string;
      endpoint?: string;
      api_key_env?: string;
      needs_key?: boolean;
    }[]
  >([]);
  const [pick, setPick] = useState<Pick>({
    provider: "mock",
    model: "mock-coder",
    endpoint: "",
    api_key_env: "",
  });
  const [advanced, setAdvanced] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [level, setLevel] = useState("workspace");
  const [state, setState] = useState<{
    busy: boolean;
    text: string;
    ok?: boolean;
  }>({ busy: false, text: "" });
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    api
      .onboarding()
      .then((data) => {
        setWorkspace(data.suggested_workspace);
        setPresets(data.providers);
        setDetected(data.detected || []);
        const running = (data.detected || []).filter(
          (d) => d.running && d.models.length,
        );
        const first = running[0];
        if (first)
          setPick({
            provider: first.provider,
            model: first.models[0].id,
            endpoint: first.endpoint,
            api_key_env: "",
          });
        setLoaded(true);
      })
      .catch(() => setLoaded(true));
  }, []);

  const installed = useMemo(
    () =>
      detected
        .filter((d) => d.running)
        .flatMap((d) =>
          d.models.map((m) => ({
            ...m,
            provider: d.provider,
            endpoint: d.endpoint,
            label: d.label,
          })),
        ),
    [detected],
  );
  const needsKey =
    presets.find((p) => p.id === pick.provider || p.provider === pick.provider)
      ?.needs_key &&
    !["ollama", "local", "llamacpp", "vllm", "mock"].includes(pick.provider);

  async function testAndStart() {
    setState({ busy: true, text: "Testing model…" });
    try {
      if (apiKey.trim()) {
        // Save the key first so the probe can use it.
        await api.saveConfig(
          {},
          apiKey.trim(),
          pick.api_key_env || "OPENAI_API_KEY",
        );
      }
      if (pick.provider !== "mock") {
        const result = await api.testModel({
          provider: pick.provider,
          name: pick.model,
          endpoint: pick.endpoint,
          api_key_env: pick.api_key_env,
        });
        if (!result.ok) {
          setState({
            busy: false,
            ok: false,
            text:
              result.error ||
              "The model did not answer. Check the provider and try again.",
          });
          return;
        }
        setState({
          busy: true,
          ok: true,
          text: `Connected in ${result.latency_ms}ms — starting…`,
        });
      }
      await api.completeOnboarding({
        workspace,
        provider: pick.provider,
        model: pick.model,
        name: pick.model,
        endpoint: pick.endpoint,
        api_key_env: pick.api_key_env,
        permission_level: level,
        theme: "light",
      });
      onDone();
    } catch (err) {
      setState({ busy: false, ok: false, text: String(err) });
    }
  }

  return (
    <div className="modal-back">
      <div className="wizard">
        <div className="onboarding-flagship-badge">
          <Cpu size={14} />
          <span>PILLAR FOR OPEN SOURCE & OPEN WEIGHTS</span>
        </div>
        <p className="kicker">ShadowCode</p>
        <h2>Pick a model, start working</h2>
        <button
          type="button"
          className="onboarding-guide-btn"
          onClick={() => setShowGuide(true)}
        >
          <Sparkles size={14} />
          <span>New to ShadowCode? Open 20-Minute Fast-Track Guide</span>
        </button>
        <div className="field">
          <label htmlFor="onboarding-field-1">Project folder</label>
          <div className="input-action">
            <input
              id="onboarding-field-1"
              value={workspace}
              onChange={(e) => setWorkspace(e.target.value)}
              placeholder="/path/to/project"
            />
            {isNative() && (
              <button
                type="button"
                className="ghost"
                disabled={state.busy}
                onClick={() =>
                  void pickDirectory()
                    .then((path) => {
                      if (path) setWorkspace(path);
                    })
                    .catch((error) =>
                      setState({ busy: false, ok: false, text: String(error) }),
                    )
                }
              >
                Browse…
              </button>
            )}
          </div>
        </div>
        <div className="field">
          <div className="hint">
            Model{" "}
            {installed.length > 0 && (
              <span className="dim">· detected on this machine</span>
            )}
          </div>
          {!loaded && (
            <div className="skel-rows">
              <span className="skel" />
              <span className="skel short" />
            </div>
          )}
          {loaded && installed.length === 0 && (
            <p className="hint">
              No local model server found. Start Ollama (or LM Studio /
              llama.cpp / vLLM) and reload, use an API provider below, or start
              with the offline preview.
            </p>
          )}
          <div className="model-grid">
            {installed.map((m) => {
              const on = pick.model === m.id && pick.provider === m.provider;
              return (
                <button
                  type="button"
                  key={`${m.provider}:${m.id}`}
                  className={`model-tile ${on ? "on" : ""}`}
                  onClick={() =>
                    setPick({
                      provider: m.provider,
                      model: m.id,
                      endpoint: m.endpoint,
                      api_key_env: "",
                    })
                  }
                >
                  <strong>{m.id}</strong>
                  <span>
                    {m.label}
                    {m.detail ? ` · ${m.detail}` : ""}
                    {m.capabilities?.tools ? " · tools" : ""}
                  </span>
                </button>
              );
            })}
            <button
              type="button"
              className={`model-tile ${pick.provider === "mock" ? "on" : ""}`}
              onClick={() =>
                setPick({
                  provider: "mock",
                  model: "mock-coder",
                  endpoint: "",
                  api_key_env: "",
                })
              }
            >
              <strong>{isNative() ? "Offline preview" : "Mock"}</strong>
              <span>
                {isNative()
                  ? "Explore the workspace; select a model to run tasks"
                  : "offline · no key · for trying the harness"}
              </span>
            </button>
          </div>
        </div>
        <button
          type="button"
          className="link"
          onClick={() => setAdvanced(!advanced)}
        >
          {advanced ? "Hide" : "Use an API provider or a custom model id…"}
        </button>
        {advanced && (
          <div className="advanced">
            <div className="field">
              <label htmlFor="onboarding-field-2">Provider</label>
              <select
                id="onboarding-field-2"
                value={pick.provider}
                onChange={(e) => {
                  const id = e.target.value;
                  const p = presets.find((x) => x.id === id);
                  setPick({
                    provider: id,
                    model: "",
                    endpoint: p?.endpoint || "",
                    api_key_env: p?.api_key_env || "",
                  });
                }}
              >
                {presets.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.id}
                  </option>
                ))}
              </select>
            </div>
            <div className="field">
              <label htmlFor="onboarding-field-3">Model id</label>
              <input
                id="onboarding-field-3"
                value={pick.model}
                onChange={(e) => setPick({ ...pick, model: e.target.value })}
                placeholder="gpt-4.1 · grok-4 · qwen3:14b"
              />
            </div>
            <div className="field">
              <label htmlFor="onboarding-field-4">Endpoint</label>
              <input
                id="onboarding-field-4"
                value={pick.endpoint}
                onChange={(e) => setPick({ ...pick, endpoint: e.target.value })}
                placeholder="https://host/v1"
              />
            </div>
            {needsKey && (
              <div className="field">
                <label htmlFor="onboarding-field-5">
                  API key{" "}
                  <span className="dim">
                    ({pick.api_key_env || "OPENAI_API_KEY"})
                  </span>
                </label>
                <input
                  id="onboarding-field-5"
                  type="password"
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                  placeholder="stored in secrets.env (600)"
                />
              </div>
            )}
            <div className="field">
              <label htmlFor="onboarding-field-6">Permissions</label>
              <select
                id="onboarding-field-6"
                value={level}
                onChange={(e) => setLevel(e.target.value)}
              >
                <option value="read_only">Read only</option>
                <option value="workspace">Workspace (recommended)</option>
                <option value="elevated">Elevated</option>
              </select>
            </div>
          </div>
        )}
        {state.text && (
          <p className={state.ok === false ? "health-bad" : "health-ok"}>
            {state.text}
          </p>
        )}
        <div className="row end">
          <button
            type="button"
            className="primary"
            disabled={state.busy || !workspace.trim() || !pick.model.trim()}
            onClick={() => void testAndStart()}
          >
            {state.busy
              ? "Working…"
              : pick.provider === "mock"
                ? "Start"
                : "Test & start"}
          </button>
        </div>
        <p className="hint">
          Workspace-level permissions by default: the agent edits and runs
          commands only inside this folder and asks before anything dangerous.
          Change anything later in Settings.
        </p>
      </div>
      {showGuide && <FlowGuide onClose={() => setShowGuide(false)} />}
    </div>
  );
}
