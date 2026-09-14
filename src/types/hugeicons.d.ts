/**
 * Ambient declarations for deep imports of @hugeicons/core-free-icons.
 *
 * The package's `exports` map points each subpath to
 * `./dist/types/<IconName>.d.ts`, but those individual `.d.ts` files are
 * not actually shipped — only the root `dist/types/index.d.ts` exists.
 * Declare each deep import as a default-export of the same SVG-data tuple
 * shape that `IconSvgElement` (from `@hugeicons/react`) uses, so the chrome
 * icons in `src/chrome/icons.tsx` type-check without bringing in the full
 * 5 MB catalog.
 */
declare module "@hugeicons/core-free-icons/*" {
  type IconSvgElement = readonly (readonly [string, { readonly [key: string]: string | number }])[];
  const Icon: IconSvgElement;
  export default Icon;
}
