// Ordered dithering, as the desktop's charts draw it
// (crates/monocode-gpui/src/visuals/dither.rs): a tone in [0, 1] becomes a
// share of inked 3px cells through a 4×4 Bayer matrix, so fills read as
// halftone texture instead of flat translucent colour.

/** Cell side in points. */
export const CELL = 3;
const BAYER4 = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/** Whether the cell at (col, row) is inked for `tone`; thresholds sit halfway
 *  between steps, so a tone of 0.25 inks exactly a quarter. */
export function inked(col: number, row: number, tone: number) {
  return tone > (BAYER4[(row & 3) * 4 + (col & 3)] + 0.5) / 16;
}

/** A tone rounded to the matrix's sixteen steps. */
export const step = (tone: number) => Math.round(tone * 16) / 16;

/** The centre of a cell. */
export const center = (col: number, row: number): [number, number] => [
  (col + 0.5) * CELL,
  (row + 0.5) * CELL,
];

/**
 * Cells `cols` × `rows` as one SVG path per colour, each horizontal run of a
 * colour a single rectangle, as the desktop paints a quad per run. `ink`
 * names a cell's colour, or null to leave it bare.
 */
export function cells(
  cols: number,
  rows: number,
  ink: (col: number, row: number) => string | null,
  size = CELL,
): Map<string, string> {
  const paths = new Map<string, string[]>();
  for (let row = 0; row < rows; row++) {
    let start = 0;
    let run: string | null = null;
    for (let col = 0; col <= cols; col++) {
      const next = col < cols ? ink(col, row) : null;
      if (next === run) continue;
      if (run !== null) {
        const list = paths.get(run) ?? [];
        list.push(`M${start * size} ${row * size}h${(col - start) * size}v${size}h${-(col - start) * size}z`);
        paths.set(run, list);
      }
      run = next;
      start = col;
    }
  }
  return new Map([...paths].map(([color, parts]) => [color, parts.join("")]));
}
