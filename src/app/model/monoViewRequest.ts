/** Tracks the latest open request and which Mono may be safely canceled. */
export type MonoViewRequestState = {
  current: number;
  pending?: { id: number; monoId: string };
};

/** Starts an open and records its Mono for targeted close or reset handling. */
export function beginMonoViewRequest(
  state: MonoViewRequestState,
  monoId: string,
): number {
  const id = ++state.current;
  state.pending = { id, monoId };
  return id;
}

/** Whether an asynchronous open may still replace the current view. */
export function isMonoViewRequestCurrent(
  state: MonoViewRequestState,
  id: number,
): boolean {
  return state.current === id;
}

/** Clears the pending marker only when this request is still the recorded one. */
export function finishMonoViewRequest(
  state: MonoViewRequestState,
  id: number,
): void {
  if (state.pending?.id === id) state.pending = undefined;
}

/** Invalidates normal closes and same-Mono opens, preserving a different target. */
export function cancelMonoViewRequest(
  state: MonoViewRequestState,
  removedMonoId?: string,
): void {
  if (removedMonoId && state.pending && state.pending.monoId !== removedMonoId)
    return;
  state.current += 1;
  state.pending = undefined;
}

/** Applies a successful reset without canceling a pending open for another Mono. */
export function resetMonoViewAfterSessionReset(
  state: MonoViewRequestState,
  monoViewId: string | null,
  sessionId: string,
  monoId: string,
  replaceView: () => void,
): void {
  if (monoViewId !== sessionId) return;
  cancelMonoViewRequest(state, monoId);
  replaceView();
}
