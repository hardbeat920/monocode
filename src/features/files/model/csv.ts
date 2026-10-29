export type DelimitedTable = {
  /** Every row padded to the widest row, so cells line up under the header. */
  rows: string[][];
  /** Rows were left out because the limit was reached. */
  truncated: boolean;
};

/**
 * Split CSV (RFC 4180) or TSV text into rows. A quoted field may hold the
 * delimiter, line breaks and doubled quotes; CRLF, a leading BOM and blank
 * lines are tolerated. Parsing stops after `limit` rows, so a large file only
 * costs what the preview shows.
 */
export function parseDelimited(
  text: string,
  delimiter: string,
  limit = Infinity,
): DelimitedTable {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  let truncated = false;
  const endRow = () => {
    row.push(field);
    field = "";
    if (row.length > 1 || row[0]) rows.push(row);
    row = [];
  };
  for (let index = text.charCodeAt(0) === 0xfeff ? 1 : 0; ; index += 1) {
    if (index >= text.length) {
      if (field || row.length) endRow();
      break;
    }
    const char = text[index];
    if (quoted) {
      if (char !== '"') field += char;
      else if (text[index + 1] === '"') {
        field += '"';
        index += 1;
      } else quoted = false;
    } else if (char === '"' && field === "") {
      quoted = true;
    } else if (char === delimiter) {
      row.push(field);
      field = "";
    } else if (char === "\n") {
      endRow();
      if (rows.length >= limit) {
        truncated = /\S/.test(text.slice(index + 1));
        break;
      }
    } else if (char !== "\r") {
      field += char;
    }
  }
  const width = Math.max(0, ...rows.map((cells) => cells.length));
  for (const cells of rows) {
    while (cells.length < width) cells.push("");
  }
  return { rows, truncated };
}

const NUMBER =
  /^[-+]?(?=\.?\d)(?:\d{1,3}(?:,\d{3})+|\d+)?(?:\.\d+)?(?:e[-+]?\d+)?%?$/i;

/**
 * Whether every filled cell in a column is a number, so the column can be
 * right-aligned like a spreadsheet. Blank cells do not count either way.
 */
export function isNumericColumn(rows: string[][], column: number): boolean {
  let filled = false;
  for (const row of rows) {
    const value = row[column]?.trim();
    if (!value) continue;
    if (!NUMBER.test(value)) return false;
    filled = true;
  }
  return filled;
}
