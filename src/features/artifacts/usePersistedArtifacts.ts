import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import {
  ARTIFACTS_CHANGED_EVENT,
  ARTIFACT_DELETED_EVENT,
  listArtifacts,
  type Artifact,
} from "./artifacts";

export function usePersistedArtifacts(): readonly Artifact[] {
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  useEffect(() => {
    let active = true;
    let sequence = 0;
    const refresh = () => {
      const request = ++sequence;
      void listArtifacts()
        .then((items) => {
          if (active && request === sequence) setArtifacts(items);
        })
        .catch(() => {});
    };
    window.addEventListener(ARTIFACTS_CHANGED_EVENT, refresh);
    const listening = listen<string>(
      ARTIFACT_DELETED_EVENT,
      ({ payload: id }) => {
        if (!active) return;
        setArtifacts((current) =>
          current.filter((artifact) => artifact.id !== id),
        );
        // Supersede reads that started before the delete committed. A later
        // refresh can then pick up a document saved again with the same ID.
        refresh();
      },
    );
    refresh();
    return () => {
      active = false;
      window.removeEventListener(ARTIFACTS_CHANGED_EVENT, refresh);
      void listening.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);
  return artifacts;
}
