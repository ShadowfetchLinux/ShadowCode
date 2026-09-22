import { useState, useMemo, useEffect } from "react";
import {
  Check,
  CheckCircle2,
  ChevronRight,
  Copy,
  Cpu,
  DownloadCloud,
  ExternalLink,
  Flame,
  HelpCircle,
  Play,
  RefreshCw,
  Search,
  Server,
  ShieldCheck,
  Sparkles,
  Terminal,
  X,
  Zap,
} from "lucide-react";
import { api, type ModelInfo, type ProviderInfo } from "../api";

interface OpenWeightHubProps {
  models: ModelInfo[];
  providers: ProviderInfo[];
  activeModelId: string;
  onSelectModel: (modelId: string) => void;
  onClose: () => void;
  onToast: (text: string, kind?: "ok" | "err" | "info") => void;
}

interface ChampionModel {
  id: string;
  name: string;
  creator: string;
  size: string;
  vramReq: string;
  vramFit: "optimal" | "quant" | "offload";
  context: string;
  description: string;
  pullCmd: string;
  tags: string[];
}

const CHAMPIONS: ChampionModel[] = [
  {
    id: "qwen2.5-coder:14b",
    name: "Qwen 2.5 Coder 14B",
    creator: "Alibaba Cloud",
    size: "9.0 GB",
    vramReq: "10 GB VRAM",
    vramFit: "optimal",
    context: "32k–128k",
    description:
      "The premier open-weight coding champion. Exceptional performance in Python, TypeScript, Rust, autonomous tool use, and multi-file code editing.",
    pullCmd: "ollama pull qwen2.5-coder:14b",
    tags: ["SOTA Coding", "Full Tool Use", "Optimal 16GB GPU"],
  },
  {
    id: "deepseek-r1:14b",
    name: "DeepSeek R1 14B",
    creator: "DeepSeek AI",
    size: "9.0 GB",
    vramReq: "10 GB VRAM",
    vramFit: "optimal",
    context: "64k–128k",
    description:
      "State-of-the-art chain-of-thought reasoning model. Unfolds step-by-step logic before writing code, perfect for architectural design and hard bug diagnosis.",
    pullCmd: "ollama pull deepseek-r1:14b",
    tags: ["Chain of Thought", "Architecture", "Deep Debugging"],
  },
  {
    id: "codestral:22b",
    name: "Mistral Codestral 22B",
    creator: "Mistral AI",
    size: "13.0 GB",
    vramReq: "14 GB VRAM",
    vramFit: "optimal",
    context: "32k",
    description:
      "Mistral's dedicated coding model. Exceptional at fill-in-the-middle, refactoring, test generation, and multi-language fluency with concise, clean output.",
    pullCmd: "ollama pull codestral:22b",
    tags: ["Mistral AI", "Long Context", "Code Specialist"],
  },
  {
    id: "huihui_ai/gemma-4-abliterated:12b",
    name: "Gemma 4 12B (Abliterated)",
    creator: "Google Open Weights / Community",
    size: "7.6 GB",
    vramReq: "8 GB VRAM",
    vramFit: "optimal",
    context: "128k",
    description:
      "Ultra-fast local open weights with uncapped instruction following. High generation speed and low memory footprint.",
    pullCmd: "ollama pull huihui_ai/gemma-4-abliterated:12b",
    tags: ["Local Installed", "Ultra Fast", "Vision Capable"],
  },
  {
    id: "qwen2.5-coder:7b",
    name: "Qwen 2.5 Coder 7B",
    creator: "Alibaba Cloud",
    size: "4.7 GB",
    vramReq: "6 GB VRAM",
    vramFit: "optimal",
    context: "32k",
    description:
      "Lightweight powerhouse. Runs blistering fast on any modern GPU or CPU, delivering surprising coding accuracy for daily edits.",
    pullCmd: "ollama pull qwen2.5-coder:7b",
    tags: ["Lightweight", "High Speed", "Laptop Friendly"],
  },
  {
    id: "qwen2.5-coder:32b",
    name: "Qwen 2.5 Coder 32B",
    creator: "Alibaba Cloud",
    size: "20.0 GB",
    vramReq: "22 GB VRAM (Q4)",
    vramFit: "quant",
    context: "128k",
    description:
      "Heavyweight coding authority. Matches proprietary frontier models on SWE-bench benchmarks. Requires high VRAM or Q4_K_M quantization.",
    pullCmd: "ollama pull qwen2.5-coder:32b",
    tags: ["Frontier Class", "SWE-Bench Titan", "32B Weights"],
  },
  {
    id: "llama3.3:70b",
    name: "Llama 3.3 70B",
    creator: "Meta AI",
    size: "42.0 GB",
    vramReq: "42 GB VRAM",
    vramFit: "offload",
    context: "128k",
    description:
      "Meta's flagship open-weight model. Incomparable general reasoning, complex instruction synthesis, and multilingual code support.",
    pullCmd: "ollama pull llama3.3:70b",
    tags: ["Meta Flagship", "Frontier General", "High VRAM"],
  },
  {
    id: "devstral",
    name: "Mistral Devstral / Codestral",
    creator: "Mistral AI",
    size: "14.0 GB",
    vramReq: "14 GB VRAM",
    vramFit: "optimal",
    context: "32k",
    description:
      "Engineered by Mistral specifically for code generation, fill-in-the-middle completion, and test authoring with concise elegance.",
    pullCmd: "ollama pull devstral",
    tags: ["Mistral AI", "Clean Syntax", "Fast Completion"],
  },
];

