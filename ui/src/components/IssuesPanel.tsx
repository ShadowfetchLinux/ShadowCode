/** Tools › Issues: "Start from an issue". Lists the repository's open
 * issues through gh / glab (the user's own sign-in), pre-fills the composer
 * with the picked issue, suggests an issue branch, and remembers the issue
 * so a pull request that closes it can be offered when the task finishes. */
import { useCallback, useEffect, useState } from "react";
import { CircleDot, RefreshCw } from "lucide-react";
import { branchNameProblem, forgeApi } from "../lib/forge";
import {
  forgeName,
  issuesApi,
  linkOf,
  type IssueDetail,
  type IssueLink,
  type IssueList,
} from "../lib/issues";
import { Empty } from "./cards";
import "../tools.css";

type Toast = (text: string, kind?: "ok" | "err" | "info") => void;

export function IssuesPanel({
  busy,
  toast,
  onStartTask,
  onLinked,
  onOpenTerminal,
}: {
  busy: boolean;
  toast: Toast;
  /** Put the task in the composer for review. */
  onStartTask: (task: string) => void;
  onLinked: (link: IssueLink) => void;
  onOpenTerminal: () => void;
}) {
  const [list, setList] = useState<IssueList | null>(null);
  const [error, setError] = useState("");
  const [picked, setPicked] = useState<IssueDetail | null>(null);
  const [loading, setLoading] = useState(0);
  const [branch, setBranch] = useState("");
  const [makeBranch, setMakeBranch] = useState(true);
  const [working, setWorking] = useState(false);

  const load = useCallback(async () => {
    setError("");
    try {
      setList(await issuesApi.list());
    } catch (e) {
      setError(String(e));
      setList(null);
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  async function pick(number: number) {
    setLoading(number);
    try {
      const detail = await issuesApi.get(number);
      setPicked(detail);
      setBranch(detail.branch);
      setMakeBranch(true);
    } catch (e) {
      toast(String(e), "err");
    } finally {
      setLoading(0);
    }
  }

  async function start() {
    if (!picked || working) return;
    setWorking(true);
    try {
      const name = branch.trim();
      if (makeBranch && name) {
        try {
          await forgeApi.branch(name, true);
        } catch (e) {
          // The branch may exist from an earlier attempt: switch to it.
          if (!/already exists/i.test(String(e))) throw e;
          await forgeApi.branch(name, false);
        }
      }
      const link = linkOf(picked);
      onLinked({ ...link, branch: makeBranch ? name : "" });
      onStartTask(picked.task);
      toast(
        makeBranch && name
          ? `On ${name}. Review the task, then send it.`
          : "Review the task, then send it.",
        "ok",
      );
    } catch (e) {
      toast(String(e), "err");
    } finally {
      setWorking(false);
    }
  }

  if (error)
    return (
      <div className="issues">
        <p className="notice bad" role="alert">
          {error}
        </p>
        <button type="button" className="mini" onClick={() => void load()}>
          Try again
        </button>
      </div>
    );
  if (!list)
    return (
      <div className="skel-rows">
        <span className="skel" />
        <span className="skel short" />
      </div>
    );

  if (picked) {
    const issue = picked.issue;
    const problem = makeBranch && branch ? branchNameProblem(branch) : "";
    return (
      <section className="issue-pick" aria-label={`Issue #${issue.number}`}>
        <button
          type="button"
          className="link-btn"
          onClick={() => setPicked(null)}
        >
          ← All issues
        </button>
        <h4>
          #{issue.number} {issue.title}
        </h4>
        <p className="hint">
          Opened by {issue.author || "unknown"}
          {issue.labels.length ? ` · ${issue.labels.join(", ")}` : ""}
          {issue.comment_count
            ? ` · ${issue.comment_count} comment${issue.comment_count === 1 ? "" : "s"}${
                issue.comment_count > issue.comments.length
                  ? ` (newest ${issue.comments.length} included)`
                  : ""
              }`
            : ""}
        </p>
        {issue.body && (
          <pre className="issue-body">{issue.body.slice(0, 1200)}</pre>
        )}
        <label className="check">
          <input
            type="checkbox"
            checked={makeBranch}
            onChange={(e) => setMakeBranch(e.target.checked)}
          />{" "}
          Work on a new branch
        </label>
        {makeBranch && (
          <input
            aria-label="Branch name"
            value={branch}
            aria-invalid={Boolean(problem)}
            onChange={(e) => setBranch(e.target.value)}
          />
        )}
        {problem && <p className="hint error">{problem}</p>}
        {busy && makeBranch && (
          <p className="hint">
            A task is running; the branch can be made once it finishes.
          </p>
        )}
        <p className="hint">
          The task text goes into the message box so you can read and edit it
          before sending. When the task finishes you can open a pull request
          that closes #{issue.number}.
        </p>
        <div className="row end">
          <button
            type="button"
            className="mini primary-mini"
            disabled={
              working ||
              Boolean(problem) ||
              (makeBranch && (!branch.trim() || busy))
            }
            onClick={() => void start()}
          >
            {working ? "Preparing…" : `Start task from #${issue.number}`}
          </button>
        </div>
      </section>
    );
  }

  if (!list.ready) {
    const cli = list.cli;
    const name = cli?.name === "glab" ? "GitLab CLI (glab)" : "GitHub CLI (gh)";
    return (
      <div className="issues">
        {list.reason ? (
          <Empty title="Issues are not available here" body={list.reason} />
        ) : !cli?.installed ? (
          <p className="hint">
            To list {forgeName(list.provider)} issues here, install the {name}{" "}
            from <a href={cli?.install_url}>{cli?.install_url}</a>, then sign in
            with <code>{cli?.login_command}</code>.
          </p>
        ) : (
          <p className="hint">
            The {name} is installed but not signed in. Run{" "}
            <code>{cli.login_command}</code> in the Terminal, then come back
            here.{" "}
            <button type="button" className="link-btn" onClick={onOpenTerminal}>
              Open the Terminal
            </button>
          </p>
        )}
        <button type="button" className="mini" onClick={() => void load()}>
          Check again
        </button>
      </div>
    );
  }

  return (
    <div className="issues">
      <div className="row">
        <p className="hint grow">
          Open {forgeName(list.provider)} issues
          {list.remote_info ? ` in ${list.remote_info.path}` : ""}. Pick one to
          start a task from it.
        </p>
        <button
          type="button"
          className="icon-btn"
          aria-label="Refresh issues"
          onClick={() => void load()}
        >
          <RefreshCw size={13} aria-hidden="true" />
        </button>
      </div>
      {list.issues.length === 0 && (
        <Empty title="No open issues" body="Nothing to pick up right now." />
      )}
      <ul className="issue-list">
        {list.issues.map((issue) => (
          <li key={issue.number}>
            <button
              type="button"
              className="issue-row"
              disabled={Boolean(loading)}
              onClick={() => void pick(issue.number)}
            >
              <CircleDot size={13} aria-hidden="true" />
              <span className="grow">
                <strong>
                  #{issue.number} {issue.title}
                </strong>
                <span className="hint">
                  {issue.author ? `by ${issue.author}` : ""}
                  {issue.labels.length ? ` · ${issue.labels.join(", ")}` : ""}
                </span>
              </span>
              {loading === issue.number && (
                <span className="hint">Reading…</span>
              )}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
