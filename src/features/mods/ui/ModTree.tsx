import { useEffect, useRef, type KeyboardEvent as ReactKeyboardEvent } from "react";
import type { TreeProps, TreeRow as ModTreeRow } from "../api/types";
import { basename } from "../../../platform/tauri/fs";
import { useGitFileStatuses } from "../../source-control/hooks/useGitFileStatuses";
import { FileTypeIcon } from "../../files/ui/FileTypeIcon";
import { GIT_STATUS_COLOR, TreeRow } from "../../../shared/ui/TreeRow";
import { iconByName } from "../../../shared/ui/icons";
import { Shimmer } from "../../../shared/ui/Shimmer";
import { toneColor, useModRender } from "./ModElement";

/** Keys the tree handles itself; everything else goes to the mod's `onKey`. */
const OWN_KEYS = new Set(["ArrowUp", "ArrowDown", "Enter", " ", "Tab"]);
const MODIFIER_KEYS = new Set(["Shift", "Control", "Alt", "Meta"]);

/** A mod's `Tree`: explorer rows without FileTree's editing. */
export function ModTree({ props }: { props: TreeProps }) {
  const { invoke, cwd } = useModRender();
  const root = useRef<HTMLDivElement>(null);
  const rows = Array.isArray(props.rows) ? props.rows.filter(isRow) : [];
  const hasPaths = rows.some((row) => typeof row.path === "string");
  const git = useGitFileStatuses(cwd ?? "", hasPaths);
  const liveKey = rows
    .filter((row) => row.live)
    .map((row) => row.id)
    .join("\0");

  const rowNode = (id: string | undefined) =>
    id == null
      ? null
      : (Array.from(
          root.current?.querySelectorAll<HTMLElement>("[data-mod-row]") ?? [],
        ).find((el) => el.dataset.modRow === id) ?? null);

  useEffect(() => {
    rowNode(props.reveal)?.scrollIntoView({ block: "nearest" });
  }, [props.reveal]);

  useEffect(() => {
    const live = liveKey.split("\0").filter(Boolean);
    rowNode(live[live.length - 1])?.scrollIntoView({ block: "nearest" });
  }, [liveKey]);

  const onKeyDown = (event: ReactKeyboardEvent<HTMLElement>, id: string) => {
    const mods = { ctrl: event.ctrlKey || event.metaKey, shift: event.shiftKey };
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const all = Array.from(
        root.current?.querySelectorAll<HTMLElement>("[data-mod-row]") ?? [],
      );
      const index = all.findIndex((el) => el.dataset.modRow === id);
      all[index + (event.key === "ArrowDown" ? 1 : -1)]?.focus();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      invoke(props.onDoublePress, [id]);
      return;
    }
    if (event.key === " ") {
      event.preventDefault();
      invoke(props.onPress, [id, mods]);
      return;
    }
    if (OWN_KEYS.has(event.key) || MODIFIER_KEYS.has(event.key)) return;
    if (props.onKey == null) return;
    event.preventDefault();
    invoke(props.onKey, [event.key, mods]);
  };

  return (
    <div ref={root} role="tree" className="flex min-w-0 flex-col">
      {rows.map((row) => {
        const path = typeof row.path === "string" ? row.path : undefined;
        const dir = row.kind === "dir";
        const status = path
          ? dir
            ? git.dirs.get(path)
            : git.files.get(path)
          : undefined;
        const Icon = iconByName(row.icon);
        const live = toneColor(row.live);
        const color = live ?? toneColor(row.tone);
        const label = String(row.label ?? "");
        return (
          <TreeRow
            key={row.id}
            data-mod-row={row.id}
            depth={Math.max(0, Math.min(Number(row.depth) || 0, 32))}
            selected={props.active === row.id}
            open={dir ? !!row.open : undefined}
            aria-level={(Number(row.depth) || 0) + 1}
            aria-selected={props.active === row.id}
            aria-expanded={dir ? !!row.open : undefined}
            title={path ?? label}
            icon={
              path ? (
                <FileTypeIcon
                  name={basename(path)}
                  isDir={dir}
                  isOpen={!!row.open}
                />
              ) : Icon ? (
                <Icon
                  className="size-4"
                  strokeWidth={1.75}
                  style={{ color: toneColor(row.tone) }}
                />
              ) : null
            }
            onClick={(event) =>
              invoke(props.onPress, [
                row.id,
                { ctrl: event.ctrlKey || event.metaKey, shift: event.shiftKey },
              ])
            }
            onDoubleClick={() => invoke(props.onDoublePress, [row.id])}
            onContextMenu={(event) => {
              if (props.onContextMenu == null) return;
              event.preventDefault();
              invoke(props.onContextMenu, [row.id, { shift: event.shiftKey }]);
            }}
            onKeyDown={(event) => onKeyDown(event, row.id)}
          >
            <span
              style={{ color }}
              className={`min-w-0 flex-1 truncate leading-label ${
                !color && status ? (GIT_STATUS_COLOR[status] ?? "") : ""
              }`}
            >
              {live ? <Shimmer>{label}</Shimmer> : label}
            </span>
            {Array.isArray(row.right)
              ? row.right.map((seg, index) => (
                  <span
                    key={index}
                    style={{ color: toneColor(seg?.color) }}
                    className={`shrink-0 pl-1.5 text-[12px] tabular-nums ${
                      seg?.color ? "" : "text-content/45"
                    } ${seg?.dimColor ? "opacity-60" : ""}`}
                  >
                    {String(seg?.text ?? "")}
                  </span>
                ))
              : null}
          </TreeRow>
        );
      })}
    </div>
  );
}

function isRow(value: unknown): value is ModTreeRow {
  const row = value as ModTreeRow | null;
  return !!row && typeof row === "object" && typeof row.id === "string";
}
