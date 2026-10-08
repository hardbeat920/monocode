import { useEffect, useState } from "react";
import { AgentMarkdown } from "../../sessions/ui/AgentMarkdown";
import {
  ARTIFACTS_CHANGED_EVENT,
  getArtifact,
  type Artifact,
} from "../artifacts";

/** Load an artifact and follow later saves, for every reader frame. */
export function useArtifact(id: string) {
  const [artifact, setArtifact] = useState<Artifact | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    let request = 0;
    setArtifact(null);
    setLoaded(false);
    setError(null);
    const refresh = () => {
      const token = ++request;
      void getArtifact(id).then(
        (saved) => {
          if (!live || token !== request) return;
          setArtifact(saved);
          setError(null);
          setLoaded(true);
        },
        () => {
          if (!live || token !== request) return;
          setError("Could not load this document.");
          setLoaded(true);
        },
      );
    };
    refresh();
    window.addEventListener(ARTIFACTS_CHANGED_EVENT, refresh);
    window.addEventListener("focus", refresh);
    return () => {
      live = false;
      window.removeEventListener(ARTIFACTS_CHANGED_EVENT, refresh);
      window.removeEventListener("focus", refresh);
    };
  }, [id]);
  return { artifact, loaded, error, setError };
}

/** Each kind owns its presentation inside the shared reader frame. */
export function ArtifactContent({
  artifact,
  onOpenFile,
}: {
  artifact: Artifact;
  onOpenFile?: (path: string) => void;
}) {
  switch (artifact.kind) {
    case "document":
      return (
        <AgentMarkdown
          text={artifact.body}
          cwd={artifact.sourceCwd}
          onOpenFile={onOpenFile}
          hardBreaks
        />
      );
  }
}
