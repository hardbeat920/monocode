import { effortLabel } from "@/features/models/modelChoice";
import { useSessionChoice } from "@/features/models/sessionChoice";
import { useModelCatalog } from "@/features/models/useModelCatalog";
import type { Mono } from "@/features/workspace/types";
import { monoConversationId } from "@/features/workspace/WorkspaceProvider";

/**
 * The model a Mono runs with, named as a session's: a pick made on this phone,
 * else what the desktop says its conversation runs with, else the catalog's.
 */
export function useMonoModel(mono: Mono | undefined) {
  const conversationId = mono ? monoConversationId(mono.id) : undefined;
  const { catalog } = useModelCatalog(conversationId, conversationId);
  const { chosen } = useSessionChoice(undefined, conversationId);
  const current = chosen?.pick ?? catalog?.current;
  const entry = catalog?.models.find((model) => model.id === current?.model);
  return {
    conversationId,
    model: chosen?.name ?? entry?.name ?? current?.model ?? mono?.model ?? "",
    harness: current?.harness ?? mono?.harness,
    effort: current ? effortLabel(entry, current) : undefined,
  };
}
