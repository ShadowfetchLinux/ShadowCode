import { afterEach, expect, it, vi } from "vitest";
import { DIFF_STAT_LIMIT, batchDiffStats } from "./diffStats";

afterEach(() => vi.useRealTimers());

it("sends every path asked for in the same tick in one request", async () => {
  const read = vi.fn(async (paths: string[]) =>
    Object.fromEntries(
      paths
        .filter((p) => p !== "image.png")
        .map((p) => [p, { add: p.length, del: 1 }]),
    ),
  );
  const stats = batchDiffStats(read);
  const [first, second] = await Promise.all([
    stats(["src/a.ts", "src/b.ts"]),
    stats(["src/b.ts", "image.png"]),
  ]);
  expect(read).toHaveBeenCalledTimes(1);
  expect(read.mock.calls[0][0].sort()).toEqual([
    "image.png",
    "src/a.ts",
    "src/b.ts",
  ]);
  expect(first).toEqual({
    "src/a.ts": { add: 8, del: 1 },
    "src/b.ts": { add: 8, del: 1 },
  });
  // Unknown (binary) files come back as null.
  expect(second).toEqual({ "src/b.ts": { add: 8, del: 1 }, "image.png": null });
  await stats(["later.ts"]);
  expect(read).toHaveBeenCalledTimes(2);
});

it("splits past the engine's per-request limit", async () => {
  const read = vi.fn(async (paths: string[]) =>
    Object.fromEntries(paths.map((p) => [p, { add: 1, del: 0 }])),
  );
  const paths = Array.from(
    { length: DIFF_STAT_LIMIT + 5 },
    (_, i) => `f${i}.ts`,
  );
  const result = await batchDiffStats(read)(paths);
  expect(read.mock.calls.map(([page]) => page.length)).toEqual([
    DIFF_STAT_LIMIT,
    5,
  ]);
  expect(Object.keys(result)).toHaveLength(paths.length);
});

it("a failed request fails every caller of that batch", async () => {
  const stats = batchDiffStats(async () => {
    throw new Error("engine gone");
  });
  await expect(stats(["a.ts"])).rejects.toThrow("engine gone");
});
