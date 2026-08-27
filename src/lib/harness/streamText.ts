export type CollectedTextUpdate =
  | { kind: "delta"; scopeId: string; text: string }
  | { kind: "completed"; scopeId: string; text: string };

export type CollectedTextState = {
  activeScopeId: string | null;
  entries: ReadonlyMap<
    string,
    { streamed: string; completed: string | null }
  >;
};

export function createCollectedTextState(): CollectedTextState {
  return { activeScopeId: null, entries: new Map() };
}

export function applyCollectedTextUpdate(
  state: CollectedTextState,
  update: CollectedTextUpdate,
): CollectedTextState {
  const existing = state.entries.get(update.scopeId);
  const entry = existing ?? { streamed: "", completed: null };
  const entries = new Map(state.entries);
  entries.set(
    update.scopeId,
    update.kind === "delta"
      ? { ...entry, streamed: entry.streamed + update.text }
      : { ...entry, completed: update.text },
  );
  return {
    activeScopeId: existing ? state.activeScopeId : update.scopeId,
    entries,
  };
}

export type CompletedTextRelationship =
  | { kind: "already-emitted" }
  | { kind: "fallback"; text: string }
  | { kind: "extends"; suffix: string }
  | { kind: "conflict" };

export function classifyCompletedText(input: {
  streamed: string;
  completed: string;
}): CompletedTextRelationship {
  if (input.completed === input.streamed) return { kind: "already-emitted" };
  if (!input.streamed) return { kind: "fallback", text: input.completed };
  if (input.completed.startsWith(input.streamed)) {
    return {
      kind: "extends",
      suffix: input.completed.slice(input.streamed.length),
    };
  }
  return { kind: "conflict" };
}

export function selectCollectedText(state: CollectedTextState): string {
  if (!state.activeScopeId) return "";
  const entry = state.entries.get(state.activeScopeId);
  return entry?.completed ?? entry?.streamed ?? "";
}
