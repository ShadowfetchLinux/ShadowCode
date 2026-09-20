import { useState } from "react";
import { api, type NativeMcpCatalog } from "../api";

export function McpSettings({
  catalog,
  onChange,
  onToast,
}: {
  catalog: NativeMcpCatalog;
  onChange: (catalog: NativeMcpCatalog) => void;
  onToast: (text: string, kind: "ok" | "err" | "info") => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [name, setName] = useState("");
  const [transport, setTransport] = useState("stdio");
  const [url, setUrl] = useState("");
  const [bearerReference, setBearerReference] = useState("");
  const [command, setCommand] = useState("");
  const [references, setReferences] = useState("{}");
  async function perform(
    action: () => Promise<NativeMcpCatalog>,
    message: string,
  ) {
    setBusy(true);
    setError("");
    try {
      onChange(await action());
      onToast(message, "ok");
      return true;
    } catch (error) {
      setError(String(error));
      return false;
    } finally {
      setBusy(false);
    }
  }
  async function register() {
    let definition: Record<string, unknown> = { name: name.trim() };
    if (transport === "http") {
      definition = {
        ...definition,
        url: url.trim(),
        ...(bearerReference.trim()
          ? { api_key_env: bearerReference.trim() }
          : {}),
      };
    } else {
      try {
        const argv: unknown = JSON.parse(command);
        const env_refs: unknown = JSON.parse(references);
        if (
          !Array.isArray(argv) ||
          !argv.length ||
          !argv.every((value) => typeof value === "string")
        )
          throw new Error("Command must be a JSON array of strings.");
        if (
          !env_refs ||
          typeof env_refs !== "object" ||
          Array.isArray(env_refs) ||
          !Object.values(env_refs).every((value) => typeof value === "string")
        )
          throw new Error("Secret references must be a JSON object of names.");
        definition = { ...definition, command: argv, env_refs };
      } catch (error) {
        setError(String(error));
        return;
      }
    }
    if (
      await perform(
        () => api.registerMcp(definition),
        "MCP server registered; review and enable it for this project",
      )
    ) {
      setName("");
      setCommand("");
      setReferences("{}");
      setUrl("");
      setBearerReference("");
    }
  }
  const missing = catalog.approved.filter(
    (grant) => !catalog.servers.some((server) => server.id === grant.server),
  );
  return (
    <section className="mcp-settings">
      <h3>MCP servers</h3>
      <p className="hint">
        Connect external tools to Build tasks in this project. Enabling a server
        allows its command to start or its HTTP endpoint to connect. Each tool
        call still needs approval. Plan and Review keep external servers
        inactive.
      </p>
      <p className="hint">
        Changes apply to new tasks. Cancel running or queued tasks to stop using
        their existing grants. External changes are not covered by file
        checkpoints.
      </p>
      <p className="hint">
        Project: <code>{catalog.workspace}</code>
      </p>
      {!catalog.trusted && (
        <p className="status-message" role="status">
          Trust this project before enabling a server.
        </p>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <button
        type="button"
        className="ghost"
        disabled={busy}
        onClick={() =>
          void perform(async () => {
            const next = await api.mcpServers();
            if (next.format !== "native-mcp-v1")
              throw new Error("The native MCP catalog is unavailable.");
            return next;
          }, "MCP definitions refreshed")
        }
      >
        Refresh MCP servers
      </button>
      <div className="list">
        {catalog.servers.map((server) => {
          const grant = catalog.approved.some(
            (item) => item.server === server.id,
          );
          return (
            <article
              className="mcp-server"
              key={server.id}
              data-server={server.id}
            >
              <div className="mcp-server-heading">
                <strong>{server.name}</strong>
                <span className="hint">
                  {server.enabled
                    ? "Enabled for new Build tasks"
                    : grant
                      ? "Definition changed — review again"
                      : "Inactive"}
                </span>
              </div>
              <p className="hint">{server.id}</p>
              {server.description && <p>{server.description}</p>}
              <pre className="mcp-command">
                {server.transport === "stdio"
                  ? JSON.stringify(server.command, null, 2)
                  : server.url}
              </pre>
              <p className="hint">
                Timeout: {server.timeout_sec}s · Definition{" "}
                <code title={server.hash}>{server.hash.slice(0, 12)}</code>
              </p>
              {server.env_names.length > 0 && (
                <p className="hint">
                  Configured environment: {server.env_names.join(", ")} (values
                  hidden)
                </p>
              )}
              {Object.keys(server.env_refs).length > 0 && (
                <p className="hint">
                  Secret references:{" "}
                  {Object.entries(server.env_refs)
                    .map(([key, value]) => `${key} → ${value}`)
                    .join(", ")}
                </p>
              )}
              {server.api_key_env && (
                <p className="hint">
                  Bearer secret reference: {server.api_key_env}
                </p>
              )}
              {server.transport === "http" && (
                <p className="hint">
                  Streamable HTTP · Remote hosts require network access in
                  Permissions.
                </p>
              )}
              <div className="mcp-actions">
                {!server.enabled && (
                  <button
                    type="button"
                    className="ghost"
                    disabled={busy || !catalog.trusted}
                    onClick={() =>
                      void perform(
                        () =>
                          api.activateMcp(
                            catalog.workspace,
                            server.id,
                            server.hash,
                            true,
                          ),
                        "MCP server enabled for new Build tasks",
                      )
                    }
                  >
                    Enable {server.name}
                  </button>
                )}
                {grant && (
                  <button
                    type="button"
                    className="ghost"
                    disabled={busy}
                    onClick={() =>
                      void perform(
                        () =>
                          api.activateMcp(
                            catalog.workspace,
                            server.id,
                            server.hash,
                            false,
                          ),
                        "MCP server disabled for new tasks",
                      )
                    }
                  >
                    Disable {server.name}
                  </button>
                )}
                {server.id.startsWith("config:") && (
                  <button
                    type="button"
                    className="ghost danger-text"
                    disabled={busy}
                    onClick={() =>
                      void perform(
                        () => api.removeMcp(server.id, server.hash),
                        "MCP registration removed",
                      )
                    }
                  >
                    Remove {server.name}
                  </button>
                )}
              </div>
            </article>
          );
        })}
        {missing.map((grant) => (
          <article className="mcp-server" key={grant.server}>
            <strong>Missing or invalid definition</strong>
            <p className="hint">{grant.server}</p>
            <button
              type="button"
              className="ghost"
              disabled={busy}
              onClick={() =>
                void perform(
                  () =>
                    api.activateMcp(catalog.workspace, grant.server, "", false),
                  "Stale MCP activation removed",
                )
              }
            >
              Disable {grant.server}
            </button>
          </article>
        ))}
        {!catalog.servers.length && !missing.length && (
          <p className="hint">No MCP servers configured.</p>
        )}
      </div>
      {catalog.issues.length > 0 && (
        <div role="status">
          {catalog.issues.map((issue, index) => (
            <p className="hint" key={index}>
              {issue}
            </p>
          ))}
        </div>
      )}
      <details className="mcp-add" open={catalog.servers.length === 0}>
        <summary>Register an MCP server</summary>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void register();
          }}
        >
          <label className="field">
            Server name
            <input
              required
              maxLength={80}
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="my-tools"
            />
          </label>
          <label className="field">
            Connection
            <select
              value={transport}
              onChange={(event) => setTransport(event.target.value)}
            >
              <option value="stdio">Local command (stdio)</option>
              <option value="http">Streamable HTTP</option>
            </select>
          </label>
          {transport === "http" ? (
            <>
              <label className="field">
                Server URL
                <input
                  required
                  type="url"
                  maxLength={4096}
                  value={url}
                  onChange={(event) => setUrl(event.target.value)}
                  placeholder="https://example.com/mcp"
                />
              </label>
              <label className="field">
                Bearer secret reference (optional)
                <input
                  maxLength={128}
                  value={bearerReference}
                  onChange={(event) => setBearerReference(event.target.value)}
                  placeholder="MY_MCP_TOKEN"
                />
              </label>
              <p className="hint">
                Enter a stored secret or environment variable name. Credentials
                require HTTPS outside localhost. Registration does not connect
                to the server.
              </p>
            </>
          ) : (
            <>
              <label className="field">
                Command and arguments
                <textarea
                  required
                  rows={3}
                  value={command}
                  onChange={(event) => setCommand(event.target.value)}
                  placeholder={'["/path/to/server", "--stdio"]'}
                />
              </label>
              <p className="hint">
                Use a JSON array. Arguments, spaces, and quotes are passed
                literally. Registration does not start the command.
              </p>
              <label className="field">
                Secret references
                <textarea
                  rows={2}
                  value={references}
                  onChange={(event) => setReferences(event.target.value)}
                  placeholder={'{"API_TOKEN":"MY_STORED_KEY"}'}
                />
              </label>
              <p className="hint">
                Map a server environment variable to a stored secret or
                environment variable name. Enter names here; store credential
                values separately.
              </p>
            </>
          )}
          <button type="submit" className="ghost" disabled={busy}>
            Register MCP server
          </button>
        </form>
      </details>
      <p className="hint">
        Project definitions are discovered in{" "}
        {catalog.dirs.map((dir, index) => (
          <span key={dir}>
            {index > 0 ? " and " : ""}
            <code>{dir}</code>
          </span>
        ))}
        . Edit a definition there, refresh, and review its new content before
        enabling it.
      </p>
    </section>
  );
}
