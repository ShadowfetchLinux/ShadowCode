/** Diff display helpers: hunks from unified diff text, line numbers,
 * side-by-side rows, and a small keyword/string/comment highlighter (no
 * dependency; enough to make code readable, not a full grammar). */

export type DiffLine = {
  kind: "add" | "del" | "ctx" | "meta";
  text: string;
  /** False when the file's last line has no line ending. */
  eol?: boolean;
};
export type Hunk = {
  id?: string;
  header: string;
  lines: DiffLine[];
};

/** Hunks from a unified diff body (`@@` headers, `+`/`-`/space lines). */
export function parseUnified(text: string): Hunk[] {
  const hunks: Hunk[] = [];
  for (const raw of text.split("\n")) {
    if (raw.startsWith("@@")) hunks.push({ header: raw, lines: [] });
    else if (!hunks.length || raw === "") continue;
    else if (raw.startsWith("\\")) continue;
    else {
      const kind = raw[0] === "+" ? "add" : raw[0] === "-" ? "del" : "ctx";
      hunks[hunks.length - 1].lines.push({
        kind,
        text: kind === "ctx" && raw[0] !== " " ? raw : raw.slice(1),
      });
    }
  }
  return hunks;
}

/** Old and new starting line numbers from `@@ -a,b +c,d @@`. */
export function hunkStarts(header: string): { old: number; new: number } {
  const match = /@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(header);
  return match
    ? { old: Number(match[1]), new: Number(match[2]) }
    : { old: 1, new: 1 };
}

export type NumberedLine = DiffLine & { old?: number; new?: number };

export function numbered(hunk: Hunk): NumberedLine[] {
  const start = hunkStarts(hunk.header);
  let [o, n] = [start.old, start.new];
  return hunk.lines.map((line) => {
    if (line.kind === "add") return { ...line, new: n++ };
    if (line.kind === "del") return { ...line, old: o++ };
    if (line.kind === "meta") return line;
    return { ...line, old: o++, new: n++ };
  });
}

/** Side-by-side rows: removed lines face the added lines that replaced
 * them; unchanged lines appear on both sides. */
export function splitRows(
  hunk: Hunk,
): { left?: NumberedLine; right?: NumberedLine }[] {
  const lines = numbered(hunk);
  const rows: { left?: NumberedLine; right?: NumberedLine }[] = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index];
    if (line.kind === "ctx" || line.kind === "meta") {
      rows.push({ left: line, right: line });
      index++;
      continue;
    }
    const dels: NumberedLine[] = [];
    const adds: NumberedLine[] = [];
    while (index < lines.length && lines[index].kind === "del")
      dels.push(lines[index++]);
    while (index < lines.length && lines[index].kind === "add")
      adds.push(lines[index++]);
    for (let i = 0; i < Math.max(dels.length, adds.length); i++)
      rows.push({ left: dels[i], right: adds[i] });
  }
  return rows;
}

export const countChanges = (hunks: Hunk[]) =>
  hunks.reduce(
    (sum, h) => {
      for (const line of h.lines) {
        if (line.kind === "add") sum.add++;
        else if (line.kind === "del") sum.del++;
      }
      return sum;
    },
    { add: 0, del: 0 },
  );

// ---------------------------------------------------------------------------
// Highlighting

export type Token = { text: string; kind?: "kw" | "str" | "com" | "num" };

type Language = {
  keywords: Set<string>;
  line: string[];
  block?: [string, string];
  strings: string[];
};

