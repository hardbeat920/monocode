import { useEffect, useState } from "react";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";

import type { ModelCatalog } from "./types";

// The last catalog seen per project or session, so a page pushed from another
// that had it shows at once while it refreshes.
const lastCatalog = new Map<string, ModelCatalog>();

/**
 * A project's models from the desktop, with what a new session there, or the
 * given conversation, runs with: the last ones seen until they arrive.
 */
export function useModelCatalog(
  projectId: string | undefined,
  conversationId?: string,
) {
  const { listModels } = useWorkspace();
  const desktop = useConnection().pairing?.desktop.fingerprint;
  const key = `${desktop}\n${projectId}\n${conversationId ?? ""}`;
  const [loaded, setLoaded] = useState<{
    key: string;
    catalog: ModelCatalog;
  } | null>(null);
  const [failure, setFailure] = useState<{
    key: string;
    message: string;
  } | null>(null);
  const catalog = projectId
    ? loaded?.key === key
      ? loaded.catalog
      : (lastCatalog.get(key) ?? null)
    : null;
  const error = failure?.key === key ? failure.message : null;

  useEffect(() => {
    if (!projectId) return;
    let cancelled = false;
    listModels(projectId, conversationId).then(
      (next) => {
        lastCatalog.set(key, next);
        if (cancelled) return;
        setLoaded({ key, catalog: next });
        setFailure(null);
      },
      (reason) => {
        if (!cancelled)
          setFailure({
            key,
            message: reason instanceof Error ? reason.message : String(reason),
          });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [projectId, conversationId, key, listModels]);

  return { catalog, error };
}
