/** A large file's colours as far as they are collected. */
export type Highlight = { palette: string[]; lines: [string, number][][] };

/** How soon to ask again, as it always was: the colours fill in four times a second. */
export const HIGHLIGHT_POLL_MS = 250;

/**
 * Collects colours as the desktop finishes them, in place, so a reply costs
 * only its own lines rather than a copy of everything so far. A reply that
 * brings nothing new hands over nothing, so the screen does not render for it.
 */
export class HighlightProgress {
  private palette: string[] = [];
  private lines: [string, number][][] = [];
  private dirty = false;

  /** How many lines are collected, which is where the next ask picks up. */
  get collected() {
    return this.lines.length;
  }

  add(value: Highlight) {
    if (value.lines.length === 0 && value.palette.length === this.palette.length) return;
    this.palette = value.palette;
    for (const line of value.lines) this.lines.push(line);
    this.dirty = true;
  }

  /** What to show now, if the last replies brought something new. */
  take(): Highlight | null {
    if (!this.dirty) return null;
    this.dirty = false;
    // The lines array is the one grown in place; the object is new, so the screen sees the change.
    return { palette: this.palette, lines: this.lines };
  }
}
