import { memo, type ReactNode } from "react";
import {
  highlight,
  numbered,
  splitRows,
  type Hunk,
  type NumberedLine,
} from "../lib/diff";

export type DiffLayout = "unified" | "split";

function Code({ text, language }: { text: string; language?: string }) {
  return (
    <>
      {highlight(text, language).map((token, i) =>
        token.kind ? (
          <span key={i} className={`tok-${token.kind}`}>
            {token.text}
          </span>
        ) : (
          token.text
        ),
      )}
    </>
  );
}

const SIGN = { add: "+", del: "−", ctx: " ", meta: " " } as const;
const LABEL = { add: "Added", del: "Removed", ctx: "", meta: "" } as const;

function Cell({
  line,
  side,
  language,
}: {
  line?: NumberedLine;
  side: "old" | "new";
  language?: string;
}) {
  if (!line)
    return (
      <>
        <td className="diff-num" />
        <td className="diff-code diff-empty" />
      </>
    );
  const number = side === "old" ? line.old : line.new;
  return (
    <>
      <td className="diff-num">{number ?? ""}</td>
      <td className={`diff-code diff-${line.kind}`}>
        <span className="diff-sign" aria-label={LABEL[line.kind] || undefined}>
          {SIGN[line.kind]}
        </span>
        <Code text={line.text} language={language} />
        {line.eol === false && (
          <span className="diff-noeol">no newline at end</span>
        )}
      </td>
    </>
  );
}

/** One hunk as a table: unified (old and new line numbers, one code
 * column) or split (old on the left, new on the right). */
export const DiffHunk = memo(function DiffHunk({
  hunk,
  layout,
  language,
  actions,
  status,
  maxLines,
}: {
  hunk: Hunk;
  layout: DiffLayout;
  language?: string;
  /** Buttons for this hunk (Keep / Undo). */
  actions?: ReactNode;
  /** A word shown in the header ("Kept", "Undone"). */
  status?: string;
  /** Show only the first lines (the approval card's preview). */
  maxLines?: number;
}) {
  const lines = numbered(hunk);
  const rows = layout === "split" ? splitRows(hunk) : [];
  const limit = maxLines ?? Infinity;
  return (
    <section className={`diff-hunk${status ? " is-settled" : ""}`}>
      <header>
        <code>{hunk.header}</code>
        {status && <span className="diff-hunk-status">{status}</span>}
        {actions && <span className="row diff-hunk-actions">{actions}</span>}
      </header>
      <table className={`diff-table diff-${layout}`}>
        <tbody>
          {layout === "unified"
            ? lines.slice(0, limit).map((line, i) => (
                <tr key={i}>
                  <td className="diff-num">{line.old ?? ""}</td>
                  <Cell line={line} side="new" language={language} />
                </tr>
              ))
            : rows.slice(0, limit).map((row, i) => (
                <tr key={i}>
                  <Cell line={row.left} side="old" language={language} />
                  <Cell line={row.right} side="new" language={language} />
                </tr>
              ))}
        </tbody>
      </table>
    </section>
  );
});

/** Unified / Split switch. */
export function LayoutToggle({
  layout,
  onLayout,
}: {
  layout: DiffLayout;
  onLayout: (layout: DiffLayout) => void;
}) {
  return (
    <div className="seg" role="group" aria-label="Diff layout">
      {(["unified", "split"] as const).map((id) => (
        <button
          key={id}
          type="button"
          className={layout === id ? "on" : ""}
          aria-pressed={layout === id}
          onClick={() => onLayout(id)}
        >
          {id === "unified" ? "Unified" : "Split"}
        </button>
      ))}
    </div>
  );
}
