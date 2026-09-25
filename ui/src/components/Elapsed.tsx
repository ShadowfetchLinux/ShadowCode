import { useEffect, useState } from "react";
import { formatDuration } from "../lib/activity";

const secondsSince = (startedAt: number) =>
  Math.max(0, Math.floor(Date.now() / 1000 - startedAt));

/** A running task's elapsed time. It ticks on its own once a second, so the
 * rest of the window does not re-render with it. */
export function Elapsed({ since }: { since: number }) {
  const [seconds, setSeconds] = useState(() => secondsSince(since));
  useEffect(() => {
    const tick = () => setSeconds(secondsSince(since));
    tick();
    const timer = setInterval(tick, 1000);
    return () => clearInterval(timer);
  }, [since]);
  return <>{formatDuration(seconds)}</>;
}
