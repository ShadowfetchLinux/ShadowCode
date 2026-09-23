import { Check, Circle, LoaderCircle } from "lucide-react";

export type TimelineStep = {
  id: string;
  label: string;
  state: "pending" | "active" | "done";
  detail?: string;
};

export function deriveTimeline(args: {
  busy: boolean;
  stage?: string;
  waitingApproval?: boolean;
  finished?: boolean;
  hasDiff?: boolean;
  testSummary?: string;
}): TimelineStep[] {
  const stage = (args.stage || "").toLowerCase();
  const reading = /understand|explor|read|search/.test(stage);
  const editing = /edit|patch|write|apply/.test(stage) || Boolean(args.hasDiff);
  const testing = /test|verify/.test(stage);
  const waiting = Boolean(args.waitingApproval);
  const finished = Boolean(args.finished) && !args.busy;
  const active = waiting
    ? "waiting"
    : testing
      ? "testing"
      : editing
        ? "editing"
        : reading || args.busy
          ? "reading"
          : finished
            ? "finished"
            : "reading";
  const order = ["reading", "editing", "testing", "waiting", "finished"] as const;
  const labels = {
    reading: "Reading project",
    editing: "Editing files",
    testing: "Running tests",
    waiting: "Waiting for approval",
    finished: "Finished",
  };
  const reached = order.indexOf(active);
  return order.map((id, i) => ({
    id,
    label: labels[id],
    state: finished && id === "finished"
      ? "done"
      : i < reached
        ? "done"
        : i === reached && args.busy
          ? "active"
          : i === reached && finished
            ? "done"
            : "pending",
    detail: id === "testing" ? args.testSummary : undefined,
  }));
}

export function ActivityTimeline({
  steps,
  output,
}: {
  steps: TimelineStep[];
  output?: string;
}) {
  return (
    <div className="activity-timeline" role="status" aria-label="Agent activity">
      {steps.map((step) => (
        <div key={step.id} className={`activity-step is-${step.state}`}>
          {step.state === "done" ? (
            <Check size={14} />
          ) : step.state === "active" ? (
            <LoaderCircle size={14} className="spin" />
          ) : (
            <Circle size={12} />
          )}
          <span>{step.label}</span>
          {step.detail ? <small>{step.detail}</small> : null}
        </div>
      ))}
      {output ? (
        <details className="activity-output">
          <summary>Show tool output</summary>
          <pre>{output}</pre>
        </details>
      ) : null}
    </div>
  );
}
