import { GitBranch } from "lucide-react";
import type { AllowanceResponse } from "../../api";
import { AllowanceButton } from "../Allowance";

const formatTokens = (n: number) =>
  n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n);

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
  contextPercent,
  totalTokens,
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
  contextPercent: number;
  totalTokens: number;
  version: string;
}) {
  const ctx = contextPercent;
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
      {ctx > 0 && (
        <span
          className={`ctx-bar${ctx >= 80 ? " ctx-warn" : ""}`}
          role="img"
          aria-label={`${ctx}% of context used`}
          title={`${ctx}% of context used · ${formatTokens(totalTokens)} tokens`}
        >
          <span
            className="ctx-fill"
            style={{ width: `${Math.min(ctx, 100)}%` }}
          />
        </span>
      )}
      <span className="version">{version ? `v${version}` : "Connecting"}</span>
    </footer>
  );
}