export function OpenWeightHub({
  models,
  providers,
  activeModelId,
  onSelectModel,
  onClose,
  onToast,
}: OpenWeightHubProps) {
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<"all" | "installed" | "optimal">("all");
  const [copiedId, setCopiedId] = useState("");
  const [testingModel, setTestingModel] = useState<string | null>(null);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  // Installed models list
  const installedMap = useMemo(() => {
    const map = new Map<string, ModelInfo>();
    for (const m of models) {
      map.set(m.name.toLowerCase(), m);
      map.set(m.id.toLowerCase(), m);
    }
    return map;
  }, [models]);

  const localProviders = useMemo(() => {
    return providers.filter((p) =>
      ["ollama", "local", "llamacpp", "vllm", "lm_studio", "lmstudio"].includes(p.id)
    );
  }, [providers]);

  const filteredChampions = useMemo(() => {
    return CHAMPIONS.filter((c) => {
      const isInstalled =
        installedMap.has(c.id.toLowerCase()) ||
        installedMap.has(c.name.toLowerCase());

      if (filter === "installed" && !isInstalled) return false;
      if (filter === "optimal" && c.vramFit !== "optimal") return false;

      if (search.trim()) {
        const q = search.toLowerCase();
        return (
          c.name.toLowerCase().includes(q) ||
          c.creator.toLowerCase().includes(q) ||
          c.description.toLowerCase().includes(q) ||
          c.tags.some((t) => t.toLowerCase().includes(q))
        );
      }
      return true;
    });
  }, [filter, search, installedMap]);

  const copyCommand = async (cmd: string, id: string) => {
    try {
      if (!navigator.clipboard) throw new Error("Clipboard access is unavailable");
      await navigator.clipboard.writeText(cmd);
      setCopiedId(id);
      onToast(`Copied command: ${cmd}`, "ok");
      setTimeout(() => setCopiedId(""), 3000);
    } catch (error) {
      onToast(`Could not copy command: ${String(error)}`, "err");
    }
  };

  const testModel = async (championId: string) => {
    setTestingModel(championId);
    try {
      const match = models.find(
        (m) =>
          m.id.toLowerCase().includes(championId.toLowerCase()) ||
          m.name.toLowerCase().includes(championId.toLowerCase())
      );
      if (!match) {
        onToast("Model is not registered locally yet. Pull it first.", "info");
        setTestingModel(null);
        return;
      }
      const res = await api.testModel({
        provider: match.provider,
        name: match.name,
        endpoint: match.endpoint,
        api_key_env: (match.metadata?.api_key_env as string) || "OPENAI_API_KEY",
      });
      if (res.ok) {
        onToast(`Connection verified in ${res.latency_ms}ms!`, "ok");
      } else {
        onToast(`Test failed: ${res.error || "Unknown error"}`, "err");
      }
    } catch (e) {
      onToast(String(e), "err");
    } finally {
      setTestingModel(null);
    }
  };

  return (
    <div className="modal-back open-weight-hub-backdrop" role="dialog" aria-modal="true" aria-labelledby="hub-title">
      <div className="open-weight-hub-modal">
        <header className="hub-header">
          <div className="hub-title-wrap">
            <div className="hub-badge">
              <Cpu size={14} />
              <span>OPEN WEIGHTS PILLAR</span>
            </div>
            <h2 id="hub-title">Open-Weight Models Showcase</h2>
            <p className="hub-sub">
              Curated coding powerhouses that run locally on your GPU or dedicated server. Zero telemetry. Zero API costs.
            </p>
          </div>
          <button
            type="button"
            className="icon-btn close-btn"
            aria-label="Close hub"
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </header>

        <div className="hub-status-bar">
          <div className="hub-hardware-pill">
            <Zap size={14} className="accent-icon" />
            <span><strong>Hardware guide:</strong> match model size to your available VRAM</span>
          </div>

          <div className="hub-provider-pills">
            {localProviders.map((p) => (
              <div
                key={p.id}
                className={`provider-pill ${p.running ? "online" : "offline"}`}
              >
                <span className="dot" />
                <span>{p.label || p.id}</span>
                <span className="count">
                  {p.running
                    ? `${models.filter((m) => m.provider === p.id).length} models`
                    : "offline"}
                </span>
              </div>
            ))}
          </div>
        </div>

        <div className="hub-toolbar">
          <div className="search-box">
            <Search size={15} />
            <input
              type="text"
              placeholder="Search open-weight models (Qwen, DeepSeek, Llama...)"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
            {search && (
              <button
                type="button"
                className="icon-btn mini"
                onClick={() => setSearch("")}
              >
                <X size={13} />
              </button>
            )}
          </div>

          <div className="hub-filters">
            <button
              type="button"
              className={`filter-btn ${filter === "all" ? "active" : ""}`}
              onClick={() => setFilter("all")}
            >
              All Champions
            </button>
            <button
              type="button"
              className={`filter-btn ${filter === "installed" ? "active" : ""}`}
              onClick={() => setFilter("installed")}
            >
              Installed Locally ({models.filter((m) => m.detected).length})
            </button>
            <button
              type="button"
              className={`filter-btn ${filter === "optimal" ? "active" : ""}`}
              onClick={() => setFilter("optimal")}
            >
              ⚡ Optimal 16GB Fit
            </button>
          </div>
        </div>

        <div className="hub-champions-grid">
          {filteredChampions.map((c) => {
            const installedModel =
              models.find(
                (m) =>
                  m.id.toLowerCase() === c.id.toLowerCase() ||
                  m.name.toLowerCase() === c.id.toLowerCase() ||
                  m.name.toLowerCase().includes(c.id.toLowerCase())
              ) || null;

            const isInstalled = Boolean(installedModel);
            const isSelected =
              installedModel?.id === activeModelId ||
              activeModelId.toLowerCase().includes(c.id.toLowerCase());

            return (
              <div
                key={c.id}
                className={`champion-card ${isSelected ? "selected" : ""} ${
                  isInstalled ? "installed" : ""
                }`}
              >
                <div className="champion-header">
                  <div>
                    <div className="champion-creator">{c.creator}</div>
                    <h4 className="champion-name">{c.name}</h4>
                  </div>
                  <div className="champion-badges">
                    <span
                      className={`vram-fit-badge ${c.vramFit}`}
                      title={c.vramReq}
                    >
                      {c.vramFit === "optimal"
                        ? "⚡ Optimal Fit"
                        : c.vramFit === "quant"
                        ? "💾 Quantized"
                        : "⚠️ High VRAM"}
                    </span>
                    {isInstalled && (
                      <span className="installed-badge">
                        <Check size={12} /> Installed
                      </span>
                    )}
                  </div>
                </div>

                <p className="champion-desc">{c.description}</p>

                <div className="champion-specs">
                  <div className="spec-item">
                    <span className="spec-label">Weight Size</span>
                    <span className="spec-val">{c.size}</span>
                  </div>
                  <div className="spec-item">
                    <span className="spec-label">VRAM Req</span>
                    <span className="spec-val">{c.vramReq}</span>
                  </div>
                  <div className="spec-item">
                    <span className="spec-label">Context</span>
                    <span className="spec-val">{c.context}</span>
                  </div>
                </div>

                <div className="champion-tags">
                  {c.tags.map((t) => (
                    <span key={t} className="tag-pill">
                      {t}
                    </span>
                  ))}
                </div>

                <div className="champion-footer">
                  {isInstalled ? (
                    <div className="installed-actions">
                      <button
                        type="button"
                        className={`primary-btn ${isSelected ? "active" : ""}`}
                        disabled={isSelected}
                        onClick={() => {
                          if (installedModel) {
                            onSelectModel(installedModel.id);
                            onToast(`Switched to ${c.name}`, "ok");
                            onClose();
                          }
                        }}
                      >
                        {isSelected ? (
                          <>
                            <Check size={14} /> Active Model
                          </>
                        ) : (
                          <>
                            <Play size={13} /> Select for Tasks
                          </>
                        )}
                      </button>
                      <button
                        type="button"
                        className="ghost-btn"
                        title="Test latency & connection"
                        disabled={testingModel === c.id}
                        onClick={() => void testModel(c.id)}
                      >
                        {testingModel === c.id ? (
                          <RefreshCw size={13} className="spin" />
                        ) : (
                          "Test"
                        )}
                      </button>
                    </div>
                  ) : (
                    <div className="pull-actions">
                      <code className="cmd-preview">{c.pullCmd}</code>
                      <button
                        type="button"
                        className="ghost-btn copy-btn"
                        title="Copy pull command"
                        onClick={() => void copyCommand(c.pullCmd, c.id)}
                      >
                        {copiedId === c.id ? (
                          <Check size={13} className="success-icon" />
                        ) : (
                          <Copy size={13} />
                        )}
                      </button>
                    </div>
                  )}
                </div>
              </div>
            );
          })}
        </div>

        <footer className="hub-footer">
          <div className="hub-footer-hint">
            <ShieldCheck size={16} />
            <span>
              Ollama, LM Studio, vLLM, and llama.cpp run on your loopback address. Models execute zero cloud telemetry.
            </span>
          </div>
          <button type="button" className="ghost" onClick={onClose}>
            Close
          </button>
        </footer>
      </div>
    </div>
  );
}
