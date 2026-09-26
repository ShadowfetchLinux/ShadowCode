/**
 * A stand-in for the engine's remote access server (native/core/src/remote)
 * for the web-interface tests: it serves the built UI, exchanges one-time
 * pairing codes for tokens, requires `Authorization: Bearer` on /api/* and
 * the event stream, and answers the API with the same deterministic fake
 * engine the desktop tests use (e2e/fakeBackend.ts), running here in Node.
 * It listens on 127.0.0.1 only; nothing leaves this computer.
 */
import { createServer, type IncomingMessage, type Server } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, join, normalize, sep } from "node:path";
import { randomBytes } from "node:crypto";
import { installFakeBackend, type FakeOptions } from "./fakeBackend";

type Fake = ReturnType<typeof installFakeBackend>;

const TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".webmanifest": "application/manifest+json",
  ".json": "application/json",
};

export type RemoteServer = {
  url: string;
  fake: Fake;
  /** A new single-use pairing code. */
  pairingCode(): string;
  /** Requests seen, with whether they carried a valid token. */
  requests: { method: string; path: string; authorized: boolean }[];
  close(): Promise<void>;
};

async function body(request: IncomingMessage): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks).toString("utf8");
}

export async function startRemoteServer(
  root: string,
  options: FakeOptions = {},
): Promise<RemoteServer> {
  // fakeBackend installs itself on `window`; in Node that is this global.
  const scope = globalThis as unknown as { window?: unknown };
  scope.window ??= globalThis;
  const fake = installFakeBackend(options);
  const codes = new Set<string>();
  const tokens = new Set<string>();
  const requests: RemoteServer["requests"] = [];
  const streams = new Set<() => void>();

  const authorized = (request: IncomingMessage) => {
    const header = request.headers.authorization || "";
    return header.startsWith("Bearer ") && tokens.has(header.slice(7));
  };

  const server: Server = createServer(async (request, response) => {
    const url = new URL(request.url || "/", "http://remote.test");
    const send = (status: number, value: unknown) => {
      response.writeHead(status, { "Content-Type": "application/json" });
      response.end(JSON.stringify(value));
    };
    const ok = authorized(request);
    requests.push({
      method: request.method || "GET",
      path: url.pathname + url.search,
      authorized: ok,
    });
    try {
      if (url.pathname === "/_remote/pair" && request.method === "POST") {
        const { code } = JSON.parse((await body(request)) || "{}");
        if (!codes.delete(code))
          return send(401, {
            error:
              "This pairing link is not valid any more. Create a new one on the computer running ShadowCode.",
          });
        const token = `scr_${randomBytes(32).toString("base64url")}`;
        tokens.add(token);
        return send(200, { token, device: { id: "d1", name: "Phone" } });
      }
      if (
        url.pathname.startsWith("/api/") ||
        url.pathname.startsWith("/_remote/")
      ) {
        if (!ok) return send(401, { error: "This device is not paired." });
      }
      if (url.pathname === "/_remote/session")
        return send(200, {
          device: { id: "d1", name: "Phone" },
          allow_terminals: false,
          version: "test",
        });
      if (url.pathname === "/_remote/stream") {
        response.writeHead(200, {
          "Content-Type": "text/event-stream",
          "Cache-Control": "no-store",
        });
        const write = (event: string) => (payload: unknown) =>
          response.write(
            `event: ${event}\ndata: ${JSON.stringify(payload)}\n\n`,
          );
        response.write("retry: 3000\n\n");
        write("shadowcode:events")({});
        const stops = await Promise.all([
          fake.bridge.listen("shadowcode:events", write("shadowcode:events")),
        ]);
        const stop = () => stops.forEach((s) => s());
        streams.add(() => {
          stop();
          response.end();
        });
        request.on("close", stop);
        return;
      }
      if (url.pathname.startsWith("/api/")) {
        const text = request.method === "GET" ? "" : await body(request);
        try {
          const result = await fake.bridge.request(
            url.pathname + url.search,
            request.method || "GET",
            text ? JSON.parse(text) : null,
          );
          return send(200, result);
        } catch (error) {
          return send(400, {
            error: String((error as Error).message || error),
          });
        }
      }
      // Static files from the built interface, never outside it.
      const relative =
        url.pathname === "/" ? "index.html" : url.pathname.slice(1);
      const file = normalize(join(root, relative));
      if (
        !file.startsWith(root + sep) ||
        relative.split("/").some((p) => p.startsWith("."))
      )
        return send(404, { error: "Not found" });
      const info = await stat(file).catch(() => null);
      if (!info?.isFile()) return send(404, { error: "Not found" });
      response.writeHead(200, {
        "Content-Type": TYPES[extname(file)] || "application/octet-stream",
      });
      response.end(await readFile(file));
    } catch (error) {
      send(500, { error: String(error) });
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : 0;
  return {
    url: `http://127.0.0.1:${port}`,
    fake,
    requests,
    pairingCode() {
      const code = randomBytes(32).toString("base64url");
      codes.add(code);
      return code;
    },
    async close() {
      streams.forEach((end) => end());
      server.closeAllConnections();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    },
  };
}
