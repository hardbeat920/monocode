// core-free-icons 4.3.3 exports per-glyph JS, but only ships a combined type
// catalog. Keep deep imports without loading that catalog during development.
declare module "@hugeicons/core-free-icons/*" {
  const icon: import("@hugeicons/react").IconSvgElement;
  export default icon;
}
