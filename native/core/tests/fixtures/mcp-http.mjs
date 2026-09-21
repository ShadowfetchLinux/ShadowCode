// Test-only HTTP/SSE peer. The application never bundles Node.
import { createServer } from "node:http";
import { writeFileSync, appendFileSync, renameSync } from "node:fs";
const [mode, root] = process.argv.slice(2);
const sockets = new Set(),
  streams = new Set();
let deletes = 0;
const json = (res, status, value, headers = {}) => {
  res.writeHead(status, { "Content-Type": "application/json", ...headers });
  res.end(JSON.stringify(value));
};
const rpc = (res, id, result, headers = {}) =>
  json(res, 200, { jsonrpc: "2.0", id, result }, headers);
const held = (res) => {
  streams.add(res);
  res.on("close", () => streams.delete(res));
};
const tool = (name) => ({
  name,
  description: `HTTP fixture ${name}`,
  inputSchema: {
    type: "object",
    properties: { region: { type: "string", "x-mcp-header": "Region" } },
  },
});
const server = createServer(async (req, res) => {
  try {
    if (req.url === "/status")
      return json(res, 200, {
        streams: streams.size,
        sockets: sockets.size,
        deletes,
      });
    const pieces = [];
    for await (const piece of req) pieces.push(piece);
    const body = Buffer.concat(pieces).toString();
    const message = body ? JSON.parse(body) : null;
    appendFileSync(
      `${root}/requests.jsonl`,
      `${JSON.stringify({ method: req.method, path: req.url, headers: req.headers, message })}\n`,
    );
    if (req.url === "/stolen") return json(res, 500, { stolen: true });
    if (req.method === "DELETE") {
      deletes++;
      res.writeHead(204);
      res.end();
      return;
    }
    if (req.method === "GET") {
      if (mode === "legacy") {
        res.writeHead(200, { "Content-Type": "text/event-stream" });
        res.write(": connected\n\n");
        held(res);
        return;
      }
      res.writeHead(405);
      res.end();
      return;
    }
    if (
      mode === "auth" &&
      req.headers.authorization !== "Bearer http-private-fixture-key"
    ) {
      json(
        res,
        401,
        { message: "private-response-must-not-leak" },
        { "WWW-Authenticate": "Bearer private-challenge-must-not-leak" },
      );
      return;
    }
    if (mode === "redirect") {
      res.writeHead(307, { Location: "/stolen" });
      res.end();
      return;
    }
    if (message.method === "server/discover") {
      if (mode.startsWith("legacy")) {
        res.writeHead(400);
        res.end("initialize first");
        return;
      }
      if (mode === "init_hang") {
        held(res);
        return;
      }
      if (mode === "init_json_large") {
        json(res, 200, { padding: "x".repeat(1_048_577) });
        return;
      }
      if (mode === "init_malformed") {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end("private-invalid-body");
        return;
      }
      if (mode === "init_truncated") {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end('{"jsonrpc":');
        return;
      }
      rpc(res, message.id, {
        resultType: "complete",
        supportedVersions: ["2026-07-28"],
        capabilities: { tools: {} },
        ttlMs: 0,
        cacheScope: "private",
        _meta: {
          "io.modelcontextprotocol/serverInfo": {
            name: "http-fixture",
            version: "1",
          },
        },
      });
      return;
    }
    if (message.method === "initialize") {
      rpc(
        res,
        message.id,
        {
          protocolVersion: "2025-11-25",
          capabilities: { tools: {} },
          serverInfo: { name: "legacy-http-fixture", version: "1" },
        },
        { "Mcp-Session-Id": "fixture-session" },
      );
      return;
    }
    if (message.method === "tools/list") {
      if (mode === "catalog_error") {
        json(res, 200, {
          jsonrpc: "2.0",
          id: message.id,
          error: {
            code: -32603,
            message: "private-error-credential",
            data: { key: "private-error-credential" },
          },
        });
        return;
      }
      rpc(res, message.id, {
        resultType: "complete",
        tools: [
          tool("echo"),
          tool("failure"),
          tool("雪"),
          {
            name: "bad-header",
            inputSchema: {
              type: "object",
              properties: { bad: { type: "string", "x-mcp-header": "" } },
            },
          },
        ],
      });
      return;
    }
    if (message.method === "tools/call") {
      const action = message.params.arguments.action;
      if (action === "expired") {
        res.writeHead(404);
        res.end("session expired");
        return;
      }
      if (action === "redirect") {
        res.writeHead(307, { Location: "/stolen" });
        res.end();
        return;
      }
      if (action === "json_large") {
        json(res, 200, { padding: "x".repeat(1_048_577) });
        return;
      }
      if (action === "chunked_large") {
        res.writeHead(200, { "Content-Type": "application/json" });
        for (let i = 0; i < 18; i++) res.write("x".repeat(65_536));
        res.end();
        return;
      }
      if (
        [
          "hang",
          "sse_large",
          "sse_invalid",
          "sse_flood",
          "sse_incomplete",
        ].includes(action)
      ) {
        res.writeHead(200, { "Content-Type": "text/event-stream" });
        if (action === "hang") {
          res.write(": waiting\n\n");
          held(res);
          return;
        }
        if (action === "sse_large") res.end(`data: ${"x".repeat(1_048_577)}`);
        if (action === "sse_invalid")
          res.end("data: private-invalid-frame\n\n");
        if (action === "sse_flood") res.end("\n".repeat(200));
        if (action === "sse_incomplete") res.end('data: {"jsonrpc":');
        return;
      }
      const result = {
        resultType: "complete",
        content: [{ type: "text", text: "HTTP fixture 雪" }],
        structuredContent: {
          arguments: message.params.arguments,
          headers: req.headers,
        },
        isError: message.params.name === "failure",
      };
      if (action === "sse") {
        res.writeHead(200, {
          "Content-Type": "text/event-stream; charset=utf-8",
        });
        const wire = Buffer.from(
          `event: message\r\ndata: ${JSON.stringify({ jsonrpc: "2.0", id: message.id, result })}\r\n\r\n`,
        );
        // Split every UTF-8 scalar and CRLF pair across transport chunks.
        for (const byte of wire) {
          if (res.destroyed) break;
          res.write(Buffer.from([byte]));
          await new Promise((resolve) => setImmediate(resolve));
        }
        res.end();
        return;
      }
      rpc(res, message.id, result);
      return;
    }
    if (message.method?.startsWith("notifications/")) {
      res.writeHead(202);
      res.end();
      return;
    }
    json(res, 200, {
      jsonrpc: "2.0",
      id: message.id,
      error: { code: -32601, message: "Unknown method" },
    });
  } catch (error) {
    process.stderr.write(`${error}\n`);
    if (!res.headersSent) res.writeHead(500);
    res.end();
  }
});
server.on("connection", (socket) => {
  sockets.add(socket);
  socket.on("close", () => sockets.delete(socket));
});
server.listen(0, "127.0.0.1", () => {
  writeFileSync(
    `${root}/ready.pending`,
    JSON.stringify({ url: `http://127.0.0.1:${server.address().port}/mcp` }),
  );
  renameSync(`${root}/ready.pending`, `${root}/ready.json`);
});
