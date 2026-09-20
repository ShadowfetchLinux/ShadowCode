// Test-only MCP peer. Node is a build/test dependency, never an application sidecar.
import { appendFileSync, writeFileSync, renameSync } from "node:fs";
import { createInterface } from "node:readline";
import { spawn } from "node:child_process";
const mode = process.argv[2];
process.on("SIGTERM", () => {});
const child = spawn(
  process.execPath,
  ["-e", 'process.on("SIGTERM",()=>{}); setInterval(()=>{},1000)'],
  { stdio: ["ignore", "ignore", "ignore"] },
);
writeFileSync(
  `${process.env.MCP_PID_FILE}.pending`,
  JSON.stringify([process.pid, child.pid]),
);
renameSync(`${process.env.MCP_PID_FILE}.pending`, process.env.MCP_PID_FILE);
const send = (id, result) =>
  process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id, result })}\n`);
let page = 0;
let pending;
const tool = (name) => ({
  name,
  description: `Fixture ${name}`,
  inputSchema: { type: "object", properties: {} },
  annotations: { readOnlyHint: name === "echo" },
});
createInterface({ input: process.stdin }).on("line", async (line) => {
  const msg = JSON.parse(line);
  appendFileSync(process.env.MCP_REQUEST_FILE, `${line}\n`);
  if (msg.method === "initialize") {
    if (mode === "init_hang") return;
    if (mode === "init_oversize") {
      process.stdout.write("x".repeat(1_048_577));
      return;
    }
    if (mode === "init_malformed") {
      process.stdout.write("not JSON\n");
      return;
    }
    if (mode === "init_truncated") {
      process.stdout.write('{"jsonrpc":');
      process.stdout.end();
      return;
    }
    send(msg.id, {
      protocolVersion:
        mode === "legacy" ? "2024-11-05" : msg.params.protocolVersion,
      serverInfo: { name: "native-fixture", version: "1" },
      capabilities: { tools: {} },
    });
  } else if (msg.method === "tools/list") {
    page++;
    if (mode === "duplicate")
      send(msg.id, { tools: [tool("echo"), tool("echo")] });
    else if (mode === "cursor")
      send(msg.id, { tools: [tool(`page${page}`)], nextCursor: "repeated" });
    else if (mode === "pages")
      send(msg.id, {
        tools: [tool(`page${page}`)],
        nextCursor: `page${page + 1}`,
      });
    else if (mode === "many")
      send(msg.id, {
        tools: Array.from({ length: 129 }, (_, i) => tool(`tool${i}`)),
      });
    else if (mode === "paginated")
      send(msg.id, {
        tools: [tool(page === 1 ? "echo" : "failure")],
        ...(page === 1 ? { nextCursor: "second" } : {}),
      });
    else
      send(msg.id, {
        tools: [
          "echo",
          "failure",
          "hang",
          "noise",
          "oversize",
          "sample",
          "interact",
          "rate",
        ].map(tool),
      });
  } else if (msg.method === "tools/call") {
    const name = msg.params.name;
    if (name === "hang") return;
    if (name === "oversize") {
      send(msg.id, {
        content: [{ type: "text", text: "x".repeat(1_048_576) }],
      });
      return;
    }
    if (name === "rate") {
      process.stdout.write("\n".repeat(200));
      return;
    }
    if (name === "noise") {
      await new Promise((resolve) =>
        process.stderr.write("e".repeat(2_000_000), resolve),
      );
    }
    if (name === "sample" || name === "interact") {
      pending = msg.id;
      process.stdout.write(
        `${JSON.stringify({ jsonrpc: "2.0", id: "peer-input", method: name === "sample" ? "sampling/createMessage" : "elicitation/create", params: name === "sample" ? { messages: [], maxTokens: 16 } : { message: "Supply credentials", requestedSchema: { type: "object", properties: {} } } })}\n`,
      );
      return;
    }
    send(msg.id, {
      content: [{ type: "text", text: JSON.stringify(msg.params.arguments) }],
      structuredContent: {
        arguments: msg.params.arguments,
        cwd: process.cwd(),
        literal: process.argv[3],
        environment: process.env.MCP_LITERAL,
        environmentKeys: Object.keys(process.env),
      },
      isError: name === "failure",
    });
  } else if (msg.id === "peer-input") {
    send(pending, {
      content: [{ type: "text", text: "Client reply" }],
      structuredContent: { response: msg },
      isError: false,
    });
  }
});
