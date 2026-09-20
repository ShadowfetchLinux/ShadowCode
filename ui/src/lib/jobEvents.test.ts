import { afterEach, describe, expect, it, vi } from "vitest";
import { nativeJobStream } from "./jobEvents";
import type { EventRow, Job } from "../api";

afterEach(() => vi.useRealTimers());
const job = (status: string, cursor: number) =>
  ({ id: "job", status, event_cursor: cursor }) as Job;
const rows = (start: number, count: number): EventRow[] =>
  Array.from({ length: count }, (_, i) => ({
    id: start + i,
    ts: 0,
    type: "model.stream",
    payload: { text: String(start + i) },
  }));

describe("native event replay", () => {
  it("drains every page before completion and unsubscribes exactly once", async () => {
    vi.useFakeTimers();
    const stop = vi.fn();
    const read = vi.fn(async (cursor: number) => ({
      events: cursor === 0 ? rows(1, 512) : rows(513, 8),
      job: job("completed", 520),
    }));
    const stream = nativeJobStream(0, { read, subscribe: async () => stop });
    const received: { id?: number; type: string }[] = [];
    stream.onmessage = (event) => received.push(JSON.parse(event.data));
    await vi.advanceTimersByTimeAsync(30);
    expect(read.mock.calls.map(([cursor]) => cursor)).toEqual([0, 512]);
    expect(received.slice(0, -1).map((e) => e.id)).toEqual(
      Array.from({ length: 520 }, (_, i) => i + 1),
    );
    expect(received.at(-1)?.type).toBe("job.done");
    stream.close();
    expect(stop).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(10000);
    expect(read).toHaveBeenCalledTimes(2);
  });
  it("ignores an in-flight response and releases a listener registered after navigation", async () => {
    vi.useFakeTimers();
    let resolveRead!: (value: { events: EventRow[]; job: Job }) => void;
    let resolveListener!: (stop: () => void) => void;
    const stop = vi.fn();
    const stream = nativeJobStream(10, {
      read: () =>
        new Promise((resolve) => {
          resolveRead = resolve;
        }),
      subscribe: () =>
        new Promise((resolve) => {
          resolveListener = resolve;
        }),
    });
    const receive = vi.fn();
    stream.onmessage = receive;
    await vi.advanceTimersByTimeAsync(30);
    stream.close();
    resolveListener(stop);
    resolveRead({ events: rows(11, 1), job: job("completed", 11) });
    await vi.advanceTimersByTimeAsync(30);
    expect(receive).not.toHaveBeenCalled();
    expect(stop).toHaveBeenCalledTimes(1);
  });
  it("recovers a missed notification through polling from the last durable cursor", async () => {
    vi.useFakeTimers();
    const read = vi
      .fn()
      .mockRejectedValueOnce(new Error("transport failed"))
      .mockResolvedValueOnce({
        events: rows(21, 2),
        job: job("completed", 22),
      });
    const stream = nativeJobStream(20, {
      read,
      subscribe: async () => () => {},
    });
    const failed = vi.fn(),
      opened = vi.fn(),
      received = vi.fn();
    stream.onerror = failed;
    stream.onopen = opened;
    stream.onmessage = received;
    await vi.advanceTimersByTimeAsync(30);
    expect(failed).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(2530);
    expect(opened).toHaveBeenCalledTimes(1);
    expect(read.mock.calls).toEqual([[20], [20]]);
    expect(received).toHaveBeenCalledTimes(3);
    stream.close();
  });
});
