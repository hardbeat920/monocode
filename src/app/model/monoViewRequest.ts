export type MonoViewRequestState = {
  current: number;
  pending?: { id: number; monoId: string };
};

export function beginMonoViewRequest(
  state: MonoViewRequestState,
  monoId: string,
): number {
  const id = ++state.current;
  state.pending = { id, monoId };
  return id;
}

export function isMonoViewRequestCurrent(
  state: MonoViewRequestState,
  id: number,
): boolean {
  return state.current === id;
}

export function finishMonoViewRequest(
  state: MonoViewRequestState,
  id: number,
): void {
  if (state.pending?.id === id) state.pending = undefined;
}

export function cancelMonoViewRequest(
  state: MonoViewRequestState,
  removedMonoId?: string,
): void {
  if (removedMonoId && state.pending && state.pending.monoId !== removedMonoId)
    return;
  state.current += 1;
  state.pending = undefined;
}
