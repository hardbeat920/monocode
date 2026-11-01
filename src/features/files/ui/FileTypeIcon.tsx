import { memo, useMemo, useSyncExternalStore } from "react";

type Props = {
  name: string;
  isDir: boolean;
  isOpen?: boolean;
  isRoot?: boolean;
  size?: number;
};

type IconPack = typeof import("react-material-icon-theme");

/**
 * The Material icon pack inlines every glyph as a component — ~1.1 MB, the
 * single largest thing in the boot chunk, for 16px decorations. Load it after
 * first paint and hold a same-sized blank until it lands, so the app starts
 * without it and nothing reflows when it arrives.
 */
let pack: IconPack | null = null;
let missingIcons: Record<string, string> = {};
let loading: Promise<void> | null = null;
const listeners = new Set<() => void>();

function loadPack() {
  if (pack || loading) return;
  loading = Promise.all([
    import("react-material-icon-theme"),
    import("./missingFolderIcons.json"),
  ]).then(([mod, missing]) => {
    missingIcons = missing.default;
    pack = mod;
    for (const listener of listeners) listener();
  });
}

function subscribe(onStoreChange: () => void) {
  listeners.add(onStoreChange);
  loadPack();
  return () => {
    listeners.delete(onStoreChange);
  };
}

function getSnapshot() {
  return pack;
}

/** Filename maps to the matching Material Icon Theme icon. */
export const FileTypeIcon = memo(function FileTypeIcon({
  name,
  isDir,
  isOpen = false,
  isRoot = false,
  size = 16,
}: Props) {
  const icons = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);

  const svg = !icons
    ? ""
    : isDir
      ? resolveFolderSvg(icons, name, isOpen, isRoot)
      : (icons.getIconSvg(resolveFileIcon(icons, name)) ?? "");
  // React compares this prop by identity. A fresh object replaces the SVG
  // subtree even when the glyph is unchanged (for example, on resize).
  const markup = useMemo(() => ({ __html: svg }), [svg]);

  if (!icons) {
    return (
      <span
        aria-hidden
        className="inline-block shrink-0"
        style={{ width: size, height: size }}
      />
    );
  }

  return (
    <span
      aria-hidden
      className="material-icon inline-block shrink-0 align-middle"
      style={{ width: size, height: size }}
      dangerouslySetInnerHTML={markup}
    />
  );
});

/**
 * The package names icons it ships no art for. Open glyphs the upstream theme
 * has come from `missingFolderIcons.json` (scripts/generate-material-icons.mjs).
 * Anything else, like `folder-redis`, falls back to the closed glyph, then to
 * the generic folder, rather than drawing nothing.
 */
function resolveFolderSvg(
  icons: IconPack,
  folderName: string,
  isOpen: boolean,
  isRoot: boolean,
): string {
  const svgFor = (icon: string) => icons.getIconSvg(icon) || missingIcons[icon];
  const icon = (open: boolean) =>
    icons.getFolderIcon({ folderName, isOpen: open, isRoot });
  return (
    svgFor(icon(isOpen)) ||
    svgFor(icon(false)) ||
    svgFor(isOpen ? "folder-open" : "folder") ||
    ""
  );
}

/**
 * The package only checks `fileExtension` when that prop is set — it does not
 * peel an extension off `fileName`. Try the full name, then compound suffixes
 * (`d.ts`, then `ts`) so `.rs` / `.toml` / `.json` resolve by compound suffix.
 */
function resolveFileIcon(icons: IconPack, fileName: string): string {
  const key = fileName.toLowerCase();
  const fromName = icons.getFileIcon({
    fileName: key,
    fallback: "",
    iconPack: "",
  });
  if (fromName) return fromName;

  for (const ext of compoundExtensions(key)) {
    const fromExt = icons.getFileIcon({
      fileExtension: ext,
      fallback: "",
      iconPack: "",
    });
    if (fromExt) return fromExt;
  }

  return "file";
}

function compoundExtensions(fileName: string): string[] {
  const parts = fileName.split(".");
  const start = parts[0] === "" ? 1 : 0;
  const exts: string[] = [];
  for (let i = start + 1; i < parts.length; i++) {
    exts.push(parts.slice(i).join("."));
  }
  return exts;
}
