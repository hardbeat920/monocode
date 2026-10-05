import { useEffect } from "react";
import { pathKey } from "../../../shared/lib/paths";
import {
  remoteRequest,
  useRemoteMachines,
} from "../../connections/model/connections";
import type { HostProject } from "../../connections/model/protocol";
import {
  parseRemotePath,
  remoteProjectFor,
} from "../../connections/model/remoteProjects";
import {
  autoLinkProjects,
  localRemoteUrl,
} from "../model/projectMachines";
import {
  collectRailHomes,
  isRemoteProjectPath,
  type RecentProject,
} from "../model/recents";

const hostPathKey = (path: string) => pathKey(path.replace(/\\/g, "/"));

/**
 * Looks up the repository behind every rail project, here and on each
 * connected machine, and joins projects that are the same repository on
 * different machines. Runs when the rail or the machine list changes.
 */
export function useProjectMachineSync(recents: RecentProject[]): void {
  const { machines, loaded } = useRemoteMachines();
  const railKey = recents.map((item) => pathKey(item.path)).join("\0");
  const machineKey = machines.map((machine) => machine.id).join("\0");
  useEffect(() => {
    if (!loaded) return;
    let cancelled = false;
    const timer = setTimeout(() => {
      const rail = [...collectRailHomes(recents, "").values()];
      const found: Record<string, string | undefined> = {};
      const local = rail.filter((item) => !isRemoteProjectPath(item.path));
      const remote = new Map<string, string[]>();
      for (const item of rail) {
        const environmentId = parseRemotePath(item.path)?.environmentId;
        if (environmentId)
          remote.set(environmentId, [...(remote.get(environmentId) ?? []), item.path]);
      }
      void Promise.all([
        ...local.map(async (item) => {
          found[item.path] = await localRemoteUrl(item.path);
        }),
        ...[...remote].map(async ([environmentId, paths]) => {
          const machine = machines.find(
            (entry) => entry.environmentId === environmentId,
          );
          if (!machine) return;
          let projects: HostProject[];
          try {
            projects = await remoteRequest<HostProject[]>(
              machine.id,
              "projects.list",
            );
          } catch {
            return; // Offline machines keep what we learned before.
          }
          for (const path of paths) {
            const known = remoteProjectFor(path);
            const project = projects.find(
              (entry) =>
                entry.id === known?.projectId ||
                hostPathKey(entry.cwd) ===
                  hostPathKey(parseRemotePath(path)?.hostPath ?? ""),
            );
            if (project) found[path] = project.remoteUrl;
          }
        }),
      ]).then(() => {
        if (!cancelled) autoLinkProjects(rail, found);
      });
    }, 500);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [railKey, machineKey, loaded]);
}
