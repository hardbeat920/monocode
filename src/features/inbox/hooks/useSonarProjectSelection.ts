import { useEffect, useMemo, useState } from "react";
import { projectRepoNames } from "../model/inboxProjectRepos";
import { sonarProjectSelection } from "../model/sonarProjectSelection";

export function useSonarProjectSelection(
  projects: { path: string; name: string }[],
  repositoryName: string,
  cwd: string,
  enabled: boolean,
) {
  const [remoteNames, setRemoteNames] = useState<Map<string, string[]>>(
    new Map(),
  );
  const paths = useMemo(
    () => projects.map((project) => project.path),
    [projects],
  );
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    void Promise.all(
      paths.map(async (path) => [path, await projectRepoNames(path)] as const),
    ).then((entries) => {
      if (!cancelled) setRemoteNames(new Map(entries));
    });
    return () => {
      cancelled = true;
    };
  }, [paths, enabled]);
  return sonarProjectSelection(projects, repositoryName, cwd, remoteNames);
}
