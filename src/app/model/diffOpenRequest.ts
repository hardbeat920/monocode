export type ResolveDiffPath = (
  cwd: string,
  path: string,
) => Promise<string | undefined>;

/**
 * Open diffs in click order. Exact paths open at once; shortened ones wait on
 * the project file index, and any click made meanwhile wins over the lookup.
 * Views opened elsewhere (all changes, a commit) call `cancel` to win too.
 */
export function createDiffOpenRequests(resolve: ResolveDiffPath) {
  let latest = 0;
  return {
    open(
      cwd: string,
      path: string | undefined,
      exact: boolean,
      open: (path: string | undefined) => void,
    ) {
      const request = ++latest;
      if (!path || exact) {
        open(path);
        return;
      }
      void resolve(cwd, path).then((resolved) => {
        if (request === latest) open(resolved ?? path);
      });
    },
    cancel() {
      latest++;
    },
  };
}
