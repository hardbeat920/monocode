import { projectName } from "../../../shared/lib/paths";

type Project = { path: string; name: string };

/** Prefer Git names over checkout aliases, then retain the usual local default. */
export function sonarProjectSelection(
  projects: readonly Project[],
  repositoryName: string,
  cwd: string,
  remoteNames: ReadonlyMap<string, readonly string[]>,
): string {
  const name = repositoryName.trim().toLowerCase();
  const matches = (value: string) =>
    Boolean(name) && value.trim().toLowerCase() === name;
  const remoteMatches = projects.filter((project) =>
    remoteNames.get(project.path)?.some(matches),
  );
  const candidates = remoteMatches.length
    ? remoteMatches
    : projects.filter((project) =>
        [project.name, projectName(project.path)].some(matches),
      );
  return (
    candidates.find((project) => project.path === cwd)?.path ||
    candidates[0]?.path ||
    projects.find((project) => project.path === cwd)?.path ||
    projects[0]?.path ||
    cwd
  );
}
