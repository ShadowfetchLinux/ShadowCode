#!/usr/bin/env node
// Fails when a tracked file contains something that looks like a real
// credential. Test fixtures use short fake values ("sk-or-good") that don't
// match these patterns.
//
//   node scripts/check-secrets.mjs                  # scan tracked files
//   node scripts/check-secrets.mjs --staged         # scan the staged diff too
//   node scripts/check-secrets.mjs --value-file F   # also look for the exact
//                                                   # value in F (never printed)
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const PATTERNS = [
  ["OpenRouter API key", /sk-or-v1-[0-9a-f]{48,}/],
  ["Anthropic API key", /sk-ant-(?:api|admin)\d{2}-[A-Za-z0-9_-]{60,}/],
  ["OpenAI API key", /sk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{20}T3BlbkFJ[A-Za-z0-9_-]{20,}/],
  ["OpenAI project key", /sk-proj-[A-Za-z0-9_-]{80,}/],
  ["Google API key", /AIza[0-9A-Za-z_-]{35}/],
  ["xAI API key", /xai-[A-Za-z0-9]{60,}/],
  ["GitHub token", /\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{60,})\b/],
  // A real key body is hundreds of characters; short fixtures don't match.
  [
    "Private key block",
    /-----BEGIN (?:RSA |EC |DSA |OPENSSH |ENCRYPTED )?PRIVATE KEY-----[A-Za-z0-9+/=\s]{200,}-----END/,
  ],
];
const FORBIDDEN_FILES = [/(^|\/)secrets\.env$/, /(^|\/)\.env(\.(?!example$)[^/]+)?$/];

const args = process.argv.slice(2);
const valueFile = args.includes("--value-file")
  ? args[args.indexOf("--value-file") + 1]
  : null;
const exact = valueFile ? readFileSync(valueFile, "utf8").trim() : "";

const git = (...a) =>
  execFileSync("git", a, { encoding: "utf8", maxBuffer: 1 << 30 });
const files = git("ls-files", "-z").split("\0").filter(Boolean);
const problems = [];

for (const file of files) {
  if (FORBIDDEN_FILES.some((re) => re.test(file)))
    problems.push(`${file}: secrets file is tracked`);
  let text;
  try {
    const buf = readFileSync(file);
    if (buf.length > 20_000_000 || buf.includes(0)) continue; // binary
    text = buf.toString("utf8");
  } catch {
    continue; // deleted in the working tree
  }
  for (const [name, re] of PATTERNS) {
    const match = text.match(re);
    if (match) {
      const line = text.slice(0, match.index).split("\n").length;
      problems.push(`${file}:${line}: looks like a ${name}`);
    }
  }
  if (exact && exact.length >= 16 && text.includes(exact))
    problems.push(`${file}: contains the value from ${valueFile}`);
}

if (args.includes("--staged")) {
  const diff = git("diff", "--cached", "-U0", "--no-color");
  for (const [name, re] of PATTERNS)
    if (re.test(diff)) problems.push(`staged changes: look like a ${name}`);
  if (exact && exact.length >= 16 && diff.includes(exact))
    problems.push(`staged changes: contain the value from ${valueFile}`);
}

if (problems.length) {
  console.error(`Possible secrets found (values not shown):\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
console.log(`No secrets found in ${files.length} tracked files.`);
