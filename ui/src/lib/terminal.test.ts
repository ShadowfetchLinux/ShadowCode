import { describe, expect, it } from "vitest";
import {
  decodeBase64,
  inputQueue,
  outputReader,
  type TerminalChunk,
} from "./terminal";

const b64 = (text: string) =>
  btoa(String.fromCharCode(...new TextEncoder().encode(text)));

describe("inputQueue", () => {
  it("sends keystrokes in order, batching what arrives mid-request", async () => {
    const sent: string[] = [];
    let release: () => void = () => undefined;
    const queue = inputQueue(
      (data) => {
        sent.push(data);
        return new Promise<void>((resolve) => (release = resolve));
      },
      () => undefined,
    );
    queue.push("l");
    queue.push("s");
    queue.push("\r");
    expect(sent).toEqual(["l"]);
    release();
    await new Promise((r) => setTimeout(r, 0));
    expect(sent).toEqual(["l", "s\r"]);
    release();
    await queue.idle();
  });

  it("splits large pastes under the engine limit and reports failures", async () => {
    const sizes: number[] = [];
    const errors: unknown[] = [];
    const queue = inputQueue(
      async (data) => {
        sizes.push(data.length);
        if (sizes.length === 2) throw new Error("This terminal was closed");
      },
      (e) => errors.push(e),
    );
    queue.push("x".repeat(150_000));
    await queue.idle();
    expect(sizes).toEqual([60_000, 60_000]);
    expect(String(errors[0])).toContain("closed");
    queue.close();
    queue.push("ignored");
    await queue.idle();
    expect(sizes).toHaveLength(2);
  });
});

describe("outputReader", () => {
  it("reads by cursor, follows `more` and coalesces wake-ups", async () => {
    const reads: number[] = [];
    const written: string[] = [];
    const chunks: Record<number, TerminalChunk> = {
      0: chunk(0, "hello ", 6, false),
      6: chunk(6, "world", 11, true),
      11: chunk(11, "", 11, false),
    };
    const reader = outputReader({
      id: "t",
      read: async (_, after) => {
        reads.push(after);
        return chunks[after];
      },
      write: (bytes) => written.push(new TextDecoder().decode(bytes)),
    });
    const first = reader.wake();
    void reader.wake(); // arrives during the read: one more pass
    void reader.wake(); // …not two
    await first;
    await new Promise((r) => setTimeout(r, 0));
    expect(written.join("")).toBe("hello world");
    expect(reads).toEqual([0, 6, 11]);
    expect(reader.cursor).toBe(11);
    reader.stop();
    await reader.wake();
    expect(reads).toHaveLength(3);
  });

  it("decodes UTF-8 split across chunks when written as bytes", () => {
    const bytes = decodeBase64(b64("é✓"));
    expect(new TextDecoder().decode(bytes)).toBe("é✓");
  });
});

function chunk(
  from: number,
  text: string,
  cursor: number,
  more: boolean,
): TerminalChunk {
  return {
    id: "t",
    data: b64(text),
    from,
    cursor,
    more,
    truncated: false,
    exited: false,
    exit_code: null,
  };
}
