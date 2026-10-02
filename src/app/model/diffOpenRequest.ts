export type ResolveDiffPath = (
  cwd: string,
  path: string,
) => Promise<string | undefined>;

/**
 * Open diffs in click order. Exact paths open at once; shortened ones wait on
 * the project file index, and any click made meanwhile wins over the lookup.
 */
export function createDiffOpenRequests(resolve: ResolveDiffPath) {
  let latest = 0;
  return (
    cwd: string,
    path: string | undefined,
    exact: boolean,
    open: (path: string | undefined) => void,
  ) => {
    const request = ++latest;
    if (!path || exact) {
      open(path);
      return;
    }
    void resolve(cwd, path).then((resolved) => {
      if (request === latest) open(resolved ?? path);
    });
  };
}
