import { useLayoutEffect, useRef, useState } from "react";
import {
  api,
  type NativePluginCatalog,
  type PluginEntry,
  type PluginPreview,
} from "../api";

export function PluginSettings({
  catalog,
  onChange,
  onNavigate,
  onToast,
}: {
  catalog: NativePluginCatalog;
  onChange: (value: NativePluginCatalog) => void;
  onNavigate: (section: "hooks" | "mcp") => void;
  onToast: (text: string, kind: "ok" | "err" | "info") => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [source, setSource] = useState("");
  const [preview, setPreview] = useState<PluginPreview | null>(null);
  const review = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (preview) {
      review.current?.scrollIntoView({ block: "start" });
      review.current?.focus({ preventScroll: true });
    }
  }, [preview]);
  const [retained, setRetained] = useState<{ path: string; reason: string }[]>(
    [],
  );
  async function perform(action: () => Promise<void>) {
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
    }
  }
  async function refresh() {
    const value = await api.plugins();
    if (value.format !== "native-plugins-v1")
      throw new Error("Native plugin catalog unavailable");
    onChange(value);
  }
  async function remove(entry: PluginEntry) {
    const value = await api.removeNativePlugin(
      catalog.workspace,
      entry.name,
      entry.hash,
    );
    onChange(value.catalog);
    setRetained(value.result.retained || []);
    setPreview(null);
    onToast(
      value.result.retained?.length
        ? "Plugin removed; your changed files were preserved"
        : "Plugin removed from this project",
      "ok",
    );
  }
  const mutable = catalog.trusted && !catalog.read_only;
  const entries = [
    ...catalog.installed,
    ...catalog.available.filter(
      (p) => !catalog.installed.some((i) => i.name === p.name),
    ),
  ];
  return (
    <section className="plugin-settings">
      <h3>Project plugins</h3>
      <p className="hint">
        Install skills, slash commands, hooks and MCP definitions together.
        Review the bundle first. Hooks and MCP servers need separate activation;
        installing a plugin runs no commands.
      </p>
      <p className="hint wrap">
        Project: <code>{catalog.workspace}</code>
      </p>
      {!mutable && (
        <p className="warning">
          Trust this project and allow workspace changes before installing or
          removing plugins.
        </p>
      )}
      <div className="row wrap">
        <button
          type="button"
          className="ghost"
          disabled={busy}
          onClick={() => void perform(refresh)}
        >
          Refresh plugins
        </button>
        <button
          type="button"
          className="ghost"
          onClick={() => onNavigate("hooks")}
        >
          Review plugin hooks
        </button>
        <button
          type="button"
          className="ghost"
          onClick={() => onNavigate("mcp")}
        >
          Review plugin MCP servers
        </button>
      </div>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {catalog.issues.map((issue) => (
        <p className="warning" key={issue}>
          {issue}
        </p>
      ))}
      {!!catalog.legacy.length && (
        <p className="warning">
          Legacy bundles preserved: {catalog.legacy.join(", ")}. Install native
          replacements below or import a converted JSON bundle. Python callbacks
          and old registry stubs remain inactive.
        </p>
      )}
      {!!retained.length && (
        <div role="status">
          <strong>Preserved files</strong>
          <ul>
            {retained.map((file) => (
              <li key={file.path}>
                <code>{file.path}</code> — {file.reason}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="list">
        {entries.map((entry) => (
          <article className="item static plugin-entry" key={entry.name}>
            <div>
              <strong>{entry.name}</strong>
              <span className="dim"> · {entry.version}</span>
            </div>
            <p>{entry.description}</p>
            <span className="hint">
              {entry.state === "installed"
                ? "Installed in this project"
                : entry.state
                  ? "Interrupted operation — remove to recover"
                  : "Available to install"}{" "}
              · {entry.files.length} files
            </span>
            <details>
              <summary>Included files</summary>
              <ul>
                {entry.files.map((file) => (
                  <li key={file.path}>
                    <code>{file.path}</code> · {file.kind}
                    {file.status && file.status !== "unchanged"
                      ? ` · ${file.status}`
                      : ""}
                  </li>
                ))}
              </ul>
            </details>
            <div className="row wrap">
              {entry.state ? (
                <button
                  type="button"
                  className="mini danger-text"
                  disabled={busy || !mutable}
                  onClick={() => void perform(() => remove(entry))}
                >
                  Remove {entry.name}
                </button>
              ) : (
                <button
                  type="button"
                  className="mini"
                  disabled={busy}
                  onClick={() =>
                    void perform(async () => {
                      setPreview(await api.previewPlugin({ name: entry.name }));
                    })
                  }
                >
                  Review {entry.name}
                </button>
              )}
            </div>
          </article>
        ))}
      </div>
      <details className="plugin-import">
        <summary>Import a local bundle</summary>
        <p className="hint">
          Paste a shadowcode-plugin-v1 JSON bundle, or choose a JSON file (up to
          256 KB). Imported content is shown before installation.
        </p>
        <label className="field">
          Bundle file
          <input
            type="file"
            accept=".json,application/json"
            disabled={busy}
            onChange={(event) => {
              const file = event.target.files?.[0];
              if (!file) return;
              void perform(async () => {
                if (file.size > 256000)
                  throw new Error("Plugin bundle exceeds 256 KB");
                const text = await file.text();
                setSource(text);
                setPreview(
                  await api.previewPlugin({ bundle: JSON.parse(text) }),
                );
              });
            }}
          />
        </label>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void perform(async () => {
              setPreview(
                await api.previewPlugin({ bundle: JSON.parse(source) }),
              );
            });
          }}
        >
          <label className="field">
            Bundle JSON
            <textarea
              rows={7}
              value={source}
              maxLength={256000}
              onChange={(event) => {
                setSource(event.target.value);
                setPreview(null);
              }}
              placeholder='{"format":"shadowcode-plugin-v1","name":"my-plugin",…}'
            />
          </label>
          <button
            type="submit"
            className="ghost"
            disabled={busy || !source.trim()}
          >
            Review imported bundle
          </button>
        </form>
      </details>
      {preview && (
        <div
          ref={review}
          tabIndex={-1}
          role="region"
          className="plugin-preview"
          aria-label="Plugin review"
        >
          <h4>
            Review {preview.bundle.name} · {preview.bundle.version}
          </h4>
          <p>{preview.bundle.description}</p>
          <p className="hint">
            Skills appear in the Skills panel and slash menu. Supporting files
            stay beside their skill. Project tools used by a bundle must already
            be installed or installed through normal approvals.
          </p>
          {preview.files.map((file) => (
            <details key={file.path}>
              <summary>
                {file.path} · {file.kind}
              </summary>
              <pre className="code">{file.content}</pre>
            </details>
          ))}
          <p className="hint">
            Removal deletes only unchanged bundle files and disables this
            bundle's hooks and MCP servers. Local edits are preserved.
          </p>
          <div className="row wrap">
            <button
              type="button"
              className="primary"
              disabled={
                busy ||
                !mutable ||
                catalog.installed.some((p) => p.name === preview.bundle.name)
              }
              onClick={() =>
                void perform(async () => {
                  const value = await api.installNativePlugin(
                    catalog.workspace,
                    preview,
                  );
                  onChange(value.catalog);
                  setPreview(null);
                  setRetained([]);
                  onToast(
                    "Plugin installed; skills and commands are ready to use",
                    "ok",
                  );
                })
              }
            >
              Install {preview.bundle.name}
            </button>
            <button
              type="button"
              className="ghost"
              disabled={busy}
              onClick={() => setPreview(null)}
            >
              Close review
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
