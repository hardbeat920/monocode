import fs from "node:fs";
import { SearchQuery } from "@codemirror/search";

const examples = [
  ["foo(?=bar)", "foobar foofoo", [[0, 3]], null],
  ["(?<=foo)bar", "foobar foofoo", [[3, 6]], null],
  ["(foo)\\1", "foobar foofoo", [[7, 13]], null],
  ["(?<=a+)b", "aaab ab", [[3, 4], [6, 7]], null],
  ["(?<=f+)oo", "foobar foofoo", [[1, 3], [8, 10], [11, 13]], null],
  ["(?<=prefix:)(\\w+)\\1", "prefix:foofoo suffix:barbar", [[7, 13]], "foo [foofoo]"],
  ["\\w+", "é foo ٣", [[3, 6]], null],
  ["\\d+", "٣ 3", [[3, 4]], null],
  ["\\s", "a\uFEFFb\u0085c", [[1, 4]], null],
];
const output = [];
const retainedEditor = [];
const byteRange = (text, from, to) => [
  Buffer.byteLength(text.slice(0, from)),
  Buffer.byteLength(text.slice(0, to)),
];
for (const [pattern, text, expected, replacement] of examples) {
  const query = new SearchQuery({ search: pattern, regexp: true, literal: true, replace: "$1 [$&]" });
  const cursor = query.getCursor(text);
  const matches = [];
  while (!cursor.next().done) matches.push(cursor.value);
  const ranges = matches.map(match => byteRange(text, match.from, match.to));
  if (JSON.stringify(ranges) !== JSON.stringify(expected)) throw new Error(`CodeMirror differs for ${pattern}`);
  const expanded = replacement === null ? null : query.create().getReplacement(matches[0]);
  if (expanded !== replacement) throw new Error("CodeMirror capture replacement differs");
  retainedEditor.push({ pattern, text, ranges, expanded });
}
const literalCases = [
  ["café", "café cafe\u0301", [[0, 5], [6, 12]], [true, true]],
  ["ff", "ﬀ ff", [[0, 3], [4, 6]], [true, true]],
  ["A", "Ａ A", [[0, 3], [4, 5]], [true, true]],
  ["f", "ﬀ ff", [[0, 3], [4, 5], [5, 6]], [false, true, true]],
];
const retainedLiteral = [];
for (const [search, text, expected, precise] of literalCases) {
  const query = new SearchQuery({ search, caseSensitive: true, literal: true, replace: "X" });
  const cursor = query.getCursor(text);
  const matches = [];
  while (!cursor.next().done) matches.push(cursor.value);
  const ranges = matches.map(match => byteRange(text, match.from, match.to));
  if (JSON.stringify(ranges) !== JSON.stringify(expected)) throw new Error(`CodeMirror literal differs for ${search}`);
  if (JSON.stringify(matches.map(match => match.precise)) !== JSON.stringify(precise)) throw new Error("CodeMirror match precision differs");
  retainedLiteral.push({ search, text, ranges, precise });
}
for (const flags of ["gmu", "gmui", "gu", "gui"]) {
  for (const [pattern, text, expected, replacement] of examples) {
    const matches = [...text.matchAll(new RegExp(pattern, flags))];
    const ranges = matches.map(match => [
      Buffer.byteLength(text.slice(0, match.index)),
      Buffer.byteLength(text.slice(0, match.index + match[0].length)),
    ]);
    if (JSON.stringify(ranges) !== JSON.stringify(expected)) {
      throw new Error(`Unexpected retained search result for ${pattern}/${flags}`);
    }
    let expanded = null;
    if (replacement !== null) {
      expanded = matches[0][0].replace(new RegExp("(\\w+)\\1", "u"), "$1 [$&]");
      if (expanded !== replacement) throw new Error("Unexpected capture replacement");
    }
    output.push({ pattern, text, flags, ranges, expanded });
  }
  let invalid = false;
  try { new RegExp("(?i)foo", flags); } catch { invalid = true; }
  if (!invalid) throw new Error("JavaScript accepted a Rust-only inline flag");
  output.push({ pattern: "(?i)foo", flags, invalid });
}
const result = {
  node: process.version,
  v8: process.versions.v8,
  editorFlags: ["gmu", "gmui"],
  previewFlags: ["gu", "gui"],
  cases: output,
  retainedEditor,
  retainedLiteral,
};
const text = `${JSON.stringify(result, null, 2)}\n`;
if (process.argv[2]) fs.writeFileSync(process.argv[2], text);
else process.stdout.write(text);
