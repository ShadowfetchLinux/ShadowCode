import { useEffect, useMemo, useState } from "react";
import { api, type EventRow, type FileEntry, type ModelInfo, type Project } from "./api";

type CenterTab = "conversation" | "plan" | "tools" | "events";
type RightTab = "files" | "git" | "status";

export default function App() {
  const [ready, setReady] = useState(false);
  const [workspace, setWorkspace] = useState("");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [events, setEvents] = useState<EventRow[]>([]);
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [git, setGit] = useState({ status: "", log: "", diff: "" });
  const [status, setStatus] = useState({ workspace: "", model: { default: "mock", provider: "mock" }, permissions: { level: "workspace" } });
  const [center, setCenter] = useState<CenterTab>("conversation");
  const [right, setRight] = useState<RightTab>("files");
  const [task, setTask] = useState("analyze, find failing tests, fix, rerun, summarize");
  const [log, setLog] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [summary, setSummary] = useState("");
  const [planMd, setPlanMd] = useState("No plan yet.");
  const [fileView, setFileView] = useState("");
  const [dir, setDir] = useState(".");

  async function refresh() {
    try {
      const [health, modelData, projectData, eventData, fileData, gitData, statusData] = await Promise.all([
        api.health(),
        api.models(),
        api.projects(),
        api.events(),
        api.files(dir),
        api.git(),
        api.status(),
      ]);
      setWorkspace(health.workspace || statusData.workspace);
      setModels(modelData.models);
      setProjects(projectData.projects);
      setEvents(eventData.events);
      setFiles(fileData.entries);
      setGit(gitData);
      setStatus(statusData);
      setReady(true);
    } catch {
      setReady(false);
    }
  }

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 4000);
    return () => clearInterval(id);
  }, [dir]);

  const toolEvents = useMemo(() => events.filter((e) => e.type.startsWith("tool.") || e.type.startsWith("test.")), [events]);

  async function runTask() {
    if (!task.trim() || busy) return;
    setBusy(true);
    setLog((rows) => [...rows, `> ${task}`]);
    try {
      const result = await api.run(task, workspace || undefined);
      setSummary(result.summary);
      setPlanMd(
        result.plan.steps.map((s) => `${s.status === "done" ? "[x]" : s.status === "failed" ? "[!]" : "[ ]"} ${s.id} ${s.title}`).join("\n") || result.plan.goal,
      );
      setLog((rows) => [...rows, result.success ? "verified complete" : "stopped", result.summary]);
      await refresh();
    } catch (err) {
      setLog((rows) => [...rows, String(err)]);
    } finally {
      setBusy(false);
    }
  }

  async function openFile(path: string, type: string) {
    if (type === "dir") {
      setDir(path);
      return;
    }
    const file = await api.file(path);
    setFileView(file.content);
    setRight("files");
  }

  if (!ready && !workspace) {
    return (
      <div className="boot">
        <p>SHADOW AGENT</p>
      </div>
    );
  }

  return (
    <div className="app">
      <header className="top">
        <div className="brand">
          <span className="mark" aria-hidden />
          <div>
            <p className="kicker">SHADOWFETCH SUITE</p>
            <h1>Shadow Agent</h1>
          </div>
        </div>
        <div className="meta">
          <span>{workspace || "no workspace"}</span>
          <span>{status.model.provider}/{status.model.default}</span>
          <span>{status.permissions.level}</span>
        </div>
        <div className={`live ${busy ? "on" : ""}`}>
          <i />
          {busy ? "RUNNING" : "IDLE"}
        </div>
      </header>

      <aside className="left">
        <div className="panel-h">PROJECTS</div>
        <div className="scroll">
          {projects.length === 0 && <div className="item">No recent projects yet.</div>}
          {projects.map((p) => (
            <div key={p.id} className={`item ${p.path === workspace ? "active" : ""}`}>
              <strong>{p.name}</strong>
              <span>{p.path}</span>
            </div>
          ))}
        </div>
        <div className="panel-h">MODELS</div>
        <div className="scroll">
          {models.map((m) => (
            <div key={m.id} className={`item ${m.id === status.model.default ? "active" : ""}`}>
              <strong>{m.name}</strong>
              <span>{m.provider} {m.endpoint}</span>
            </div>
          ))}
        </div>
      </aside>

      <main className="center">
        <div className="panel-h">
          <span>WORKSPACE</span>
          <div className="tabs">
            {(["conversation", "plan", "tools", "events"] as CenterTab[]).map((tab) => (
              <button key={tab} className={center === tab ? "on" : ""} onClick={() => setCenter(tab)}>
                {tab}
              </button>
            ))}
          </div>
        </div>
        <div className="scroll">
          {center === "conversation" && (
            <>
              {summary && (
                <div className="msg">
                  <div className="who">AGENT</div>
                  <pre>{summary}</pre>
                </div>
              )}
              {log.slice(-12).map((line, i) => (
                <div key={i} className={`msg ${line.startsWith(">") ? "user" : ""}`}>
                  <div className="who">{line.startsWith(">") ? "YOU" : "HARNESS"}</div>
                  <pre>{line}</pre>
                </div>
              ))}
              {!summary && log.length === 0 && (
                <div className="msg">
                  <div className="who">HARNESS</div>
                  <pre>Same Agent API as the CLI. Describe a coding task. The loop will inspect, plan, use tools, and verify before it finishes.</pre>
                </div>
              )}
            </>
          )}
          {center === "plan" && <div className="plan">{planMd}</div>}
          {center === "tools" &&
            toolEvents.slice(-40).map((e, i) => (
              <div key={i} className={`event ${e.type.includes("fail") ? "bad" : "ok"}`}>
                {e.type} · {JSON.stringify(e.payload).slice(0, 180)}
              </div>
            ))}
          {center === "events" &&
            events.slice(-80).map((e, i) => (
              <div key={i} className="event">
                {new Date(e.ts * 1000).toLocaleTimeString()} {e.type}
              </div>
            ))}
        </div>
        <form
          className="composer"
          onSubmit={(ev) => {
            ev.preventDefault();
            runTask();
          }}
        >
          <textarea value={task} onChange={(ev) => setTask(ev.target.value)} placeholder="Task for the agent loop…" />
          <button type="submit" disabled={busy}>
            {busy ? "…" : "Run"}
          </button>
        </form>
      </main>

      <aside className="right">
        <div className="panel-h">
          <span>INSPECT</span>
          <div className="tabs">
            {(["files", "git", "status"] as RightTab[]).map((tab) => (
              <button key={tab} className={right === tab ? "on" : ""} onClick={() => setRight(tab)}>
                {tab}
              </button>
            ))}
          </div>
        </div>
        <div className="scroll">
          {right === "files" && (
            <>
              <div className="item" onClick={() => setDir(".")}>
                <strong>{dir}</strong>
              </div>
              {files.map((f) => (
                <div key={f.path} className="file" onClick={() => openFile(f.path, f.type)}>
                  <strong>{f.type === "dir" ? "▸" : "·"} {f.name}</strong>
                </div>
              ))}
              {fileView && <pre className="plan">{fileView.slice(0, 4000)}</pre>}
            </>
          )}
          {right === "git" && (
            <pre className="plan">{`${git.status}\n\n${git.log}\n\n${git.diff}`}</pre>
          )}
          {right === "status" && (
            <>
              <div className="status-row"><span>workspace</span><code>{status.workspace}</code></div>
              <div className="status-row"><span>model</span><code>{status.model.default}</code></div>
              <div className="status-row"><span>provider</span><code>{status.model.provider}</code></div>
              <div className="status-row"><span>permissions</span><code>{status.permissions.level}</code></div>
            </>
          )}
        </div>
      </aside>

      <footer className="term">
        <div className="panel-h">TERMINAL / EVENT STREAM</div>
        <pre>
          {events
            .slice(-18)
            .map((e) => `${e.type.padEnd(18)} ${JSON.stringify(e.payload).slice(0, 140)}`)
            .join("\n") || "waiting for agent events…"}
        </pre>
      </footer>
    </div>
  );
}
