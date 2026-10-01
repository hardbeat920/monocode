import { lazy, Suspense, type ComponentType } from "react";

/** Keep optional surfaces out of the composer's startup dependency graph. */
export function lazySurface<Props extends object>(
  load: () => Promise<{ default: ComponentType<Props> }>,
) {
  const Surface = lazy(load);
  return function LazySurface(props: Props) {
    return (
      <Suspense fallback={null}>
        <Surface {...props} />
      </Suspense>
    );
  };
}
