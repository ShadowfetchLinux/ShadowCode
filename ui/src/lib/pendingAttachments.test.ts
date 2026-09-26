import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import {
  MAX_PENDING,
  pendingAttachments,
  usePendingAttachments,
} from "./pendingAttachments";

afterEach(() => pendingAttachments.clear());

const element = (n: number) => ({
  kind: "element" as const,
  label: `button ${n}`,
  text: `Element ${n}`,
});

describe("pending attachments", () => {
  it("adds, dedupes, removes and caps", () => {
    const first = pendingAttachments.add(element(1));
    expect(pendingAttachments.add(element(1))).toBe(first);
    pendingAttachments.add(element(2));
    expect(pendingAttachments.list().map((i) => i.text)).toEqual([
      "Element 1",
      "Element 2",
    ]);
    pendingAttachments.remove(first);
    expect(pendingAttachments.list()).toHaveLength(1);
    for (let n = 3; n < 3 + MAX_PENDING; n++)
      pendingAttachments.add(element(n));
    expect(pendingAttachments.list()).toHaveLength(MAX_PENDING);
    expect(pendingAttachments.list()[0].text).toBe("Element 3");
  });

  it("take empties the store and restore puts items back once", () => {
    pendingAttachments.add(element(1));
    const taken = pendingAttachments.take();
    expect(taken).toHaveLength(1);
    expect(pendingAttachments.list()).toEqual([]);
    pendingAttachments.add(element(2));
    pendingAttachments.restore(taken);
    pendingAttachments.restore(taken);
    expect(pendingAttachments.list().map((i) => i.text)).toEqual([
      "Element 1",
      "Element 2",
    ]);
  });

  it("re-renders the composer when items change", () => {
    const { result } = renderHook(() => usePendingAttachments());
    expect(result.current).toEqual([]);
    act(() => {
      pendingAttachments.add({
        kind: "console",
        label: "Console error",
        text: "boom",
      });
    });
    expect(result.current.map((i) => i.label)).toEqual(["Console error"]);
    act(() => pendingAttachments.clear());
    expect(result.current).toEqual([]);
  });
});
