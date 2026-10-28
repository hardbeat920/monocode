import { memo } from "react";
import { SvgXml } from "react-native-svg";
import {
  getFileIcon,
  getFolderIcon,
  getIconSvg,
} from "react-material-icon-theme";

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
  const svg = getIconSvg(icon) ?? getIconSvg(isDir ? "folder" : "file");
  if (!svg) return null;
  return <SvgXml xml={svg} width={size} height={size} />;
});

/**
 * The package only checks `fileExtension` when that prop is set — it does not
 * peel an extension off `fileName`. Try the full name, then compound suffixes
 * (`d.ts`, then `ts`), the same as the desktop.
 */
export function resolveFileIcon(fileName: string): string {
  const key = fileName.toLowerCase();
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
