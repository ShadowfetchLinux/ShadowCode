/**
 * Deterministic fake engine for UI tests. It implements the subset of
 * docs/API_CONTRACT_0.28.md the window uses and installs itself as
 * window.__SHADOW_TEST_TRANSPORT__. The app honours that global only in builds
 * made with VITE_SHADOW_TEST_TRANSPORT=1 (see src/lib/transport.ts).
 *
 * The function is self-contained on purpose: Playwright serialises it into an
 * init script, so it must not reference anything outside its own body.
 */
export type FakeOptions = {
  /** Milliseconds between streamed task events. */
  stepMs?: number;
  /** Start with the onboarding screen. */
  onboarding?: boolean;
  /** Codex tasks stop at the plan limit (then limits.on_limit applies). */
  limitOnCodex?: boolean;
};

export function installFakeBackend(options: FakeOptions = {}) {
  type Json = Record<string, any>;
  const step = options.stepMs ?? 60;
  const now = () => Math.floor(Date.now() / 1000);
  const workspace = "/work/demo";
  const log: { method: string; path: string; body: any }[] = [];
  const listeners: Record<string, ((payload: unknown) => void)[]> = {};
  const wake = () =>
    (listeners["shadowcode:events"] || []).forEach((fn) => fn({}));

  const unknownUsage = {
    state: "unavailable",
    label: "Usage unavailable · Open provider usage",
    detail: [],
    plan: null,
    pool: null,
    pool_shared: false,
    windows: [],
    remaining_percent: null,
    credits: null,
    limit_reached: false,
    last_refresh: null,
    provider_usage_url: null,
  };
  const codexUsage = {
    ...unknownUsage,
    state: "ok",
    label: "Shared plan usage · 2% left · resets in 3h",
    detail: [
      "ChatGPT plan: pro",
      "Weekly: 98% used · resets in 3h",
      "Shared pool: codex (all models on this account)",
    ],
    plan: "pro",
    pool: "codex",
    pool_shared: true,
    windows: [
      {
        label: "Weekly",
        used_percent: 98,
        remaining_percent: 2,
        window_minutes: 10080,
        resets_at: now() + 3 * 3600,
      },
    ],
    remaining_percent: 2,
    last_refresh: now() - 60,
    provider_usage_url: "https://chatgpt.com/codex/settings/usage",
  };
  // Antigravity runs through Google's ACP agent server, which ShadowCode
  // downloads only when the user chooses Install.
  const agentBytes = 333_590_110;
  const agentInstall = (over: Json = {}) => ({
    installed: false,
    version: "1.2.1",
    path: null as null | string,
    managed: false,
    download_bytes: agentBytes,
    installed_bytes: 1_052_767_112,
    source:
      "https://dl.google.com/agy-extensions/releases/linux/agy-acp-server-1.2.1-linux-x86_64.zip",
    dir: "/home/user/.local/share/shadowcode/antigravity-acp/1.2.1",
    state: "not_installed",
    busy: false,
    done: 0,
    total: 0,
    error: null as null | string,
    ...over,
  });
  const agentNotInstalled = {
    state: "not_installed",
    status: "warn",
    availability: "setup_required",
    availability_label: "Setup required",
    detail:
      "Install the Antigravity agent from Settings › Accounts (Google's official ACP server, a 334 MB download from dl.google.com).",
    fix: "Install the Antigravity agent from Settings › Accounts (Google's official ACP server, a 334 MB download from dl.google.com).",
    version: null,
    account: null,
    models: [],
  };
  // Cursor's pool is spent: its rows stay visible with the reason.
  const cursorLimited = {
    ...unknownUsage,
    state: "limit_reached",
    label: "Plan limit reached · resets in 2h",
    detail: ["Plan limit reached · resets in 2h"],
    plan: "Pro",
    windows: [
      {
        label: "Monthly",
        used_percent: 100,
        remaining_percent: 0,
        window_minutes: 43200,
        resets_at: now() + 2 * 3600,
      },
    ],
    remaining_percent: 0,
    limit_reached: true,
    last_refresh: now() - 300,
    provider_usage_url: "https://cursor.com/dashboard?tab=usage",
  };
  const localUsage = {
    ...unknownUsage,
    state: "local",
    label: "Runs on this computer · No subscription quota",
  };

  // OpenRouter catalogue: [slug, name, $/M in, $/M out, tools, vision].
  // Enough rows that the picker collapses the section.
  const openrouterSeed: [string, string, number, number, boolean, boolean][] = [
    ["qwen/qwen3-coder", "Qwen: Qwen3 Coder", 0.3, 1.2, true, false],
    ["qwen/qwen3-235b-a22b", "Qwen: Qwen3 235B A22B", 0.13, 0.6, true, false],
    [
      "qwen/qwen2.5-vl-72b-instruct",
      "Qwen: Qwen2.5 VL 72B",
      0.25,
      0.75,
      false,
      true,
    ],
    [
      "deepseek/deepseek-chat-v3.1",
      "DeepSeek: DeepSeek V3.1",
      0.2,
      0.8,
      true,
      false,
    ],
    ["deepseek/deepseek-r1:free", "DeepSeek: R1 (free)", 0, 0, false, false],
    ["moonshotai/kimi-k2", "MoonshotAI: Kimi K2", 0.14, 2.49, true, false],
    ["z-ai/glm-4.6", "Z.AI: GLM 4.6", 0.4, 1.75, true, false],
    [
      "mistralai/devstral-medium",
      "Mistral: Devstral Medium",
      0.4,
      2,
      true,
      false,
    ],
    [
      "mistralai/mistral-small-3.2-24b-instruct",
      "Mistral: Mistral Small 3.2 24B",
      0.05,
      0.1,
      true,
      true,
    ],
    [
      "google/gemini-2.5-flash",
      "Google: Gemini 2.5 Flash",
      0.3,
      2.5,
      true,
      true,
    ],
    [
      "google/gemma-3-27b-it:free",
      "Google: Gemma 3 27B (free)",
      0,
      0,
      false,
      true,
    ],
    [
      "meta-llama/llama-3.3-70b-instruct",
      "Meta: Llama 3.3 70B Instruct",
      0.13,
      0.4,
      true,
      false,
    ],
    [
      "meta-llama/llama-4-maverick",
      "Meta: Llama 4 Maverick",
      0.15,
      0.6,
      true,
      true,
    ],
    ["openai/gpt-oss-120b", "OpenAI: gpt-oss-120b", 0.05, 0.25, true, false],
    ["x-ai/grok-code-fast-1", "xAI: Grok Code Fast 1", 0.2, 1.5, true, false],
    [
      "nousresearch/hermes-3-llama-3.1-405b",
      "Nous: Hermes 3 405B",
      0.7,
      0.8,
      false,
      false,
    ],
  ];
  for (let i = openrouterSeed.length; i < 40; i++)
    openrouterSeed.push([
      `vendor${i % 5}/model-${i}`,
      `Vendor ${i % 5}: Model ${i}`,
      0.1 * (i % 7),
      0.4 * (i % 7),
      i % 4 !== 0,
      i % 3 === 0,
    ]);
  const openrouterModels = openrouterSeed.map(
    ([slug, name, input, output, tools, vision]) => ({
      slug,
      name,
      input,
      output,
      tools,
      vision,
    }),
  );

  const state: Json = {
    onboarded: !options.onboarding,
    config: {
      model: { default: "mock", provider: "mock", name: "mock-coder" },
      permissions: {
        level: "workspace",
        mode: "ask",
        network: false,
        require_approval_for_dangerous: true,
        vendor_notes: {
          codex: "Codex sandbox: workspace-write with approvals for commands.",
          cursor: "Cursor asks through ACP permission requests.",
        },
      },
      network: { mode: "online" },
      ui: { theme: "light", notify: true },
      cli_agents: { enabled: true },
      guardian: { enabled: false, interval_sec: 3600 },
      limits: { on_limit: "local", fallback_model: "" },
    },
    vendors: {
      codex: {
        id: "cli-codex",
        label: "Codex",
        state: "ready",
        status: "pass",
        availability: "ready",
        availability_label: "Ready",
        detail: "Signed in with ChatGPT",
        version: "codex-cli 0.155.0",
        binary: "codex",
        fix: null,
        account: {
          email: "dev@example.com",
          plan: "Pro",
          auth_mode: "chatgpt",
        },
        models: [
          {
            id: "gpt-6-astra",
            label: "GPT-6-Astra",
            is_default: true,
            vision: true,
          },
          {
            id: "gpt-6-luna",
            label: "GPT-6-Luna",
            is_default: false,
            vision: true,
          },
        ],
        accepts_images: true,
        asks_approval: true,
        fetched_at: now(),
        error: null,
        usage_note: null,
        login_command: ["codex", "login"],
        logout_command: ["codex", "logout"],
        shared_cli_note:
          "This signs out the codex CLI for your whole user account, not just ShadowCode.",
        usage: codexUsage,
      },
      claude: {
        id: "cli-claude",
        label: "Claude Code",
        state: "not_logged_in",
        status: "warn",
        availability: "sign_in",
        availability_label: "Sign in",
        detail: "Not signed in",
        version: "2.1.278",
        binary: "claude",
        fix: "Connect to sign in with your Claude subscription.",
        account: null,
        models: [],
        accepts_images: true,
        asks_approval: true,
        fetched_at: now(),
        error: null,
        usage_note: "Claude Code does not report plan usage.",
        login_command: ["claude", "auth", "login"],
        logout_command: ["claude", "auth", "logout"],
        shared_cli_note:
          "This signs out the claude CLI for your whole user account, not just ShadowCode.",
        usage: unknownUsage,
      },
      antigravity: {
        id: "cli-antigravity",
        label: "Antigravity (Google's ACP agent)",
        product: "Antigravity",
        ...agentNotInstalled,
        binary: "agy_acp_server.par",
        accepts_images: true,
        asks_approval: true,
        fetched_at: now(),
        error: null,
        usage_note: "Antigravity does not report plan usage.",
        login_command: [],
        logout_command: [],
        shared_cli_note:
          "This deletes ShadowCode's private Antigravity profile (its Google sign-in). The agy CLI and the Antigravity app keep their own sign-in.",
        usage: unknownUsage,
        install: agentInstall(),
      },
      cursor: {
        id: "cli-cursor",
        label: "Cursor",
        state: "ready",
        status: "pass",
        availability: "ready",
        availability_label: "Ready",
        detail: "Signed in",
        version: "2026.09.12",
        binary: "cursor-agent",
        fix: null,
        account: { email: "dev@example.com", plan: "Pro", auth_mode: "oauth" },
        models: [
          { id: "auto", label: "Auto", is_default: true, vision: false },
        ],
        accepts_images: false,
        asks_approval: true,
        fetched_at: now(),
        error: null,
        usage_note: null,
        login_command: ["cursor-agent", "login"],
        logout_command: ["cursor-agent", "logout"],
        shared_cli_note:
          "This signs out the cursor-agent CLI for your whole user account, not just ShadowCode.",
        usage: cursorLimited,
      },
      grok: {
        id: "cli-grok",
        label: "Grok",
        state: "not_installed",
        status: "warn",
        availability: "setup_required",
        availability_label: "Setup required",
        detail: "Install the Grok CLI, then refresh Accounts.",
        fix: "Install the Grok CLI, then refresh Accounts.",
        version: null,
        binary: null,
        account: null,
        models: [],
        accepts_images: false,
        asks_approval: true,
        fetched_at: now(),
        error: null,
        usage_note: "Grok does not report plan usage.",
        login_command: ["grok", "login"],
        logout_command: ["grok", "logout"],
        shared_cli_note: "",
        usage: unknownUsage,
      },
    },
    /** Codex tasks stop at the plan limit (see `limitScript`). */
    limitOnCodex: Boolean(options.limitOnCodex),
    /** The last local model a task ran on (the automatic fallback). */
    lastLocal: "" as string,
    /** Install progress steps still to come (one per status check);
     * `installFails` makes the next install stop with an error. */
    agent: { steps: [] as Json[], installFails: false, hold: false },
    login: null as null | { vendor: string; polls: number },
    local: {
      hardware: {
        cpu_cores: 16,
        ram_bytes: 62 * 1024 ** 3,
        gpu: "NVIDIA GeForce RTX 5060 Ti",
        vram_bytes: 16 * 1024 ** 3,
        backend: "vulkan",
        devices: ["Vulkan0: NVIDIA GeForce RTX 5060 Ti (16311 MiB)"],
        detail: "",
      },
      runtime: {
        state: "ready",
        path: "/opt/shadowcode/llama-server",
        origin: "bundled",
        version: "b6500",
        backend: "vulkan",
        commit: "abc1234",
        detail: "Managed llama.cpp with Vulkan and CPU backends",
      },
      models: [
        {
          id: "local:gguf:qwen",
          name: "qwen3:14b",
          path: "/models/ollama/blobs/sha256-qwen",
          bytes: 9_300_000_000,
          source: "ollama",
          architecture: "qwen3",
          context_train: 40960,
          context_tokens: 16384,
          compatible: true,
          reason: "Supported architecture",
          vision: false,
          mmproj: null,
          tools: true,
          tools_reason: "Chat template supports tools",
          memory: {
            weights_bytes: 9_000_000_000,
            kv_cache_bytes: 2_600_000_000,
            compute_bytes: 300_000_000,
            projector_bytes: 0,
            overhead_bytes: 200_000_000,
            total_bytes: 12_100_000_000,
            context_tokens: 16384,
          },
          fits: "gpu",
          availability: "ready",
          last_error: null,
        },
        {
          id: "local:gguf:gptoss",
          name: "gpt-oss:20b",
          path: "/models/ollama/blobs/sha256-gptoss",
          bytes: 13_000_000_000,
          source: "ollama",
          architecture: "gptoss",
          context_train: 131072,
          context_tokens: 8192,
          compatible: false,
          reason: "unknown model architecture: gptoss",
          vision: false,
          mmproj: null,
          tools: false,
          tools_reason: "",
          memory: null,
          fits: "no",
          availability: "unavailable",
          last_error: null,
        },
      ],
      loaded: null,
      ollama_store: {
        path: "/home/user/models/ollama",
        available: true,
        models: [
          {
            tag: "qwen3:14b",
            path: "/models/ollama/blobs/sha256-qwen",
            projector: null,
            bytes: 9_300_000_000,
            compatible: true,
            reason: "",
            already_added: true,
          },
          {
            tag: "gemma-4:12b",
            path: "/models/ollama/blobs/sha256-gemma",
            projector: "/models/ollama/blobs/sha256-proj",
            bytes: 8_100_000_000,
            compatible: true,
            reason: "",
            already_added: false,
          },
        ],
      },
    },
    sessions: [
      {
        id: "s1",
        workspace,
        status: "idle",
        title: "New task",
        updated_at: now(),
        target: null as string | null,
      },
    ],
    openrouter: {
      /** The saved key never leaves the fake either; only `info` is served. */
      key: null as null | string,
      info: null as null | Json,
      fetched_at: null as null | number,
    },
    jobs: [] as Json[],
    events: [] as Json[],
    cursor: 0,
    lastRoute: {} as Record<string, string>,
  };

  function vendorRow(key: string, v: Json, model?: Json) {
    return {
      id: model ? `cli:${key}:${model.id}` : `cli:${key}`,
      provider: `cli:${key}`,
      account: `account:${key}`,
      model: model ? model.id : "default",
      route: "vendor_cli",
      group: "subscriptions",
      name: `${v.product || v.label} · ${model ? model.label : "Default"}`,
      subtitle: "Cloud · subscription",
      inference: "cloud",
      availability: v.usage?.limit_reached ? "unavailable" : v.availability,
      availability_label: v.usage?.limit_reached
        ? "Plan limit reached"
        : v.availability_label,
      reason: v.usage?.limit_reached ? v.usage.label : v.detail,
      featured: true,
      vision: model ? Boolean(model.vision) : Boolean(v.accepts_images),
      tools: true,
      is_default: model ? Boolean(model.is_default) : true,
      usage: v.usage,
    };
  }
  const openrouterOffline = () => state.config.network?.mode === "offline";
  function openrouterStatus() {
    const or = state.openrouter;
    return {
      key_set: Boolean(or.key),
      key: or.key ? or.info : null,
      key_error: null,
      models: openrouterModels.length,
      tool_models: openrouterModels.filter((m) => m.tools).length,
      fetched_at: or.fetched_at,
      offline: openrouterOffline(),
      keys_url: "https://openrouter.ai/keys",
      activity_url: "https://openrouter.ai/activity",
    };
  }
  function openrouterRow(m: (typeof openrouterModels)[number]) {
    const price = (n: number) => `$${n.toFixed(2)}/M`;
    const free = m.input === 0 && m.output === 0;
    const offline = openrouterOffline();
    const ready = Boolean(state.openrouter.key) && !offline;
    return {
      id: `api:openrouter:${m.slug}`,
      provider: "openrouter",
      account: "",
      model: m.slug,
      route: "native",
      group: "api",
      name: m.name,
      subtitle: `OpenRouter · ${m.slug}`,
      inference: "cloud",
      availability: offline ? "unavailable" : ready ? "ready" : "sign_in",
      availability_label: offline
        ? "Unavailable"
        : ready
          ? "Ready"
          : "Add API key",
      reason: offline
        ? "Offline mode: OpenRouter is off"
        : ready
          ? ""
          : "Add an OpenRouter API key in Accounts",
      featured: false,
      vision: m.vision,
      tools: m.tools,
      is_default: false,
      usage: {
        ...unknownUsage,
        state: "api_key",
        label: free
          ? "API key · free"
          : `API key · ${price(m.input)} in · ${price(m.output)} out`,
        detail: [
          free
            ? "Free model: no charge per token"
            : `Input ${price(m.input)} tokens · output ${price(m.output)} tokens`,
          "Billed per token to your OpenRouter key",
        ],
        provider_usage_url: "https://openrouter.ai/activity",
      },
    };
  }
  function pickerTargets() {
    const rows: Json[] = [];
    for (const [key, v] of Object.entries(state.vendors) as [string, Json][]) {
      if (v.state === "ready" && v.models.length)
        for (const m of v.models) rows.push(vendorRow(key, v, m));
      else rows.push(vendorRow(key, v));
    }
    for (const m of openrouterModels) rows.push(openrouterRow(m));
    for (const m of state.local.models as Json[])
      rows.push({
        id: m.id,
        provider: "llamacpp",
        account: "this-computer",
        model: m.name,
        route: "local_llamacpp",
        group: "local",
        name: `${m.name} · This computer`,
        subtitle: "Runs on this computer · No subscription quota",
        inference: "local",
        availability: m.compatible ? "ready" : "setup_required",
        availability_label: m.compatible ? "Ready" : "Setup required",
        reason: m.reason,
        featured: true,
        vision: m.vision,
        tools: m.tools,
        is_default: false,
        usage: localUsage,
      });
    return rows;
  }

  /** GET /api/allowance, built the way native/core/src/allowance.rs does:
   * only reported figures. */
  function subscriptionRow(key: string, v: Json) {
    const usage = v.usage || {};
    const windows = (usage.windows || []).map((w: Json) => ({
      label: w.label,
      remaining_percent: w.remaining_percent,
      resets_at: w.resets_at,
    }));
    const lefts = windows
      .map((w: Json) => w.remaining_percent)
      .filter((n: unknown) => typeof n === "number") as number[];
    const left = lefts.length ? Math.min(...lefts) : null;
    let rowState = "unavailable";
    let headline = v.detail || "Unavailable";
    if (v.availability === "setup_required") {
      rowState = "not_installed";
      headline = "Not installed";
    } else if (v.availability === "sign_in") {
      rowState = "sign_in";
      headline = "Not signed in";
    } else if (usage.limit_reached || usage.state === "limit_reached") {
      rowState = "limit_reached";
      headline = "Plan limit reached";
    } else if (v.availability === "ready") {
      if (left != null) {
        rowState = left <= 10 ? "low" : "ok";
        headline = `${left.toFixed(0)}% left`;
      } else {
        rowState = "unknown";
        headline = "Usage not reported";
      }
    }
    return {
      id: `cli:${key}`,
      kind: "subscription",
      product: v.product || v.label,
      state: rowState,
      headline,
      remaining_percent: left,
      windows,
      plan: v.account?.plan ?? null,
      note: v.usage_note ?? null,
      last_checked: usage.last_refresh ?? null,
      usage_url: usage.provider_usage_url ?? null,
    };
  }
  function localFallback(): { id: string; name: string } | null {
    const ready = (state.local.models as Json[]).filter(
      (m) => m.availability === "ready",
    );
    const find = (id: string) => ready.find((m) => m.id === id);
    const hit =
      find(state.config.limits?.fallback_model || "") ||
      find(state.lastLocal) ||
      ready.find((m) => m.tools) ||
      ready[0];
    return hit ? { id: hit.id, name: hit.name } : null;
  }
  function allowance() {
    const rows: Json[] = [];
    for (const key of ["codex", "claude", "cursor", "antigravity", "grok"])
      if (state.vendors[key])
        rows.push(subscriptionRow(key, state.vendors[key]));
    const or = openrouterStatus();
    const info = or.key as Json | null;
    let orState = "ok";
    let orHeadline = "";
    let orPercent: number | null = null;
    if (or.offline) {
      orState = "offline";
      orHeadline = "Offline mode";
    } else if (!or.key_set) {
      orState = "no_key";
      orHeadline = "No API key";
    } else if (info?.limit && info.limit_remaining != null) {
      orPercent = Math.max(
        0,
        Math.min(100, (info.limit_remaining / info.limit) * 100),
      );
      orState =
        info.limit_remaining <= 0
          ? "limit_reached"
          : orPercent <= 10
            ? "low"
            : "ok";
      orHeadline = `$${info.limit_remaining.toFixed(2)} of $${info.limit.toFixed(2)} left`;
    } else orHeadline = `$${(info?.usage || 0).toFixed(2)} used · no limit set`;
    rows.push({
      id: "openrouter",
      kind: "api_key",
      product: "OpenRouter",
      state: orState,
      headline: orHeadline,
      remaining_percent: orPercent,
      used: info?.usage ?? null,
      limit: info?.limit ?? null,
      limit_remaining: info?.limit_remaining ?? null,
      usage_url: or.activity_url,
    });
    const ready = (state.local.models as Json[]).filter(
      (m) => m.availability === "ready",
    ).length;
    rows.push({
      id: "local",
      kind: "local",
      product: "On this computer",
      state: ready ? "ok" : "none",
      headline: ready
        ? `No quota · ${ready} model${ready === 1 ? "" : "s"} ready`
        : "No local model ready",
      remaining_percent: null,
      ready_models: ready,
      on_limit: state.config.limits?.on_limit || "local",
      fallback: localFallback(),
    });
    return { generated_at: now(), rows };
  }

  /** One status check of a running install: the next progress step. */
  function agentStep() {
    const v = state.vendors.antigravity;
    const next = state.agent.hold ? undefined : state.agent.steps.shift();
    if (next === "installed") {
      Object.assign(v, {
        state: "not_logged_in",
        status: "warn",
        availability: "sign_in",
        availability_label: "Sign in",
        detail: "Not signed in",
        fix: "Connect to sign in with your Google account.",
        version: "1.2.1",
        install: agentInstall({
          installed: true,
          managed: true,
          path: "/home/user/.local/share/shadowcode/antigravity-acp/1.2.1/agy_acp_server.par",
          state: "installed",
          done: agentBytes,
          total: agentBytes,
        }),
      });
    } else if (next) v.install = next;
    return v.install;
  }

  function emit(
    sessionId: string,
    taskId: string,
    type: string,
    payload: Json,
  ) {
    state.cursor += 1;
    state.events.push({
      id: state.cursor,
      ts: now(),
      type,
      payload,
      session_id: sessionId,
      task_id: taskId,
    });
    wake();
  }

  function script(job: Json, local: boolean) {
    const sid = job.session_id;
    const tid = job.task_id;
    const steps: [string, Json][] = [
      ["agent.started", { task: job.task, job_id: job.id }],
      ...(job.handoff
        ? [["agent.handoff", job.handoff] as [string, Json]]
        : []),
      [
        "routing.selected",
        {
          purpose: "coder",
          source: "explicit",
          requested: job.model,
          model_id: job.model,
          model_name: job.model_name,
          provider: local ? "llamacpp" : job.provider,
          context_limit: local ? 16384 : 0,
          inference: local ? "local" : "cloud",
        },
      ],
      [
        "tool.started",
        { tool: "read_file", call_id: "c1", arguments: { path: "src/app.ts" } },
      ],
      [
        "tool.completed",
        {
          tool: "read_file",
          call_id: "c1",
          success: true,
          output_preview: "export const add = (a, b) => a - b;",
          arguments: { path: "src/app.ts" },
        },
      ],
      [
        "tool.started",
        { tool: "edit_file", call_id: "c2", arguments: { path: "src/app.ts" } },
      ],
      [
        "tool.completed",
        {
          tool: "edit_file",
          call_id: "c2",
          success: true,
          output: { paths: ["src/app.ts"] },
          output_preview: "Edited src/app.ts",
          arguments: { path: "src/app.ts" },
        },
      ],
      [
        "checkpoint.updated",
        {
          task_id: tid,
          workspace,
          changes: 1,
          paths: ["src/app.ts"],
          restored: false,
        },
      ],
      [
        "tool.started",
        { tool: "exec", call_id: "c3", arguments: { command: "npm test" } },
      ],
      [
        "tool.completed",
        {
          tool: "exec",
          call_id: "c3",
          success: true,
          output_preview: "Tests  4 passed (4)",
          output: { exit_code: 0, stdout: "Tests  4 passed (4)" },
        },
      ],
      [
        "verification.summary",
        {
          status: "last_command_succeeded",
          commands: [{ command: "npm test", success: true, exit_code: 0 }],
        },
      ],
      [
        "model.delta",
        {
          text: "Fixed `add` in src/app.ts and ran the tests.",
          message_id: "m1",
        },
      ],
    ];
    let index = 0;
    const tick = () => {
      if (job.status === "cancelling") {
        job.status = "cancelled";
        emit(sid, tid, "agent.completed", {
          summary: "Task cancelled",
          success: false,
          cancelled: true,
        });
        job.event_cursor = state.cursor;
        return;
      }
      if (index < steps.length) {
        if (index === 0) job.status = "running";
        const [type, payload] = steps[index++];
        emit(sid, tid, type, payload);
        job.event_cursor = state.cursor;
        setTimeout(tick, step);
        return;
      }
      const verification = {
        status: "last_command_succeeded",
        commands: [{ command: "npm test", success: true, exit_code: 0 }],
      };
      job.status = "completed";
      job.finished_at = now();
      job.summary = "Fixed `add` in src/app.ts and ran the tests.";
      job.result = { success: true, summary: job.summary, verification };
      emit(sid, tid, "agent.completed", {
        summary: job.summary,
        success: true,
        cancelled: false,
        verification,
        usage: {},
      });
      job.event_cursor = state.cursor;
    };
    setTimeout(tick, step);
  }

  /** A Codex turn that stops at the plan limit, then what the engine does
   * about it (engine.rs continue_after_limit): continue on a local model in
   * the same conversation, or record the stop for the user to decide. */
  function limitScript(job: Json) {
    const sid = job.session_id;
    const tid = job.task_id;
    const detail = "You've hit your usage limit. Try again in 3 hours";
    const spent = {
      ...codexUsage,
      state: "limit_reached",
      label: "Plan limit reached · resets in 3h",
      detail: ["Plan limit reached · resets in 3h"],
      windows: codexUsage.windows.map((w) => ({
        ...w,
        used_percent: 100,
        remaining_percent: 0,
      })),
      remaining_percent: 0,
      limit_reached: true,
      last_refresh: now(),
    };
    const steps: [string, Json][] = [
      ["agent.started", { task: job.task, job_id: job.id }],
      [
        "routing.selected",
        {
          purpose: "coder",
          source: "explicit",
          requested: job.model,
          model_id: job.model,
          model_name: job.model_name,
          provider: job.provider,
          context_limit: 0,
          inference: "cloud",
        },
      ],
      [
        "tool.started",
        { tool: "read_file", call_id: "c1", arguments: { path: "src/app.ts" } },
      ],
      [
        "tool.completed",
        {
          tool: "read_file",
          call_id: "c1",
          success: true,
          output_preview: "export const add = (a, b) => a - b;",
          arguments: { path: "src/app.ts" },
        },
      ],
      [
        "limit.reached",
        { vendor: "codex", usage: spent, detail, job_id: job.id },
      ],
    ];
    let index = 0;
    const tick = () => {
      if (index < steps.length) {
        if (index === 0) job.status = "running";
        const [type, payload] = steps[index++];
        emit(sid, tid, type, payload);
        job.event_cursor = state.cursor;
        setTimeout(tick, step);
        return;
      }
      state.vendors.codex.usage = spent;
      const verification = {
        status: "vendor_owned",
        commands: [],
        note: "Codex ran and judged its own checks; ShadowCode did not verify them.",
      };
      job.status = "limit_reached";
      job.finished_at = now();
      job.summary = `Codex plan limit reached: ${detail}. ShadowCode never retries on the same plan or buys more usage.`;
      job.result = {
        success: false,
        cancelled: false,
        summary: job.summary,
        verification,
        limit_reached: { vendor: "codex", detail, usage: spent },
      };
      emit(sid, tid, "agent.completed", { ...job.result, usage: {} });
      job.event_cursor = state.cursor;
      // The engine decides after the job has ended.
      setTimeout(() => {
        if ((state.config.limits?.on_limit || "local") !== "local") {
          emit(sid, tid, "limit.fallback", { ok: false, ask: true });
          return;
        }
        const fallback = localFallback();
        if (!fallback) {
          emit(sid, tid, "limit.fallback", {
            ok: false,
            from: "Codex",
            reason:
              "No local model is ready. Add one in Settings › Local models to keep going when a plan runs out.",
          });
          return;
        }
        const next = createJob({
          task: `Continue where Codex stopped when its plan limit was reached. The request was:\n\n${job.task}`,
          session_id: sid,
          model: fallback.id,
          handoff: {
            from: "cli:codex",
            to: "local:llamacpp",
            excerpt_chars: 1800,
            delivery: "message_tape",
            job_id: "",
          },
        });
        emit(sid, tid, "limit.fallback", {
          ok: true,
          from: "Codex",
          to: fallback.name,
          target: fallback.id,
          job_id: next.id,
        });
      }, step);
    };
    setTimeout(tick, step);
  }

  /** Queue a job and its user.message; `handoff` marks a provider change. */
  function createJob(body: Json) {
    const target = pickerTargets().find((t) => t.id === body.model);
    if (!target) throw new Error("Choose a model in the composer");
    const sid = body.session_id || "s1";
    const cloud = target.inference === "cloud";
    state.lastRoute[sid] = cloud ? "cloud" : "local";
    if (!cloud) state.lastLocal = target.id;
    const id = `j${state.jobs.length + 1}`;
    const job: Json = {
      id,
      task_id: `t${state.jobs.length + 1}`,
      workspace,
      session_id: sid,
      status: "queued",
      task: body.task,
      model: body.model,
      model_name: target.name,
      provider: target.provider,
      started_at: now(),
      event_cursor: state.cursor,
      web: Boolean(body.web),
      handoff: body.handoff ? { ...body.handoff, job_id: id } : undefined,
    };
    state.jobs.push(job);
    emit(sid, job.task_id, "user.message", { text: body.task });
    job.event_cursor = state.cursor - 1;
    const session = state.sessions.find((s: Json) => s.id === sid);
    if (session) session.target = body.model;
    if (target.provider === "cli:codex" && state.limitOnCodex) limitScript(job);
    else script(job, !cloud);
    return job;
  }

  function sessionDetail(id: string) {
    const session = state.sessions.find((s: Json) => s.id === id);
    if (!session) throw new Error(`Unknown session ${id}`);
    return {
      ...session,
      tasks: [],
      events: state.events.filter((e: Json) => e.session_id === id),
      event_cursor: state.cursor,
      execution_target: session.target,
      native_sessions: {},
    };
  }

  function route(method: string, fullPath: string, body: any): unknown {
    const [path, query = ""] = fullPath.split("?");
    const q = new URLSearchParams(query);
    let m: RegExpMatchArray | null;
    if (method === "GET" && path === "/api/health")
      return {
        ok: true,
        version: "0.28.0-test",
        workspace,
        trusted: true,
        permissions: state.config.permissions,
        tools: { git: { ok: true, detail: "git 2.45" } },
      };
    if (path === "/api/onboarding") {
      if (method === "POST") {
        state.onboarded = true;
        return { ok: true, workspace, session_id: "s1" };
      }
      return { completed: state.onboarded, suggested_workspace: workspace };
    }
    if (path === "/api/config") {
      if (method === "PUT") {
        const values = body.values || {};
        for (const [k, v] of Object.entries(values))
          state.config[k] = { ...(state.config[k] || {}), ...(v as Json) };
      }
      return state.config;
    }
    if (path === "/api/workspace/status")
      return {
        workspace,
        model: state.config.model,
        permissions: state.config.permissions,
        trusted: true,
      };
    if (path === "/api/picker")
      return {
        targets: pickerTargets(),
        vendors: state.vendors,
        local_engine: state.local,
        generated_at: now(),
      };
    if (path === "/api/accounts")
      return { vendors: state.vendors, config: {}, local_engine: state.local };
    if (path === "/api/allowance") {
      if (q.get("refresh") === "1")
        for (const v of Object.values(state.vendors) as Json[])
          if (v.usage?.last_refresh) v.usage.last_refresh = now();
      return allowance();
    }
    if (path === "/api/accounts/antigravity/install" && method === "GET")
      return agentStep();
    if (path === "/api/accounts/antigravity/install" && method === "POST") {
      if (state.config.network?.mode === "offline")
        throw new Error(
          JSON.stringify({ error: "Offline mode: downloads are off" }),
        );
      if (body?.confirm !== true)
        throw new Error(
          JSON.stringify({ error: "Confirm the 333 MB download first" }),
        );
      const v = state.vendors.antigravity;
      if (v.install.busy) return { ...v.install, started: false };
      const MB = 1_000_000;
      const progress = (done: number) =>
        agentInstall({
          state: "downloading",
          busy: true,
          done,
          total: agentBytes,
        });
      v.install = progress(0);
      state.agent.steps = [
        progress(120 * MB),
        progress(240 * MB),
        progress(agentBytes),
        ...(state.agent.installFails
          ? [
              agentInstall({
                state: "error",
                total: agentBytes,
                error:
                  "The download's SHA-256 does not match the published checksum",
              }),
            ]
          : [
              agentInstall({
                state: "verifying",
                busy: true,
                done: agentBytes,
                total: agentBytes,
              }),
              agentInstall({
                state: "unpacking",
                busy: true,
                done: agentBytes,
                total: agentBytes,
              }),
              "installed",
            ]),
      ];
      state.agent.installFails = false;
      return { ...v.install, started: true };
    }
    if (path === "/api/accounts/antigravity/uninstall" && method === "POST") {
      const v = state.vendors.antigravity;
      Object.assign(v, agentNotInstalled, { install: agentInstall() });
      return v.install;
    }
    if (
      (m = path.match(
        /^\/api\/accounts\/([a-z]+)\/(connect|login|refresh|disconnect|cancel-login)$/,
      ))
    ) {
      const [, vendor, action] = m;
      const v = state.vendors[vendor];
      if (!v) throw new Error(`Unknown vendor ${vendor}`);
      if (action === "connect") {
        if (v.availability === "setup_required")
          throw new Error(
            JSON.stringify({
              error: `${v.product || v.label} is not installed`,
            }),
          );
        state.login = { vendor, polls: 0 };
        return { ok: true, state: "started", note: "Opening the sign-in page" };
      }
      if (action === "login") {
        const login = state.login;
        if (!login || login.vendor !== vendor)
          return { running: false, lines: [], done: null };
        login.polls += 1;
        const google = vendor === "antigravity";
        // Antigravity's engine relays {vendor, line, url} records.
        const lines: (string | Json)[] = google
          ? [
              {
                vendor,
                line: "Sign in with Google: https://accounts.google.com/o/oauth2/v2/auth?client_id=agy-acp&response_type=code",
                url: "https://accounts.google.com/o/oauth2/v2/auth?client_id=agy-acp&response_type=code",
              },
              { vendor, line: "Waiting for the browser sign-in…", url: null },
            ]
          : [
              "Opening https://claude.ai/oauth/authorize?client=cli in your browser",
              "If the browser did not open, enter code WXYZ-1234 at the page above",
            ];
        if (login.polls >= 3) {
          Object.assign(
            v,
            google
              ? {
                  state: "ready",
                  status: "pass",
                  availability: "ready",
                  availability_label: "Ready",
                  detail: "Signed in with Google",
                  fix: null,
                  account: {
                    email: "dev@example.com",
                    plan: null,
                    auth_mode: "oauth-personal",
                  },
                  models: [
                    {
                      id: "gemini-3.5-pro",
                      label: "Gemini 3.5 Pro",
                      is_default: true,
                      vision: true,
                    },
                    {
                      id: "gemini-3.5-flash",
                      label: "Gemini 3.5 Flash",
                      is_default: false,
                      vision: true,
                    },
                  ],
                }
              : {
                  state: "ready",
                  availability: "ready",
                  availability_label: "Ready",
                  detail: "Signed in with Claude Max",
                  account: {
                    email: "dev@example.com",
                    plan: "Max",
                    auth_mode: "claude.ai",
                  },
                  models: [
                    {
                      id: "default",
                      label: "Default",
                      is_default: true,
                      vision: true,
                    },
                  ],
                },
          );
          return {
            running: false,
            lines,
            done: { ok: true, detail: "Signed in" },
          };
        }
        return {
          running: true,
          lines: lines.slice(0, login.polls),
          done: null,
        };
      }
      if (action === "refresh") return v;
      if (action === "cancel-login") {
        state.login = null;
        return { ok: true };
      }
      if (action === "disconnect") {
        if (body?.confirm !== true) throw new Error("confirm:true is required");
        Object.assign(v, {
          state: "not_logged_in",
          availability: "sign_in",
          availability_label: "Sign in",
          models: [],
          account: null,
        });
        return {
          ok: true,
          ran: v.logout_command,
          note: `Signed out of ${v.label}`,
        };
      }
    }
    if (path === "/api/openrouter") return openrouterStatus();
    if (path === "/api/openrouter/key" && method === "POST") {
      const key = String(body?.api_key ?? "").trim();
      if (!key) {
        state.openrouter.key = null;
        state.openrouter.info = null;
        return openrouterStatus();
      }
      if (openrouterOffline())
        throw new Error(
          JSON.stringify({ error: "Offline mode: OpenRouter is off" }),
        );
      if (key !== "sk-or-valid")
        // HTTP 400 over IPC: the error string carries the JSON body.
        throw new Error(
          JSON.stringify({ error: "OpenRouter rejected this key (401)" }),
        );
      state.openrouter.key = key;
      state.openrouter.info = {
        label: "sk-or-v1-a1b…9f2",
        usage: 1.2345,
        limit: 10,
        limit_remaining: 8.7655,
        is_free_tier: false,
      };
      state.openrouter.fetched_at = now();
      return openrouterStatus();
    }
    if (path === "/api/openrouter/refresh" && method === "POST") {
      state.openrouter.fetched_at = now();
      return openrouterStatus();
    }
    if (path === "/api/local-models") return state.local;
    if (path === "/api/local-models/load") {
      const model = state.local.models.find((x: Json) => x.id === body.id);
      if (!model?.compatible) throw new Error("This model cannot run here");
      state.local.loaded = {
        id: model.id,
        name: model.name,
        port: 41234,
        since: now(),
        context_tokens: model.context_tokens,
        backend: "Vulkan0",
      };
      return { ok: true, loaded: state.local.loaded };
    }
    if (path === "/api/local-models/unload") {
      state.local.loaded = null;
      return { ok: true };
    }
    if (path === "/api/local-models/import-ollama") {
      const found = state.local.ollama_store.models.find(
        (x: Json) => x.tag === body.tag,
      );
      found.already_added = true;
      state.local.models.push({
        ...state.local.models[0],
        id: "local:gguf:gemma",
        name: body.tag,
        architecture: "gemma4",
        vision: true,
        mmproj: found.projector,
      });
      return { ok: true, local_engine: state.local };
    }
    if (path === "/api/local-models/remove") {
      state.local.models = state.local.models.filter(
        (x: Json) => x.path !== body.path,
      );
      return { ok: true, deleted_weights: false };
    }
    if (path === "/api/local-models/add")
      return { ok: true, local_engine: state.local };
    if (path === "/api/sessions" && method === "GET")
      return { sessions: state.sessions };
    if (path === "/api/sessions" && method === "POST") {
      const id = `s${state.sessions.length + 1}`;
      state.sessions.unshift({
        id,
        workspace,
        status: "idle",
        title: body.title || "New task",
        updated_at: now(),
        target: null,
      });
      return { id, workspace };
    }
    if ((m = path.match(/^\/api\/sessions\/([^/]+)\/target$/))) {
      const session = state.sessions.find((s: Json) => s.id === m![1]);
      session.target = body.target_id;
      return { ok: true };
    }
    if ((m = path.match(/^\/api\/sessions\/([^/]+)\/activate$/)))
      return sessionDetail(m[1]);
    if ((m = path.match(/^\/api\/sessions\/([^/]+)$/)))
      return sessionDetail(m[1]);
    if (path === "/api/projects")
      return {
        projects: [
          { id: "p1", path: workspace, name: "demo", last_opened: now() },
        ],
      };
    if (path === "/api/jobs/current") {
      const sid = q.get("session_id");
      const job = [...state.jobs]
        .reverse()
        .find((j: Json) => j.session_id === sid);
      return { job: job || null };
    }
    if (path === "/api/jobs" && method === "GET") return { jobs: state.jobs };
    if (path === "/api/jobs" && method === "POST") {
      const target = pickerTargets().find((t) => t.id === body.model);
      if (!target) throw new Error("Choose a model in the composer");
      const sid = body.session_id || "s1";
      const previous = state.lastRoute[sid];
      const cloud = target.inference === "cloud";
      if (cloud && previous === "local" && !body.handoff_consent)
        // The IPC transport has no HTTP status: the answer arrives as an error
        // string carrying the JSON body.
        throw new Error(
          JSON.stringify({
            error:
              "Consent required before sending local content to a cloud provider",
            needs_consent: true,
            handoff: {
              from: "local:llamacpp",
              to: target.provider,
              excerpt_chars: 2400,
              images: 0,
            },
          }),
        );
      return { ...createJob(body) };
    }
    if ((m = path.match(/^\/api\/jobs\/([^/]+)\/events$/))) {
      const job = state.jobs.find((j: Json) => j.id === m![1]);
      const after = Number(q.get("after") || 0);
      // As in the engine, a finished job's page ends at its own completion.
      const through = job.finished_at ? job.event_cursor : Infinity;
      return {
        events: state.events.filter(
          (e: Json) =>
            e.id > after && e.id <= through && e.session_id === job.session_id,
        ),
        job: { ...job },
      };
    }
    if ((m = path.match(/^\/api\/jobs\/([^/]+)\/cancel$/))) {
      const job = state.jobs.find((j: Json) => j.id === m![1]);
      if (["queued", "running"].includes(job.status)) job.status = "cancelling";
      return { ...job };
    }
    if ((m = path.match(/^\/api\/jobs\/([^/]+)$/)))
      return { ...state.jobs.find((j: Json) => j.id === m![1]) };
    if (path === "/api/approvals") return { approvals: [] };
    if (path === "/api/commands") return { commands: [] };
    if (path === "/api/workspace/git")
      return {
        repo: true,
        status: "## main",
        log: "",
        diff: "",
        files: state.jobs.some((j: Json) => j.status === "completed")
          ? [{ path: "src/app.ts", label: "M" }]
          : [],
      };
    if (path === "/api/workspace/diff") {
      const hunks = [
        {
          header: "@@ -1,1 +1,1 @@",
          lines: [
            { kind: "del", text: "export const add = (a, b) => a - b;" },
            { kind: "add", text: "export const add = (a, b) => a + b;" },
          ],
        },
      ];
      return {
        diff: "",
        staged: "",
        hunks: q.get("path") || state.jobs.length ? hunks : [],
        staged_hunks: [],
        untracked: false,
        binary: false,
        truncated: false,
      };
    }
    if (path === "/api/workspace/attach-image")
      return {
        path: `.shadow/attachments/${body.filename}`,
        mime: "image/png",
        bytes: 68,
        kind: "image",
      };
    if (path === "/api/workspace/attach")
      return { path: `.shadow/attachments/${body.filename}`, kind: "text" };
    if (path === "/api/doctor")
      return { ok: true, version: "0.28.0-test", checks: [], suggestions: [] };
    throw new Error(`Fake backend has no route for ${method} ${path}`);
  }

  const bridge = {
    async request(path: string, method: string, body: unknown) {
      log.push({ method, path, body });
      return JSON.parse(JSON.stringify(route(method, path, body)));
    },
    async invoke(command: string, args?: Record<string, unknown>) {
      log.push({ method: "INVOKE", path: command, body: args });
      if (command === "open_external") return null;
      if (command === "pick_local_model" || command === "pick_directory")
        return null;
      if (command === "export_session") return null;
      return null;
    },
    async listen(event: string, handler: (payload: unknown) => void) {
      (listeners[event] ||= []).push(handler);
      return () => {
        listeners[event] = (listeners[event] || []).filter(
          (fn) => fn !== handler,
        );
      };
    },
  };
  (window as any).__SHADOW_TEST_TRANSPORT__ = bridge;
  (window as any).__SHADOW_FAKE__ = { log, state };
  return { bridge, log, state };
}
