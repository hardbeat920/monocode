/** Whether a removed session still owns the displayed Mono view. */
export function shouldCloseMonoViewAfterSessionRemoval(
  monoViewId: string | null,
  sessionId: string,
): boolean {
  return monoViewId === sessionId;
}

/** Closes the removed view while preserving pending opens for a different Mono. */
export function closeMonoViewAfterSessionRemoval(
  monoViewId: string | null,
  sessionId: string,
  monoForSession: (sessionId: string) => { id: string } | undefined,
  closeMonoView: (removedMonoId?: string) => void,
): void {
  if (shouldCloseMonoViewAfterSessionRemoval(monoViewId, sessionId))
    closeMonoView(monoForSession(sessionId)?.id);
}
