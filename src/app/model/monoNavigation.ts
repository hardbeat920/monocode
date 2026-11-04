/** Run a Mono navigation only while it is current and the settings editor can leave. */
export async function navigateMono<T>(
  request: { current: number },
  leave: () => Promise<boolean>,
  load: () => Promise<T | undefined>,
  commit: (value: T) => void,
): Promise<boolean> {
  const id = ++request.current;
  if (!(await leave()) || id !== request.current) return false;
  const value = await load();
  if (value === undefined || id !== request.current) return false;
  if (!(await leave()) || id !== request.current) return false;
  commit(value);
  return true;
}
