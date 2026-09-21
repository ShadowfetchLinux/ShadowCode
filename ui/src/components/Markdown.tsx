import { memo, useState, type ComponentPropsWithoutRef } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Check, Copy } from "lucide-react";

// Providers may send a response up to 16 MiB. Rendering all of that as one
// Markdown tree can make the desktop unresponsive, while the durable event and
// JSON export still retain the complete response.
export const MARKDOWN_PREVIEW_LIMIT = 512 * 1024;

function CodeBlock({ children, ...props }: ComponentPropsWithoutRef<"pre">) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="code-block">
      <button
        type="button"
        className="copy-code"
        aria-label={copied ? "Code copied" : "Copy code"}
        onClick={async (e) => {
          const text =
            e.currentTarget.parentElement?.querySelector("pre")?.textContent ||
            "";
          try {
            await navigator.clipboard.writeText(text);
            setCopied(true);
            setTimeout(() => setCopied(false), 1800);
          } catch {
            setCopied(false);
          }
        }}
      >
        {copied ? <Check size={14} /> : <Copy size={14} />}
        {copied ? "Copied" : "Copy"}
      </button>
      <pre {...props}>{children}</pre>
    </div>
  );
}

// Streaming one response must not reparse every earlier message.
export const Markdown = memo(function Markdown({
  children,
}: {
  children: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const abbreviated = children.length > MARKDOWN_PREVIEW_LIMIT && !expanded;
  const visible = abbreviated
    ? children.slice(0, MARKDOWN_PREVIEW_LIMIT)
    : children;
  return (
    <div className="markdown">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          pre: CodeBlock,
          a: ({ children, ...props }) => (
            <a {...props} target="_blank" rel="noopener noreferrer">
              {children}
            </a>
          ),
          // Remote images from model output must not make invisible network requests.
          img: ({ alt }) => (
            <span className="hint">[Image: {alt || "attachment"}]</span>
          ),
        }}
      >
        {visible}
      </ReactMarkdown>
      {abbreviated && (
        <div className="markdown-preview" role="status">
          <p className="hint">
            Showing the first 512 KiB of this response. The complete response is
            retained in task history and JSON export.
          </p>
          <button
            type="button"
            className="mini"
            onClick={() => setExpanded(true)}
          >
            Show full response
          </button>
        </div>
      )}
    </div>
  );
});
