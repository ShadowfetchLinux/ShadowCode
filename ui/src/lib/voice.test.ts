import { describe, expect, it } from "vitest";
import { formatSeconds, insertAtCursor } from "./voice";

describe("insertAtCursor", () => {
  it("fills an empty composer", () => {
    expect(insertAtCursor("", 0, 0, "Fix the parser.")).toEqual({
      value: "Fix the parser.",
      caret: 15,
    });
  });

  it("appends after existing text with a space", () => {
    expect(insertAtCursor("Please", 6, 6, "add tests")).toEqual({
      value: "Please add tests",
      caret: 16,
    });
    // No extra space after whitespace or before punctuation.
    expect(insertAtCursor("Please ", 7, 7, "add tests").value).toBe(
      "Please add tests",
    );
    expect(insertAtCursor("Done", 4, 4, ".").value).toBe("Done.");
  });

  it("inserts in the middle and keeps words apart", () => {
    const next = insertAtCursor("Fix bug", 4, 4, "the");
    expect(next.value).toBe("Fix the bug");
    expect(next.caret).toBe(8); // after the added space
    expect(insertAtCursor("ab", 1, 1, "X").value).toBe("a X b");
  });

  it("replaces the selection", () => {
    const next = insertAtCursor("Fix the old bug", 8, 11, "parser");
    expect(next.value).toBe("Fix the parser bug");
    expect(next.caret).toBe(14);
  });

  it("keeps line breaks from voice commands and clamps bad ranges", () => {
    expect(insertAtCursor("Title", 5, 5, "\nBody").value).toBe("Title\nBody");
    expect(insertAtCursor("abc", 99, 2, "d").value).toBe("abc d");
    expect(insertAtCursor("abc", -3, -1, "z").value).toBe("z abc");
  });
});

describe("formatSeconds", () => {
  it("shows minutes and seconds", () => {
    expect(formatSeconds(0)).toBe("0:00");
    expect(formatSeconds(7.9)).toBe("0:07");
    expect(formatSeconds(125)).toBe("2:05");
    expect(formatSeconds(-1)).toBe("0:00");
  });
});
