import { memo } from "react";
import { parse, SvgAst } from "react-native-svg";
import {
  getFileIcon,
  getFolderIcon,
  getIconSvg,
} from "react-material-icon-theme";

/** Each glyph's SVG parsed once, and each file name's glyph looked up once:
 *  a list of tens of thousands of files shares about a hundred of them, and a
 *  recycled row shouldn't parse XML again as it scrolls. */
const parsed = new Map<string, ReturnType<typeof parse>>();
const byName = new Map<string, string>();

function glyph(icon: string, fallback: string) {
  let ast = parsed.get(icon);
  if (ast === undefined) {
    const svg = getIconSvg(icon) ?? getIconSvg(fallback);
    try {
      ast = svg ? parse(svg) : null;
    } catch {
      ast = null;
    }
    parsed.set(icon, ast);
  }
  return ast;
}

/**
 * The desktop explorer's file icons (src/features/files/ui/FileTypeIcon.tsx):
 * the Material Icon Theme picks a glyph by name, drawn here from its SVG.
 */
export const FileTypeIcon = memo(function FileTypeIcon({
  name,
  isDir,
  isOpen = false,
  size = 18,
}: {
  name: string;
  isDir: boolean;
  isOpen?: boolean;
  size?: number;
}) {
  const icon = isDir
    ? getFolderIcon({ folderName: name, isOpen, isRoot: false })
    : resolveFileIcon(name);
  const ast = glyph(icon, isDir ? "folder" : "file");
  if (!ast) return null;
  return <SvgAst ast={ast} override={{ width: size, height: size }} />;
});

/**
 * The package only checks `fileExtension` when that prop is set — it does not
 * peel an extension off `fileName`. Try the full name, then compound suffixes
 * (`d.ts`, then `ts`), the same as the desktop.
 */
export function resolveFileIcon(fileName: string): string {
  const key = fileName.toLowerCase();
  const known = byName.get(key);
  if (known) return known;
  const icon = lookupFileIcon(key);
  // Bounded, as file names in a large repository are many.
  if (byName.size > 5000) byName.clear();
  byName.set(key, icon);
  return icon;
}

function lookupFileIcon(key: string): string {
  const fromName = getFileIcon({ fileName: key, fallback: "", iconPack: "" });
  if (fromName) return fromName;

  for (const ext of compoundExtensions(key)) {
    const fromExt = getFileIcon({ fileExtension: ext, fallback: "", iconPack: "" });
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
