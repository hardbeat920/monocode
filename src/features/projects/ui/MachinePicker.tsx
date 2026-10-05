import { useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { Check, Computer, Internet, Plus } from "../../../shared/ui/icons";
import { Popover } from "../../../shared/ui/Popover";
import { useRemoteMachines } from "../../connections/model/connections";
import {
  locationFolder,
  locationLabel,
  useProjectLocations,
} from "../model/projectMachines";
import { isRemoteProjectPath, sameProjectPath } from "../model/recents";

const MENU_WIDTH = 288;
const SELF = "[data-machine-picker]";

type Row =
  | { kind: "location"; path: string }
  | { kind: "add"; where: "remote" | "local" };

/**
 * "Run on": which machine a new session runs on. Lists the project's folder
 * on each machine, plus ways to add one. Read-only once the session starts.
 */
export function MachinePicker({
  cwd,
  enabled,
  onChange,
  onAdd,
  onClose,
}: {
  /** The session's project folder, on whichever machine it is. */
  cwd: string;
  enabled: boolean;
  onChange: (path: string) => void;
  onAdd: (where: "remote" | "local") => void;
  onClose?: () => void;
}) {
  const locations = useProjectLocations(cwd);
  const { machines } = useRemoteMachines();
  const root = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  if (locations.length < 2 && machines.length === 0) return null;
  const current = locations.find((path) => sameProjectPath(path, cwd)) ?? cwd;
  const label = locationLabel(current, machines);
  const rows: Row[] = [
    ...locations.map((path) => ({ kind: "location" as const, path })),
    ...(machines.length > locations.filter(isRemoteProjectPath).length
      ? [{ kind: "add" as const, where: "remote" as const }]
      : []),
    ...(locations.some((path) => !isRemoteProjectPath(path))
      ? []
      : [{ kind: "add" as const, where: "local" as const }]),
  ];
  const interactive = enabled && rows.length > 1;

  const dismiss = (refocus: boolean) => {
    setOpen(false);
    if (refocus) onClose?.();
  };
  const pick = (row: Row) => {
    dismiss(true);
    if (row.kind === "add") onAdd(row.where);
    else if (!sameProjectPath(row.path, current)) onChange(row.path);
  };
  const onKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (!interactive) return;
    if (!open) {
      if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        setActive(Math.max(0, rows.findIndex((row) => row.kind === "location" && sameProjectPath(row.path, current))));
        setOpen(true);
      }
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((index) => Math.min(rows.length - 1, index + 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((index) => Math.max(0, index - 1));
    } else if (event.key === "Enter") {
      event.preventDefault();
      const row = rows[active];
      if (row) pick(row);
    }
  };
  const Icon = isRemoteProjectPath(current) ? Internet : Computer;
  const firstAdd = rows.findIndex((row) => row.kind === "add");

  return (
    <div ref={root} className="relative flex h-full min-w-0 shrink-0">
      <button
        type="button"
        title={
          interactive
            ? `Runs on ${label}: ${locationFolder(current)}`
            : `Runs on ${label}`
        }
        aria-label={`Run on ${label}`}
        aria-expanded={open}
        aria-haspopup={interactive ? "menu" : undefined}
        disabled={!interactive}
        data-tauri-drag-region="false"
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          if (!interactive) return;
          if (open) dismiss(true);
          else {
            setActive(Math.max(0, rows.findIndex((row) => row.kind === "location" && sameProjectPath(row.path, current))));
            setOpen(true);
          }
        }}
        onKeyDown={onKeyDown}
        className={`flex min-w-0 max-w-40 items-center gap-1.5 ${
          open ? "text-content" : "text-content/50 enabled:hover:text-content"
        }`}
      >
        <Icon className="size-3.5 shrink-0" strokeWidth={1.5} />
        <span className="truncate text-[12px]">{label}</span>
      </button>
      {open ? (
        <Popover
          anchor={root}
          side="top"
          width={MENU_WIDTH}
          maxHeight={320}
          ignore={SELF}
          onDismiss={(reason) => dismiss(reason === "escape")}
          role="menu"
          aria-label="Run on"
          data-machine-picker
          className="overflow-y-auto overscroll-none py-1"
        >
          <p className="px-2.5 pb-1 pt-2 text-[10px] uppercase tracking-widest text-content/50">
            Run on
          </p>
          {rows.map((row, index) => (
            <button
              key={row.kind === "location" ? row.path : `add:${row.where}`}
              type="button"
              role="menuitem"
              onMouseDown={(event) => event.preventDefault()}
              onMouseEnter={() => setActive(index)}
              onClick={() => pick(row)}
              className={`flex w-full items-center gap-2 px-2.5 py-2 text-left ${
                index === firstAdd ? "mt-1 border-t border-stroke pt-2.5" : ""
              } ${index === active ? "bg-content/5 text-content" : "text-content/80"}`}
            >
              {row.kind === "location" ? (
                <>
                  {isRemoteProjectPath(row.path) ? (
                    <Internet className="size-3.5 shrink-0 text-content/50" strokeWidth={1.5} />
                  ) : (
                    <Computer className="size-3.5 shrink-0 text-content/50" strokeWidth={1.5} />
                  )}
                  <span className="min-w-0 shrink-0 truncate text-[13px]">
                    {locationLabel(row.path, machines)}
                  </span>
                  <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-content/45">
                    {locationFolder(row.path)}
                  </span>
                  {sameProjectPath(row.path, current) ? (
                    <Check className="size-3.5 shrink-0" strokeWidth={2} />
                  ) : null}
                </>
              ) : (
                <>
                  <Plus className="size-3.5 shrink-0 text-content/50" strokeWidth={1.5} />
                  <span className="text-[13px]">
                    {row.where === "remote"
                      ? "Add on another machine…"
                      : "Add folder on this computer…"}
                  </span>
                </>
              )}
            </button>
          ))}
        </Popover>
      ) : null}
    </div>
  );
}
