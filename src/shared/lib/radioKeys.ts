/**
 * Arrow-key step for a radio group: the option an arrow key moves to from
 * `current`, wrapping at both ends, or null for any other key.
 */
export function radioArrowStep<T>(
  key: string,
  options: readonly T[],
  current: T,
): T | null {
  const delta =
    key === "ArrowRight" || key === "ArrowDown"
      ? 1
      : key === "ArrowLeft" || key === "ArrowUp"
        ? -1
        : 0;
  if (!delta || options.length === 0) return null;
  const i = options.indexOf(current);
  return options[(i + delta + options.length) % options.length];
}
