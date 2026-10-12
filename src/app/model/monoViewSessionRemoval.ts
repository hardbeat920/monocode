export function shouldCloseMonoViewAfterSessionRemoval(
  monoViewId: string | null,
  sessionId: string,
): boolean {
  return monoViewId === sessionId;
}

export function closeMonoViewAfterSessionRemoval(
  monoViewId: string | null,
  sessionId: string,
  monoForSession: (sessionId: string) => { id: string } | undefined,
  closeMonoView: (removedMonoId?: string) => void,
): void {
  if (shouldCloseMonoViewAfterSessionRemoval(monoViewId, sessionId))
    closeMonoView(monoForSession(sessionId)?.id);
}
