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
    detail: ["ChatGPT Pro"],
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
  const localUsage = {
    ...unknownUsage,
    state: "local",
    label: "Runs on this computer · No subscription quota",
  };

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
        account: { email: "dev@example.com", plan: "Pro", auth_mode: "chatgpt" },
        models: [
          { id: "gpt-6-astra", label: "GPT-6-Astra", is_default: true, vision: true },
          { id: "gpt-6-luna", label: "GPT-6-Luna", is_default: false, vision: true },
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
    },
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
      name: `${v.label} · ${model ? model.label : "Default"}`,
      subtitle: "Cloud · subscription",
      inference: "cloud",
      availability: v.availability,
      availability_label: v.availability_label,
      reason: v.detail,
      featured: true,
      vision: model ? Boolean(model.vision) : Boolean(v.accepts_images),
      tools: true,
      is_default: model ? Boolean(model.is_default) : true,
      usage: v.usage,
    };
  }
  function pickerTargets() {
    const rows: Json[] = [];
    for (const [key, v] of Object.entries(state.vendors) as [string, Json][]) {
      if (v.state === "ready" && v.models.length)
        for (const m of v.models) rows.push(vendorRow(key, v, m));
      else rows.push(vendorRow(key, v));
    }
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

  function emit(sessionId: string, taskId: string, type: string, payload: Json) {
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
      ["agent.started", { task: job.task }],
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
      ["tool.started", { tool: "read_file", call_id: "c1", arguments: { path: "src/app.ts" } }],
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
        { task_id: tid, workspace, changes: 1, paths: ["src/app.ts"], restored: false },
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
        { text: "Fixed `add` in src/app.ts and ran the tests.", message_id: "m1" },
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
    if ((m = path.match(/^\/api\/accounts\/([a-z]+)\/(connect|login|refresh|disconnect|cancel-login)$/))) {
      const [, vendor, action] = m;
      const v = state.vendors[vendor];
      if (!v) throw new Error(`Unknown vendor ${vendor}`);
      if (action === "connect") {
        state.login = { vendor, polls: 0 };
        return { ok: true, state: "started", note: "Opening the sign-in page" };
      }
      if (action === "login") {
        const login = state.login;
        if (!login || login.vendor !== vendor) return { running: false, lines: [], done: null };
        login.polls += 1;
        const lines = [
          "Opening https://claude.ai/oauth/authorize?client=cli in your browser",
          "If the browser did not open, enter code WXYZ-1234 at the page above",
        ];
        if (login.polls >= 3) {
          Object.assign(v, {
            state: "ready",
            availability: "ready",
            availability_label: "Ready",
            detail: "Signed in with Claude Max",
            account: { email: "dev@example.com", plan: "Max", auth_mode: "claude.ai" },
            models: [{ id: "default", label: "Default", is_default: true, vision: true }],
          });
          return { running: false, lines, done: { ok: true, detail: "Signed in" } };
        }
        return { running: true, lines: lines.slice(0, login.polls), done: null };
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
        });
        return { ok: true, ran: v.logout_command, note: `Signed out of ${v.label}` };
      }
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
      const found = state.local.ollama_store.models.find((x: Json) => x.tag === body.tag);
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
      state.local.models = state.local.models.filter((x: Json) => x.path !== body.path);
      return { ok: true, deleted_weights: false };
    }
    if (path === "/api/local-models/add") return { ok: true, local_engine: state.local };
    if (path === "/api/sessions" && method === "GET") return { sessions: state.sessions };
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
    if ((m = path.match(/^\/api\/sessions\/([^/]+)\/activate$/))) return sessionDetail(m[1]);
    if ((m = path.match(/^\/api\/sessions\/([^/]+)$/))) return sessionDetail(m[1]);
    if (path === "/api/projects")
      return { projects: [{ id: "p1", path: workspace, name: "demo", last_opened: now() }] };
    if (path === "/api/jobs/current") {
      const sid = q.get("session_id");
      const job = [...state.jobs].reverse().find((j: Json) => j.session_id === sid);
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
            error: "Consent required before sending local content to a cloud provider",
            needs_consent: true,
            handoff: { from: "local:llamacpp", to: target.provider, excerpt_chars: 2400, images: 0 },
          }),
        );
      state.lastRoute[sid] = cloud ? "cloud" : "local";
      const id = `j${state.jobs.length + 1}`;
      const job = {
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
      };
      state.jobs.push(job);
      emit(sid, job.task_id, "user.message", { text: body.task });
      job.event_cursor = state.cursor - 1;
      const session = state.sessions.find((s: Json) => s.id === sid);
      if (session) session.target = body.model;
      script(job, !cloud);
      return { ...job };
    }
    if ((m = path.match(/^\/api\/jobs\/([^/]+)\/events$/))) {
      const job = state.jobs.find((j: Json) => j.id === m![1]);
      const after = Number(q.get("after") || 0);
      return {
        events: state.events.filter(
          (e: Json) => e.id > after && e.session_id === job.session_id,
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
      return { path: `.shadow/attachments/${body.filename}`, mime: "image/png", bytes: 68, kind: "image" };
    if (path === "/api/workspace/attach")
      return { path: `.shadow/attachments/${body.filename}`, kind: "text" };
    if (path === "/api/doctor") return { ok: true, version: "0.28.0-test", checks: [], suggestions: [] };
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
      if (command === "pick_local_model" || command === "pick_directory") return null;
      if (command === "export_session") return null;
      return null;
    },
    async listen(event: string, handler: (payload: unknown) => void) {
      (listeners[event] ||= []).push(handler);
      return () => {
        listeners[event] = (listeners[event] || []).filter((fn) => fn !== handler);
      };
    },
  };
  (window as any).__SHADOW_TEST_TRANSPORT__ = bridge;
  (window as any).__SHADOW_FAKE__ = { log, state };
  return { bridge, log, state };
}
