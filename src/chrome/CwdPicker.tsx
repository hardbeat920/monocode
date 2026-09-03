import { Check, ChevronDown, Plus, Search } from "./icons";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import { basename } from "../lib/fs";
import { substringPositions } from "../lib/fuzzy";
import { prettyCwd, prettyParent, projectName } from "../lib/paths";
import {
  looksLikeProject,
  sameProjectPath,
  type RecentProject,
} from "../lib/recents";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupLogo,
  resolveTabGroupMascot,
} from "../lib/tabGroups";
import { useTabGroupLogos } from "../hooks/useTabGroupLogos";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { MatchText } from "./MatchText";
import { Popover } from "./Popover";
import { pickerRowTone } from "./pickerRow";
import { ProjectLogoIcon } from "./ProjectLogoIcon";
import { ProjectMascot } from "./ProjectMascot";

type Props = {
  cwd: string;
  recents: RecentProject[];
  projectLogoPath?: string | null;
  enabled?: boolean;
  placement?: "above" | "below";
  className?: string;
  buttonClassName?: string;
  /** Chevron on the trailing edge; flips when the menu is open. */
  chevron?: boolean;
  children?: ReactNode;
  onCwdChange: (path: string) => void;
  onNewProject?: () => void;
  onClose?: () => void;
};

const MENU_WIDTH = 280;
const MENU_MIN_HEIGHT = 180;
const MENU_MAX_HEIGHT = 300;

type Row =
  | { kind: "recent"; path: string; current: boolean }
  | { kind: "new-project" };
