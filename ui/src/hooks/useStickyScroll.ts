import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type UIEvent,
} from "react";

/** The conversation follows new content while the reader is at the bottom
 * (within 100 px) and stays put once they scroll up. Browsing saved history
 * starts at the top of each page; leaving it returns to the bottom.
 * `content` lists what, when it changes, may have grown the stream. */
export function useStickyScroll({
  active,
  content,
  historyViewing,
  historyCursor,
}: {
  /** The stream is showing a conversation (not booting or switching). */
  active: boolean;
  content: unknown[];
  historyViewing: boolean;
  historyCursor: number;
}) {
  const streamRef = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const browsingHistory = useRef(false);
  const [atBottom, setAtBottom] = useState(true);

  useLayoutEffect(() => {
    if (active && stick.current)
      streamRef.current?.scrollTo({ top: streamRef.current.scrollHeight });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, ...content]);
  useLayoutEffect(() => {
    if (historyViewing) {
      stick.current = false;
      streamRef.current?.scrollTo({ top: 0 });
    } else if (browsingHistory.current) {
      stick.current = true;
      setAtBottom(true);
      streamRef.current?.scrollTo({ top: streamRef.current.scrollHeight });
    }
    browsingHistory.current = historyViewing;
  }, [historyCursor, historyViewing]);

  /** Follow new content again (after sending, opening a task…). */
  const pin = useCallback(() => {
    stick.current = true;
    setAtBottom(true);
  }, []);
  /** Stop following (paging through history). */
  const release = useCallback(() => {
    stick.current = false;
  }, []);
  const onScroll = useCallback((event: UIEvent<HTMLDivElement>) => {
    const el = event.currentTarget;
    stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 100;
    setAtBottom(stick.current);
  }, []);
  const jumpToLatest = useCallback(() => {
    setAtBottom(true);
    stick.current = true;
    streamRef.current?.scrollTo({
      top: streamRef.current.scrollHeight,
      behavior: "smooth",
    });
  }, []);
  return { streamRef, atBottom, pin, release, onScroll, jumpToLatest };
}
