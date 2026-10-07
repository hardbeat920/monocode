import { lazy, Suspense, type ComponentType } from "react";

/** Keep optional surfaces out of the composer's startup dependency graph. */
export function lazySurface<Props extends object>(
  load: () => Promise<{ default: ComponentType<Props> }>,
  { suspense = true }: { suspense?: boolean } = {},
) {
  let pending: ReturnType<typeof load> | undefined;
  const preload = () => {
    pending ??= load().catch((error: unknown) => {
      pending = undefined;
      throw error;
    });
    return pending;
  };
  const Surface = lazy(preload);
  function LazySurface(props: Props) {
    // Navigation uses the app's existing boundary so a transition keeps the
    // current screen visible until the destination has loaded.
    if (!suspense) return <Surface {...props} />;
    return (
      <Suspense fallback={null}>
        <Surface {...props} />
      </Suspense>
    );
  }
  return Object.assign(LazySurface, { preload });
}
