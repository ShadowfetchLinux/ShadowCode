import { GitCompareArrows, GitPullRequest, LoaderCircle } from "lucide-react";

/** Banners above the conversation: a shared engine, a plan limit, an
 * interrupted task, a lost connection and a comparison lane. */
export function StageBanners({
  attached,
  limitVendor,
  onChooseModel,
  interrupted,
  onContinueInterrupted,
  reconnecting,
  laneName,
  switching,
  onBackToCompare,
  issueOffer,
}: {
  attached: boolean;
  /** The vendor whose plan limit stopped the conversation. */
  limitVendor?: string;
  onChooseModel: () => void;
  interrupted: boolean;
  onContinueInterrupted: () => void;
  reconnecting: boolean;
  /** Set while a comparison lane's conversation is open. */
  laneName?: string;
  switching: boolean;
  onBackToCompare: () => void;
  /** A task started from an issue finished: offer its pull request. */
  issueOffer?: {
    number: number;
    onOpen: () => void;
    onDismiss: () => void;
  } | null;
}) {
  return (
    <>
      {attached && (
        <div className="connection-banner" role="status">
          Connected to your running engine. Closing this window leaves its work
          running.
        </div>
      )}
      {limitVendor && (
        <div className="connection-banner limit-banner" role="alert">
          <span>
            Plan limit reached on {limitVendor} · choose another model
          </span>
          <button type="button" className="mini" onClick={onChooseModel}>
            Choose model
          </button>
        </div>
      )}
      {interrupted && (
        <div className="connection-banner" role="status">
          <span>
            This task was interrupted. Its recorded work is available below.
          </span>
          <button
            type="button"
            className="mini"
            onClick={onContinueInterrupted}
          >
            Continue task
          </button>
        </div>
      )}
      {issueOffer && (
        <div className="connection-banner issue-banner" role="status">
          <GitPullRequest size={14} aria-hidden="true" />
          <span>Task for issue #{issueOffer.number} finished.</span>
          <button type="button" className="mini" onClick={issueOffer.onOpen}>
            Open PR that closes #{issueOffer.number}
          </button>
          <button type="button" className="mini" onClick={issueOffer.onDismiss}>
            Dismiss
          </button>
        </div>
      )}
      {reconnecting && (
        <div className="connection-banner" role="status">
          <LoaderCircle size={14} className="spin" aria-hidden="true" />
          Reconnecting… Your task continues in the background.
        </div>
      )}
      {laneName !== undefined && (
        <div className="connection-banner compare-banner" role="status">
          <GitCompareArrows size={14} aria-hidden="true" />
          <span>
            Part of a comparison · {laneName} works in its own copy of the
            project
          </span>
          <button
            type="button"
            className="mini"
            disabled={switching}
            onClick={onBackToCompare}
          >
            Back to comparison
          </button>
        </div>
      )}
    </>
  );
}
