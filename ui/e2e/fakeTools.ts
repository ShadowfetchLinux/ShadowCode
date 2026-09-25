/**
 * Fake terminals and Git panel routes for the Playwright suite. Install it
 * after `installFakeBackend`: it wraps that bridge, answers `/api/terminals…`
 * and `/api/git…`, and passes everything else through.
 *
 * Self-contained like fakeBackend.ts (Playwright serialises it).
 */
export type FakeToolsOptions = {
  /** `gh` is installed and signed in. */
  ghReady?: boolean;
};

export function installFakeTools(options: FakeToolsOptions = {}) {
  type Json = Record<string, any>;
  const inner = (window as any).__SHADOW_TEST_TRANSPORT__;
  if (!inner) throw new Error("installFakeBackend must run first");
  const fake = (window as any).__SHADOW_FAKE__;
  const log: { method: string; path: string; body: any }[] = fake.log;
  const listeners: ((payload: unknown) => void)[] = [];
  const encode = (text: string) =>
    btoa(String.fromCharCode(...new TextEncoder().encode(text)));

  // --- terminals: a tiny line-editing "shell" per tab ---------------------
  type Term = {
    id: string;
    number: number;
    output: string;
    line: string;
    cols: number;
    rows: number;
    exited: boolean;
  };
  const terminals: Term[] = [];
  let serial = 0;
  const prompt = "demo$ ";
  const wakeTerminal = (id: string) =>
    setTimeout(
      () =>
        listeners.forEach((fn) =>
          fn({ type: "terminal.output", terminal_id: id }),
        ),
      5,
    );
  const describe = (t: Term) => ({
    id: t.id,
    title: `Terminal ${t.number}`,
    number: t.number,
    workspace: "/work/demo",
    shell: "/bin/bash",
    cols: t.cols,
    rows: t.rows,
    created: t.number,
    exited: t.exited,
    exit_code: t.exited ? 0 : null,
    cursor: t.output.length,
  });
  const run = (t: Term, command: string) => {
    const [name, ...rest] = command.trim().split(/\s+/);
    if (!name) return "";
    if (name === "echo") return `${rest.join(" ")}\r\n`;
    if (name === "pwd") return "/work/demo\r\n";
    if (name === "exit") {
      t.exited = true;
      return "logout\r\n";
    }
    return `${name}: ran in /work/demo\r\n`;
  };
  const type = (t: Term, data: string) => {
    for (const ch of data) {
      if (ch === "\r") {
        const out = run(t, t.line);
        t.line = "";
        t.output += `\r\n${out}${t.exited ? "" : prompt}`;
      } else if (ch === "\x7f") {
        if (t.line) {
          t.line = t.line.slice(0, -1);
          t.output += "\b \b";
        }
      } else {
        t.line += ch;
        t.output += ch;
      }
    }
    wakeTerminal(t.id);
  };

  // --- git ----------------------------------------------------------------
  const git: Json = {
    branch: "main",
    branches: ["main", "develop"],
    upstream: { main: "origin/main", develop: "origin/develop" } as Json,
    staged: 0,
    changed: 1,
    ahead: 0,
    commits: [] as string[],
    prs: [] as Json[],
  };
  const overview = () => ({
    repo: true,
    branch: git.branch,
    detached: false,
    has_commits: true,
    upstream: git.upstream[git.branch] || null,
    ahead: git.ahead,
    behind: 0,
    staged: git.staged,
    changed: git.changed,
    branches: git.branches.map((name: string) => ({
      name,
      upstream: git.upstream[name] || "",
      current: name === git.branch,
    })),
    remotes: [{ name: "origin", info: remote }],
    remote: "origin",
    remote_info: remote,
    bases: ["main", "develop"],
    default_base: "main",
  });
  const remote = {
    host: "github.com",
    path: "octo/demo",
    web_url: "https://github.com/octo/demo",
    kind: "github",
  };
  const cli = () =>
    options.ghReady === false
      ? {
          name: "gh",
          installed: true,
          authenticated: false,
          detail: "You are not logged into any GitHub hosts.",
          install_url: "https://cli.github.com",
          login_command: "gh auth login --hostname github.com",
        }
      : {
          name: "gh",
          installed: true,
          version: "gh version 2.99.0",
          authenticated: true,
          detail: "Logged in to github.com account octo (keyring)",
          install_url: "https://cli.github.com",
          login_command: "gh auth login --hostname github.com",
        };
  const fail = (message: string) => {
    throw new Error(message);
  };

  function route(method: string, fullPath: string, body: any): unknown {
    const [path, query = ""] = fullPath.split("?");
    const q = new URLSearchParams(query);
    let m: RegExpMatchArray | null;
    if (path === "/api/terminals" && method === "GET")
      return {
        workspace: "/work/demo",
        terminals: terminals.map(describe),
        limits: { open: 12, scrollback_bytes: 524288 },
      };
    if (path === "/api/terminals" && method === "POST") {
      const used = terminals.map((t) => t.number);
      let number = 1;
      while (used.includes(number)) number++;
      serial++;
      const t: Term = {
        id: serial.toString(16).padStart(32, "0"),
        number,
        output: `Welcome to the fake shell\r\n${prompt}`,
        line: "",
        cols: body?.cols || 80,
        rows: body?.rows || 24,
        exited: false,
      };
      terminals.push(t);
      return describe(t);
    }
    if ((m = path.match(/^\/api\/terminals\/([0-9a-f]{32})\/(\w+)$/))) {
      const t = terminals.find((x) => x.id === m![1]);
      if (!t) fail("This terminal was closed");
      const term = t as Term;
      switch (m[2]) {
        case "input":
          if (term.exited) fail("This terminal's shell has exited.");
          type(term, String(body?.data || ""));
          return { ok: true };
        case "resize":
          term.cols = body?.cols || term.cols;
          term.rows = body?.rows || term.rows;
          return { cols: term.cols, rows: term.rows };
        case "output": {
          const after = Number(q.get("after") || 0);
          const text = term.output.slice(after);
          return {
            id: term.id,
            data: encode(text),
            from: after,
            cursor: term.output.length,
            more: false,
            truncated: false,
            exited: term.exited,
            exit_code: term.exited ? 0 : null,
          };
        }
        case "close":
          terminals.splice(terminals.indexOf(term), 1);
          return { ok: true };
      }
    }
    // Drawer › Tools (moved from Settings › Advanced).
    if (path === "/api/goals" && method === "GET")
      return {
        goals: [
          {
            id: "g1",
            workspace: "/work/demo",
            instruction: "Ship the login page",
            title: "Ship the login page",
            status: "paused",
            progress: 0,
            progress_pct: 0,
            running: false,
            session_id: "s1",
            milestones: [],
            updated_at: 1,
          },
        ],
      };
    if (path === "/api/background" && method === "GET") return { tasks: [] };
    if (path === "/api/worktrees" && method === "GET")
      return { workspace: "/work/demo", worktrees: [] };
    if (path === "/api/git" && method === "GET") return overview();
    if (path === "/api/git/branch" && method === "POST") {
      const name = String(body?.name || "");
      if (/\s/.test(name) || !name) fail("That is not a valid branch name");
      if (body.create) {
        if (git.branches.includes(name))
          fail(`a branch named '${name}' already exists`);
        git.branches.push(name);
      } else if (!git.branches.includes(name)) fail("No such branch");
      git.branch = name;
      return { ok: true, branch: name, created: Boolean(body.create) };
    }
    if (path === "/api/workspace/git/add" && method === "POST") {
      git.staged = git.changed;
      return { ok: true };
    }
    if (path === "/api/workspace/git/commit" && method === "POST") {
      if (!git.staged) fail("nothing to commit");
      git.commits.push(String(body.message));
      git.staged = 0;
      git.changed = 0;
      git.ahead += 1;
      return { ok: true };
    }
    if (path === "/api/git/suggest" && method === "POST") {
      if (body?.kind === "pr")
        return {
          kind: "pr",
          source: "local",
          model: "qwen3:14b",
          note: "",
          title: "Fix the add function",
          body: "Adds numbers instead of subtracting them.\n\n## Changes\n- Fix add in src/math.js",
        };
      if (!git.staged)
        fail("Nothing is staged yet. Stage the changes to commit first.");
      return {
        kind: "commit",
        source: "local",
        model: "qwen3:14b",
        note: "",
        message: "Fix the add function\n\nIt subtracted its arguments.",
      };
    }
    if (path === "/api/git/push" && method === "POST") {
      git.upstream[git.branch] = `origin/${git.branch}`;
      git.ahead = 0;
      return {
        ok: true,
        remote: "origin",
        branch: git.branch,
        output: "",
        remote_info: remote,
      };
    }
    if (path === "/api/git/pr" && method === "GET") {
      const pr = git.prs.find((p: Json) => p.head === git.branch) || null;
      return {
        remote: "origin",
        provider: "github",
        remote_info: remote,
        cli: cli(),
        base: q.get("base") || "main",
        compare_url: `https://github.com/octo/demo/compare/${q.get("base") || "main"}...${git.branch}?expand=1`,
        pr,
      };
    }
    if (path === "/api/git/pr" && method === "POST") {
      if (options.ghReady === false) fail("gh is not ready.");
      const pushed = !git.upstream[git.branch] || git.ahead > 0;
      if (pushed) {
        git.upstream[git.branch] = `origin/${git.branch}`;
        git.ahead = 0;
      }
      const number = 7 + git.prs.length;
      const pr = {
        number,
        url: `https://github.com/octo/demo/pull/${number}`,
        state: "OPEN",
        draft: Boolean(body.draft),
        title: body.title,
        base: body.base,
        head: git.branch,
      };
      git.prs.push(pr);
      return {
        ok: true,
        url: pr.url,
        number,
        provider: "github",
        pushed,
        branch: git.branch,
        base: body.base,
        draft: pr.draft,
      };
    }
    if (path === "/api/git/pr/checks" && method === "GET")
      return {
        supported: true,
        checks: [
          {
            name: "build",
            workflow: "CI",
            state: "SUCCESS",
            bucket: "pass",
            link: "https://github.com/octo/demo/actions/runs/1",
          },
          {
            name: "test",
            workflow: "CI",
            state: "IN_PROGRESS",
            bucket: "pending",
            link: "https://github.com/octo/demo/actions/runs/2",
          },
        ],
        summary: { pass: 1, pending: 1 },
        overall: "pending",
        url: `https://github.com/octo/demo/pull/${q.get("number")}/checks`,
        checked_at: Date.now() / 1000,
      };
    return undefined;
  }

  const bridge = {
    ...inner,
    async request(path: string, method: string, body: unknown) {
      const answer = route(method, path, body);
      if (answer === undefined) return inner.request(path, method, body);
      log.push({ method, path, body });
      return JSON.parse(JSON.stringify(answer));
    },
    async listen(event: string, handler: (payload: unknown) => void) {
      if (event !== "shadowcode:terminal") return inner.listen(event, handler);
      listeners.push(handler);
      return () => {
        const at = listeners.indexOf(handler);
        if (at >= 0) listeners.splice(at, 1);
      };
    },
  };
  (window as any).__SHADOW_TEST_TRANSPORT__ = bridge;
  fake.tools = { terminals, git };
}
