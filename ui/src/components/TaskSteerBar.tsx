import { useState } from "react";
import { Pause, Play, RotateCcw, CornerDownLeft } from "lucide-react";
import { api, type Job } from "../api";

/** Compact live-steering strip: Pause / Steer / Resume / Rewind. */
export function TaskSteerBar({
  job,
  onToast,
}: {
  job: Job;
  onToast: (text: string, kind?: "ok" | "err" | "info") => void;
}) {
  const [steerOpen, setSteerOpen] = useState(false);
  const [instruction, setInstruction] = useState("");
  const [editPath, setEditPath] = useState("");
  const [busy, setBusy] = useState(false);
  const paused = job.status === "paused";

  async function run(label: string, action: () => Promise<unknown>) {
    setBusy(true);
    try {
      await action();
      onToast(label, "ok");
    } catch (error) {
      onToast(String(error), "err");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="steer-bar" role="group" aria-label="Live steering">
      {!paused ? (
        <button
          type="button"
          className="mini"
          disabled={busy || job.status === "cancelling"}
          title="Pause at the next safe boundary; a running command finishes first"
          onClick={() =>
            void run("Pause requested", () => api.pauseJob(job.id))
          }
        >
          <Pause size={12} />
          Pause task
        </button>
      ) : (
        <button
          type="button"
          className="mini"
          disabled={busy}
          title="Resume with any steering note"
          onClick={() => void run("Resumed", () => api.resumeJob(job.id))}
        >
          <Play size={12} />
          Resume task
        </button>
      )}
      <button
        type="button"
        className="mini"
        disabled={busy || (!paused && job.status !== "running")}
        title={
          paused
            ? "Add a steering instruction for the next model turn"
            : "Pause at the next safe boundary and add a steering instruction"
        }
        onClick={() => setSteerOpen((v) => !v)}
      >
        <CornerDownLeft size={12} />
        Steer
      </button>
      <button
        type="button"
        className="mini"
        disabled={busy || !paused}
        title="Restore checkpointed files after the current operation has reached the pause boundary"
        onClick={() => void run("Rewound files", () => api.rewindJob(job.id))}
      >
        <RotateCcw size={12} />
        Rewind files
      </button>
      {steerOpen && (
        <form
          className="steer-form"
          onSubmit={(e) => {
            e.preventDefault();
            if (!instruction.trim()) return;
            void run(
              "Steering saved. Resume the task to apply it.",
              async () => {
                if (!paused) await api.pauseJob(job.id);
                await api.steerJob(
                  job.id,
                  instruction.trim(),
                  editPath.trim() || undefined,
                );
                setInstruction("");
                setEditPath("");
                setSteerOpen(false);
              },
            );
          }}
        >
          <input
            value={instruction}
            onChange={(e) => setInstruction(e.target.value)}
            placeholder="e.g. do not refactor schema; use db/v2.sql"
            aria-label="Steering instruction"
            autoFocus
          />
          <input
            value={editPath}
            onChange={(e) => setEditPath(e.target.value)}
            placeholder="Changed file (optional)"
            aria-label="Manually edited path"
          />
          <button type="submit" className="mini" disabled={busy}>
            Save
          </button>
        </form>
      )}
    </div>
  );
}
