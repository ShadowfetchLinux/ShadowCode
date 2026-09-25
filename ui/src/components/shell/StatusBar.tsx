import type { ReactNode } from "react";
import { GitBranch } from "lucide-react";
import type { AllowanceResponse } from "../../api";
import { AllowanceButton } from "../Allowance";

/** The bottom line: state, branch, allowance, context use and version. */
export function StatusBar({
  busy,
  paused,
  reconnecting,
  branch,
  onBranch,
  allowance,
  allowanceOpen,
  onAllowance,
  context,
  version,
}: {
  busy: boolean;
  paused: boolean;
  reconnecting: boolean;
  branch: string;
  onBranch: () => void;
  allowance: AllowanceResponse | null;
  allowanceOpen: boolean;
  onAllowance: () => void;
  /** The context & cost chip. */
  context?: ReactNode;
  version: string;
}) {
  return (
    <footer className="statusline" aria-live="polite">
      <span className={`status-dot ${busy ? "active" : ""}`} />
      <span className="status-label">
        {busy
          ? paused
            ? "Paused"
            : "Working"
          : reconnecting
            ? "Reconnecting"
            : "Ready"}
      </span>
      {branch && (
        <>
          <span className="sep" aria-hidden="true">
            ·
          </span>
          <button type="button" title="Review git changes" onClick={onBranch}>
            <GitBranch size={12} aria-hidden="true" />
            {branch}
          </button>
        </>
      )}
      <span className="sep" aria-hidden="true">
        ·
      </span>
      <AllowanceButton
        data={allowance}
        open={allowanceOpen}
        onOpen={onAllowance}
      />
      <span className="grow" />
      {context}
      <span className="version">{version ? `v${version}` : "Connecting"}</span>
    </footer>
  );
}
