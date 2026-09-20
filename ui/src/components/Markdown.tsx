import { useState, type ComponentPropsWithoutRef } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Check, Copy } from "lucide-react";

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

export function Markdown({ children }: { children: string }) {
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
        {children}
      </ReactMarkdown>
    </div>
  );
}