export function CwdPicker({
  cwd,
  recents,
  projectLogoPath,
  enabled = true,
  placement = "above",
  className,
  buttonClassName,
  chevron = false,
  children,
  onCwdChange,
  onNewProject,
  onClose,
}: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const activeRef = useRef<HTMLButtonElement>(null);
  const pointer = useRef({ x: Number.NaN, y: Number.NaN, allow: false });
  const fromPointer = useRef(false);
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const onCwdChangeRef = useRef(onCwdChange);
  onCwdChangeRef.current = onCwdChange;
  const onNewProjectRef = useRef(onNewProject);
  onNewProjectRef.current = onNewProject;
  const groupLogos = useTabGroupLogos();
  const [groupMascots] = useState(loadTabGroupMascots);
  const [groupColors] = useState(loadTabGroupColors);
  const [groupCustomColors] = useState(loadTabGroupCustomColors);
  const triggerKey = projectName(cwd);
  const triggerLogoPath =
    projectLogoPath ?? resolveTabGroupLogo(triggerKey, groupLogos);
  const triggerColor = resolveTabGroupColor(
    triggerKey,
    groupColors,
    groupCustomColors,
    triggerKey,
  );
  const triggerMascot = resolveTabGroupMascot(triggerKey, groupMascots);
  const inProject = looksLikeProject(cwd);
  const label = prettyCwd(cwd);
  const filtering = query.trim().length > 0;

  const rows = useMemo((): Row[] => {
    const needle = query.trim().toLowerCase();
    const matches = (path: string) => {
      if (!needle) return true;
      return (
        basename(path).toLowerCase().includes(needle) ||
        path.toLowerCase().includes(needle)
      );
    };
    const out: Row[] = [];
    if (inProject) out.push({ kind: "recent", path: cwd, current: true });
    for (const item of recents) {
      if (inProject && sameProjectPath(item.path, cwd)) continue;
      if (!matches(item.path)) continue;
      out.push({ kind: "recent", path: item.path, current: false });
    }
    if (onNewProject) out.push({ kind: "new-project" });
    return out;
  }, [cwd, inProject, onNewProject, query, recents]);

  const dismiss = (restore = false) => {
    setOpen(false);
    setQuery("");
    setActive(0);
    if (restore) onCloseRef.current?.();
  };

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActive(0);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    search.current?.focus();
    // The popover measures on its first pass before painting; re-focus after
    // placement (and after any parent focus-restore effect) so the query wins.
    const raf = requestAnimationFrame(() => search.current?.focus());
    return () => cancelAnimationFrame(raf);
  }, [open]);

  useEffect(() => {
    setActive((i) => (rows.length === 0 ? 0 : Math.min(i, rows.length - 1)));
  }, [rows.length]);

  useEffect(() => {
    pointer.current.allow = false;
  }, [rows]);

  useEffect(() => {
    if (fromPointer.current) {
      fromPointer.current = false;
      return;
    }
    pointer.current.allow = false;
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const pick = (row: Row) => {
    if (row.kind === "new-project") {
      dismiss(true);
      onNewProjectRef.current?.();
      return;
    }
    dismiss(true);
    if (row.current) return;
    onCwdChangeRef.current(row.path);
  };
  const onListMouseMove = (e: ReactMouseEvent<HTMLDivElement>) => {
    if (e.clientX === pointer.current.x && e.clientY === pointer.current.y) {
      return;
    }
    pointer.current = { x: e.clientX, y: e.clientY, allow: true };
  };

  const onRowEnter = (index: number) => {
    if (!pointer.current.allow) return;
    fromPointer.current = true;
    setActive(index);
  };

  const onSearchKey = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (rows.length === 0) return;
      setActive((i) => Math.min(rows.length - 1, i + 1));
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (rows.length === 0) return;
      setActive((i) => Math.max(0, i - 1));
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const row = rows[active];
      if (row) pick(row);
    }
  };

  const onTriggerKey = (e: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (!enabled) return;
    if (!open) {
      if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        setOpen(true);
      }
    }
  };
  return (
    <div
      ref={root}
      className={`relative flex h-full min-w-0${className ? ` ${className}` : ""}`}
    >
      <button
        type="button"
        title={cwd}
        aria-label={`Project ${label}`}
        aria-expanded={open}
        aria-haspopup="menu"
        disabled={!enabled}
        data-tauri-drag-region="false"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => {
          if (!enabled) return;
          if (open) {
            dismiss(true);
            return;
          }
          setOpen(true);
        }}
        onKeyDown={onTriggerKey}
        className={
          buttonClassName
            ? `${buttonClassName} ${
                open ? "bg-content/10 text-content" : "hover:bg-content/5"
              } disabled:opacity-40`
            : `flex min-w-0 items-center gap-1.5 ${
                open ? "text-content" : "text-content/50 hover:text-content"
              } disabled:opacity-40`
        }
      >
        {children ?? (
          <>
            {triggerLogoPath ? (
              <ProjectLogoIcon
                path={triggerLogoPath}
                fallbackStrokeWidth={1.5}
              />
            ) : (
              <ProjectMascot
                project={triggerKey}
                color={triggerColor}
                name={triggerMascot}
                className="size-3.5 shrink-0"
              />
            )}
            <span className="truncate font-mono text-[12px]">{label}</span>
          </>
        )}
        {chevron ? (
          <ChevronDown
            className={`size-3 shrink-0 text-content/50 ${
              open ? "rotate-180" : ""
            }`}
            strokeWidth={1.75}
          />
        ) : null}
      </button>
      {open ? (
        <Popover
          anchor={root}
          side={placement === "below" ? "bottom" : "top"}
          width={MENU_WIDTH}
          minHeight={MENU_MIN_HEIGHT}
          maxHeight={MENU_MAX_HEIGHT}
          onDismiss={(reason) => dismiss(reason === "escape")}
          role="dialog"
          aria-label="Project picker"
          data-cwd-picker
          className="flex flex-col overflow-hidden"
        >
          <label className="flex shrink-0 items-center gap-2 border-b border-content/10 px-2 py-2.5 text-content/50">
            <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
            <input
              ref={search}
              type="text"
              value={query}
              placeholder="Search projects..."
              aria-label="Search projects"
              spellCheck={false}
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="off"
              className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/40"
              onChange={(e) => {
                setQuery(e.target.value);
                setActive(0);
              }}
              onKeyDown={onSearchKey}
            />
          </label>
          <div
            ref={lockOverscroll}
            role="listbox"
            aria-label="Projects"
            onMouseMove={onListMouseMove}
            className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto overscroll-none px-1.5 py-1.5"
          >
            {rows.length === 0 ? (
              <div className="px-3 py-4 text-[12px] text-content/50">
                {filtering ? "No matching projects" : "No projects"}
              </div>
            ) : (
              rows.map((row, index) => {
                const highlighted = index === active;
                if (row.kind === "new-project") {
                  return (
                    <button
                      key="new-project"
                      ref={highlighted ? activeRef : undefined}
                      type="button"
                      role="option"
                      aria-selected={false}
                      title="New project"
                      onMouseDown={(e) => e.preventDefault()}
                      onMouseEnter={() => onRowEnter(index)}
                      onClick={() => pick(row)}
                      className={`flex h-8 w-full min-w-0 scroll-my-2 items-center gap-2 rounded-md px-2 text-left ${
                        highlighted
                          ? "bg-content/10 text-content"
                          : "text-content/80 hover:bg-content/5 hover:text-content"
                      }`}
                    >
                      <Plus
                        className="size-3.5 shrink-0"
                        strokeWidth={1.75}
                      />
                      <span className="min-w-0 truncate text-[12px]">
                        New project
                      </span>
                    </button>
                  );
                }
                const selected = row.current;
                const rowKey = projectName(row.path);
                const rowLogoPath = resolveTabGroupLogo(rowKey, groupLogos);
                const rowColor = resolveTabGroupColor(
                  rowKey,
                  groupColors,
                  groupCustomColors,
                  rowKey,
                );
                const rowMascot = resolveTabGroupMascot(rowKey, groupMascots);
                return (
                  <button
                    key={row.path}
                    ref={highlighted ? activeRef : undefined}
                    type="button"
                    role="option"
                    aria-selected={selected}
                    title={row.path}
                    onMouseDown={(e) => e.preventDefault()}
                    onMouseEnter={() => onRowEnter(index)}
                    onClick={() => pick(row)}
                    className={`flex w-full scroll-my-2 items-center gap-2 rounded-lg px-2 py-1.5 text-left text-content ${pickerRowTone({ highlighted, selected })}`}
                  >
                    {selected ? (
                      <Check
                        className="size-3.5 shrink-0"
                        strokeWidth={1.75}
                      />
                    ) : rowLogoPath ? (
                      <ProjectLogoIcon
                        path={rowLogoPath}
                        className="size-3.5 shrink-0 rounded-sm"
                        imageClassName="size-3.5"
                        fallbackStrokeWidth={1.75}
                      />
                    ) : (
                      <ProjectMascot
                        project={rowKey}
                        color={rowColor}
                        name={rowMascot}
                        className="size-3.5 shrink-0"
                      />
                    )}
                    <span className="min-w-0 flex-1 truncate text-[12px]">
                      <MatchText
                        text={basename(row.path)}
                        positions={substringPositions(basename(row.path), query)}
                        active={filtering}
                      />
                    </span>
                    <span className="max-w-28 shrink-0 truncate font-mono text-[10px] text-content/40">
                      <MatchText
                        text={prettyParent(row.path)}
                        positions={substringPositions(prettyParent(row.path), query)}
                        active={filtering}
                      />
                    </span>
                  </button>
                );
              })
            )}
          </div>
        </Popover>
      ) : null}
    </div>
  );
}
