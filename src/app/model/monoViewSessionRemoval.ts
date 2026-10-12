export function shouldCloseMonoViewAfterSessionRemoval(
  monoViewId: string | null,
  sessionId: string,
): boolean {
  return monoViewId === sessionId;
}
