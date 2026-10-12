import { invoke } from "@tauri-apps/api/core";
import type { Block, Session } from "../sessions/model/session";

/** Extend this union when a renderer and storage support for a new kind exist. */
export type ArtifactKind = "document";
export type Artifact = {
  id: string;
  kind: ArtifactKind;
  title: string;
  body: string;
  sourceSessionId?: string;
  sourceCwd?: string;
  createdAt: number;
  updatedAt: number;
};
export type ArtifactUpsert = Omit<Artifact, "createdAt" | "updatedAt">;
export type ArtifactCard = Pick<Artifact, "id" | "kind" | "title"> & {
  summary?: string;
};
export const ARTIFACTS_CHANGED_EVENT = "monocode:artifacts-changed";
export const ARTIFACT_DELETED_EVENT = "monocode:artifact-deleted";

export function artifactLabel(kind: ArtifactKind): string {
  switch (kind) {
    case "document":
      return "Document";
  }
}

export function getArtifact(id: string): Promise<Artifact | null> {
  return invoke("artifacts_get", { id });
}

export async function saveArtifact(
  artifact: ArtifactUpsert,
): Promise<Artifact> {
  const saved = await invoke<Artifact>("artifacts_upsert", { artifact });
  window.dispatchEvent(new Event(ARTIFACTS_CHANGED_EVENT));
  return saved;
}

export function deleteArtifact(id: string): Promise<void> {
  return invoke("artifacts_delete", { id });
}

export function artifactCards(blocks: readonly Block[]): ArtifactCard[] {
  const cards = new Map<string, ArtifactCard>();
  for (const block of blocks)
    for (const card of block.artifactCards ?? []) cards.set(card.id, card);
  return [...cards.values()];
}

/** Retries and edits update the existing reference instead of adding a card. */
export function recordArtifactCard(
  session: Session,
  turnId: string,
  card: ArtifactCard,
): Session {
  return {
    ...session,
    blocks: session.blocks.map((block) =>
      block.id === turnId
        ? {
            ...block,
            artifactCards: [
              ...(block.artifactCards ?? []).filter(
                (entry) => entry.id !== card.id,
              ),
              card,
            ],
          }
        : block,
    ),
  };
}

/** Preserve other cards and the reply, and leave untouched sessions intact. */
export function removeArtifactCard(session: Session, id: string): Session {
  let changed = false;
  const blocks = session.blocks.map((block) => {
    if (!block.artifactCards?.some((card) => card.id === id)) return block;
    changed = true;
    const { artifactCards: cards, ...rest } = block;
    const remaining = cards.filter((card) => card.id !== id);
    return remaining.length ? { ...rest, artifactCards: remaining } : rest;
  });
  return changed ? { ...session, blocks } : session;
}
