export type LineCounts = { add: number; del: number } | null;
type Read = (paths: string[]) => Promise<Record<string, LineCounts>>;

/** The engine counts at most this many paths per request. */
export const DIFF_STAT_LIMIT = 200;

/** Task summaries ask for line counts as they mount. Every path asked for
 * in the same tick goes into one request (split only past the engine's
 * limit), instead of one full diff per file. */
export function batchDiffStats(read: Read) {
  let queue: Set<string> | null = null;
  let flush: Promise<Record<string, LineCounts>> | null = null;
  return (paths: string[]): Promise<Record<string, LineCounts>> => {
    if (!queue || !flush) {
      const current = new Set<string>();
      queue = current;
      flush = new Promise((resolve) => setTimeout(resolve, 0)).then(
        async () => {
          queue = null;
          const all = [...current];
          const pages = [];
          for (let i = 0; i < all.length; i += DIFF_STAT_LIMIT)
            pages.push(read(all.slice(i, i + DIFF_STAT_LIMIT)));
          return Object.assign({}, ...(await Promise.all(pages)));
        },
      );
    }
    for (const path of paths) queue.add(path);
    return flush.then((stats) =>
      Object.fromEntries(paths.map((path) => [path, stats[path] ?? null])),
    );
  };
}