const words = (list: string) => new Set(list.split(" "));
const C_LIKE = { line: ["//"], block: ["/*", "*/"] as [string, string] };
const LANGUAGES: Record<string, Language> = {
  js: {
    ...C_LIKE,
    strings: ['"', "'", "`"],
    keywords: words(
      "as async await break case catch class const continue default delete do else export extends false finally for from function if import in instanceof interface let new null of return static super switch this throw true try type typeof undefined var void while yield enum implements readonly",
    ),
  },
  rust: {
    ...C_LIKE,
    strings: ['"'],
    keywords: words(
      "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while Some None Ok Err",
    ),
  },
  python: {
    line: ["#"],
    strings: ['"', "'"],
    keywords: words(
      "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield self",
    ),
  },
  go: {
    ...C_LIKE,
    strings: ['"', "`"],
    keywords: words(
      "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var nil true false",
    ),
  },
  c: {
    ...C_LIKE,
    strings: ['"', "'"],
    keywords: words(
      "auto break case char class const continue default delete do double else enum extern false float for if inline int long namespace new nullptr private protected public return short signed sizeof static struct switch template this true typedef union unsigned using virtual void volatile while bool final override public static void boolean import package",
    ),
  },
  shell: {
    line: ["#"],
    strings: ['"', "'"],
    keywords: words(
      "if then else elif fi for in do done while until case esac function return local export echo exit set",
    ),
  },
  data: {
    line: ["#"],
    strings: ['"'],
    keywords: words("true false null yes no"),
  },
};
const EXTENSIONS: Record<string, string> = {
  js: "js",
  jsx: "js",
  mjs: "js",
  cjs: "js",
  ts: "js",
  tsx: "js",
  rs: "rust",
  py: "python",
  go: "go",
  c: "c",
  h: "c",
  cc: "c",
  cpp: "c",
  hpp: "c",
  java: "c",
  kt: "c",
  cs: "c",
  swift: "c",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  json: "data",
  yaml: "data",
  yml: "data",
  toml: "data",
};

export function languageOf(path: string): string | undefined {
  const ext = path.split(".").pop()?.toLowerCase() || "";
  return EXTENSIONS[ext];
}

/** Split one line into tokens. Comments that open on an earlier line are
 * not tracked (each line is highlighted on its own). */
export function highlight(text: string, language?: string): Token[] {
  const lang = language ? LANGUAGES[language] : undefined;
  if (!lang || text.length > 2000) return [{ text }];
  const tokens: Token[] = [];
  let plain = "";
  const flush = () => {
    if (plain) tokens.push({ text: plain });
    plain = "";
  };
  let i = 0;
  while (i < text.length) {
    const rest = text.slice(i);
    const comment = lang.line.find((c) => rest.startsWith(c));
    if (comment && (comment !== "#" || i === 0 || /\s/.test(text[i - 1]))) {
      flush();
      tokens.push({ text: rest, kind: "com" });
      return tokens;
    }
    if (lang.block && rest.startsWith(lang.block[0])) {
      const end = rest.indexOf(lang.block[1], lang.block[0].length);
      const length = end < 0 ? rest.length : end + lang.block[1].length;
      flush();
      tokens.push({ text: rest.slice(0, length), kind: "com" });
      i += length;
      continue;
    }
    const quote = lang.strings.find((q) => rest.startsWith(q));
    if (quote) {
      let j = 1;
      while (j < rest.length && rest[j] !== quote) j += rest[j] === "\\" ? 2 : 1;
      const length = Math.min(j + 1, rest.length);
      flush();
      tokens.push({ text: rest.slice(0, length), kind: "str" });
      i += length;
      continue;
    }
    const word = /^[A-Za-z_$][\w$]*/.exec(rest)?.[0];
    if (word) {
      if (lang.keywords.has(word)) {
        flush();
        tokens.push({ text: word, kind: "kw" });
      } else plain += word;
      i += word.length;
      continue;
    }
    const number = /^\d[\d_.xXa-fA-F]*/.exec(rest)?.[0];
    if (number && !/[\w$]/.test(text[i - 1] || "")) {
      flush();
      tokens.push({ text: number, kind: "num" });
      i += number.length;
      continue;
    }
    plain += text[i];
    i++;
  }
  flush();
  return tokens;
}
