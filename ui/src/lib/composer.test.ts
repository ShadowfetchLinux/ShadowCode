import { beforeEach, describe, expect, it } from "vitest";
import {
  insertMention,
  mentionAt,
  mentionToken,
  mentionsInText,
  removeMentionText,
} from "./mentions";
import {
  HISTORY_LIMIT,
  pushHistory,
  readHistory,
  stepHistory,
} from "./promptHistory";
import { purposeFor, readEffort, supportsEffort, writeEffort } from "./effort";
import {
  countChanges,
  highlight,
  languageOf,
  numbered,
  parseUnified,
  splitRows,
} from "./diff";

beforeEach(() => localStorage.clear());

describe("mentions", () => {
  it("finds the @token under the caret, not inside words", () => {
    expect(mentionAt("@", 1)).toEqual({ start: 0, end: 1, query: "" });
    expect(mentionAt("fix @src/ap please", 9)).toEqual({
      start: 4,
      end: 11,
      query: "src/ap",
    });
    expect(mentionAt("mail a@b.com", 12)).toBeNull();
    expect(mentionAt("fix @src now", 12)).toBeNull();
  });

  it("inserts a picked path and keeps chips in sync with the text", () => {
    const token = mentionAt("Explain @ap", 11)!;
    const next = insertMention("Explain @ap", token, "@src/app.ts");
    expect(next).toEqual({ text: "Explain @src/app.ts ", caret: 20 });
    const chips = [
      { path: "src/app.ts", kind: "file" as const },
      { path: "docs", kind: "dir" as const },
    ];
    expect(mentionToken(chips[1])).toBe("@docs/");
    expect(mentionsInText("Explain @src/app.ts, and @docs/", chips)).toEqual(
      chips,
    );
    // Deleting the text drops the attachment.
    expect(mentionsInText("Explain it", chips)).toEqual([]);
    expect(removeMentionText("Explain @src/app.ts now", chips[0])).toBe(
      "Explain now",
    );
  });
});

describe("prompt history", () => {
  it("keeps prompts per project, newest last, without repeats", () => {
    pushHistory("/a", "first");
    pushHistory("/a", "second");
    pushHistory("/a", "first");
    pushHistory("/b", "other");
    expect(readHistory("/a")).toEqual(["second", "first"]);
    expect(readHistory("/b")).toEqual(["other"]);
    for (let i = 0; i < HISTORY_LIMIT + 5; i++) pushHistory("/c", `p${i}`);
    expect(readHistory("/c")).toHaveLength(HISTORY_LIMIT);
  });

  it("steps older and newer, restoring the draft at the end", () => {
    const list = ["one", "two", "three"];
    let at = stepHistory(list, -1, "older", "draft")!;
    expect(at).toEqual({ index: 0, text: "three" });
    at = stepHistory(list, at.index, "older", "draft")!;
    expect(at.text).toBe("two");
    at = stepHistory(list, at.index, "newer", "draft")!;
    expect(at.text).toBe("three");
    at = stepHistory(list, at.index, "newer", "draft")!;
    expect(at).toEqual({ index: -1, text: "draft" });
    expect(stepHistory(list, -1, "newer", "draft")).toBeNull();
    expect(stepHistory(list, 2, "older", "")).toBeNull();
  });
});

describe("effort and mode", () => {
  it("remembers effort per model and only for rows that support it", () => {
    expect(readEffort("cli:codex")).toBe("default");
    writeEffort("cli:codex", "high");
    expect(readEffort("cli:codex")).toBe("high");
    expect(readEffort("cli:claude")).toBe("default");
    writeEffort("cli:codex", "default");
    expect(readEffort("cli:codex")).toBe("default");
    expect(
      supportsEffort({
        id: "x",
        provider: "p",
        group: "g",
        name: "n",
        inference: "cloud",
        availability: "ready",
        reasoning: true,
      }),
    ).toBe(true);
    expect(supportsEffort(undefined)).toBe(false);
    expect(purposeFor("plan")).toBe("planner");
    expect(purposeFor("ask")).toBe("reviewer");
    expect(purposeFor("code")).toBe("coder");
  });
});

describe("diff helpers", () => {
  const text = "@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n@@ -10,1 +10,2 @@\n x\n+y\n";
  it("parses hunks, numbers lines and pairs split rows", () => {
    const hunks = parseUnified(text);
    expect(hunks).toHaveLength(2);
    expect(countChanges(hunks)).toEqual({ add: 2, del: 1 });
    const lines = numbered(hunks[0]);
    expect(lines.map((l) => [l.old, l.new])).toEqual([
      [1, 1],
      [2, undefined],
      [undefined, 2],
      [3, 3],
    ]);
    const rows = splitRows(hunks[0]);
    expect(rows).toHaveLength(3);
    expect(rows[1].left?.text).toBe("b");
    expect(rows[1].right?.text).toBe("B");
    expect(splitRows(hunks[1])[1]).toEqual({
      left: undefined,
      right: { kind: "add", text: "y", new: 11 },
    });
  });

  it("highlights keywords, strings, comments and numbers", () => {
    expect(languageOf("src/app.tsx")).toBe("js");
    expect(languageOf("main.rs")).toBe("rust");
    expect(languageOf("notes.txt")).toBeUndefined();
    const tokens = highlight('const x = "hi" + 42; // done', "js");
    expect(tokens.find((t) => t.kind === "kw")?.text).toBe("const");
    expect(tokens.find((t) => t.kind === "str")?.text).toBe('"hi"');
    expect(tokens.find((t) => t.kind === "num")?.text).toBe("42");
    expect(tokens.at(-1)).toEqual({ text: "// done", kind: "com" });
    expect(tokens.map((t) => t.text).join("")).toBe(
      'const x = "hi" + 42; // done',
    );
    expect(highlight("plain", undefined)).toEqual([{ text: "plain" }]);
  });
});
