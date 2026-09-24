import type { ChatItem } from "./cards";
import type { Fallback } from "../lib/allowance";

/** What happened after a subscription reported its plan limit
 * (limit.fallback): the conversation continued on this computer, the user is
 * asked, or nothing could continue. */
export function LimitFallbackItem({
  item,
  fallback,
  disabled,
  onContinue,
  onChoose,
  onOpenLocal,
}: {
  item: Extract<ChatItem, { kind: "limit" }>;
  /** The local model "Continue on …" uses. */
  fallback: Fallback | null;
  disabled?: boolean;
  onContinue: (fallback: Fallback) => void;
  onChoose: () => void;
  onOpenLocal: () => void;
}) {
  if (item.mode === "continued")
    return <div className="msg-note limit-note">{item.text}</div>;
  if (item.mode === "unavailable")
    return (
      <div className="msg-note warning limit-note">
        <span>{item.text}</span>
        <button type="button" className="mini" onClick={onOpenLocal}>
          Open Local models
        </button>
      </div>
    );
  return (
    <section
      className={`limit-card${item.resolved ? " is-resolved" : ""}`}
      aria-label="Plan limit reached"
    >
      <p>
        <strong>{item.text}</strong>
      </p>
      {item.resolved ? null : (
        <>
          <p className="dim">
            {fallback
              ? "Keep this conversation going on a model on this computer, or choose another model."
              : "No local model is ready. Choose another model, or add one in Local models."}
          </p>
          <div className="row">
            {fallback ? (
              <button
                type="button"
                className="mini primary-mini"
                disabled={disabled}
                onClick={() => onContinue(fallback)}
              >
                Continue on {fallback.name}
              </button>
            ) : (
              <button type="button" className="mini" onClick={onOpenLocal}>
                Open Local models
              </button>
            )}
            <button type="button" className="mini" onClick={onChoose}>
              Choose another model
            </button>
          </div>
        </>
      )}
    </section>
  );
}
