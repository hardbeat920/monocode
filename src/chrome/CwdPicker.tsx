import { ChevronRight } from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { basename, pickFolder } from "../lib/fs";
import { prettyCwd, prettyParent } from "../lib/paths";
import {
  looksLikeProject,
  sameProjectPath,
  type RecentProject,
} from "../lib/recents";
import { useLockOverscroll } from "../lib/useLockOverscroll";
import { ProjectLogoIcon } from "./ProjectLogoIcon";
import { MOD } from "../lib/platform";

type Props = {
  cwd: string;
  recents: RecentProject[];
  projectLogoPath?: string | null;
  enabled?: boolean;
  onCwdChange: (path: string) => void;
  onNewTerminal?: () => void;
  onClose?: () => void;
};

const MENU_WIDTH = 288;
const PREVIEW = 5;
const SUBMENU_GAP = 4;
const HOVER_CLOSE_MS = 100;

type Row =
  | { kind: "recent"; path: string }
  | { kind: "more" }
  | { kind: "new-project" }
  | { kind: "new-terminal" };

function menuStyle(anchor: DOMRect): CSSProperties {
  const width = Math.min(MENU_WIDTH, window.innerWidth - 16);
  const left = Math.min(
    Math.max(8, anchor.left),
    window.innerWidth - width - 8,
  );
  return {
    position: "fixed",
    left,
    bottom: window.innerHeight - anchor.top + 6,
    width,
    maxHeight: Math.min(360, anchor.top - 12),
    zIndex: 50,
  };
}

function submenuStyle(anchor: DOMRect): CSSProperties {
  const width = Math.min(MENU_WIDTH, window.innerWidth - 16);
  let left = anchor.right + SUBMENU_GAP;
  if (left + width > window.innerWidth - 8) {
    left = anchor.left - width - SUBMENU_GAP;
  }
  const maxHeight = Math.min(320, window.innerHeight - 16);
  let top = anchor.top - 4;
  if (top + maxHeight > window.innerHeight - 8) {
    top = window.innerHeight - maxHeight - 8;
  }
  return {
    position: "fixed",
    left: Math.max(8, left),
    top: Math.max(8, top),
    width,
    maxHeight,
    zIndex: 51,
  };
}

export function CwdPicker({
  cwd,
  recents,
  projectLogoPath,
  enabled = true,
  onCwdChange,
  onNewTerminal,
  onClose,
}: Props) {
  const [open, setOpen] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [menu, setMenu] = useState<CSSProperties>();
  const [submenu, setSubmenu] = useState<CSSProperties>();
  const root = useRef<HTMLDivElement>(null);
  const moreRef = useRef<HTMLButtonElement>(null);
  const closeMoreTimer = useRef<number | null>(null);
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  const inProject = looksLikeProject(cwd);
  const label = prettyCwd(cwd);
  const otherRecents = useMemo(
    () =>
      recents.filter((item) => !inProject || !sameProjectPath(item.path, cwd)),
    [cwd, inProject, recents],
  );
  const previewRecents = otherRecents.slice(0, PREVIEW);
  const overflowRecents = otherRecents.slice(PREVIEW);
  const hasMore = overflowRecents.length > 0;

  const rows = useMemo((): Row[] => {
    const out: Row[] = previewRecents.map((item) => ({
      kind: "recent",
      path: item.path,
    }));
    if (hasMore) out.push({ kind: "more" });
    out.push({ kind: "new-project" });
    if (onNewTerminal) out.push({ kind: "new-terminal" });
    return out;
  }, [hasMore, onNewTerminal, previewRecents]);

  const dismiss = (restore = false) => {
    setOpen(false);
    setMoreOpen(false);
    setActive(0);
    if (closeMoreTimer.current != null) {
      window.clearTimeout(closeMoreTimer.current);
      closeMoreTimer.current = null;
    }
    if (restore) onCloseRef.current?.();
  };

  const openMore = () => {
    if (closeMoreTimer.current != null) {
      window.clearTimeout(closeMoreTimer.current);
      closeMoreTimer.current = null;
    }
    setMoreOpen(true);
  };

  const scheduleCloseMore = () => {
    if (closeMoreTimer.current != null)
      window.clearTimeout(closeMoreTimer.current);
    closeMoreTimer.current = window.setTimeout(() => {
      closeMoreTimer.current = null;
      setMoreOpen(false);
    }, HOVER_CLOSE_MS);
  };

  const openMenu = (anchor: DOMRect) => {
    setMenu(menuStyle(anchor));
    setOpen(true);
  };

  useLayoutEffect(() => {
    if (!open || !root.current) return;
    const place = () => {
      const rect = root.current?.getBoundingClientRect();
      if (rect) setMenu(menuStyle(rect));
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [open]);

  useLayoutEffect(() => {
    if (!moreOpen || !moreRef.current) {
      setSubmenu(undefined);
      return;
    }
    const place = () => {
      const rect = moreRef.current?.getBoundingClientRect();
      if (rect) setSubmenu(submenuStyle(rect));
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [moreOpen]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent) => {
      const target = e.target as Node | null;
      if (root.current?.contains(target)) return;
      if ((target as HTMLElement | null)?.closest("[data-cwd-submenu]")) return;
      dismiss(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      dismiss(true);
    };
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  const pick = (row: Row) => {
    if (row.kind === "more") return;
    dismiss(true);
    if (row.kind === "new-project") {
      void pickFolder("Open project").then((path) => {
        if (path) onCwdChange(path);
      });
      return;
    }
    if (row.kind === "new-terminal") {
      onNewTerminal?.();
      return;
    }
    onCwdChange(row.path);
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (!enabled) return;
    if (!open) {
      if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        const rect = root.current?.getBoundingClientRect();
        if (rect) openMenu(rect);
      }
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      const row = rows[active + 1];
      if (row?.kind === "more") openMore();
      setActive((i) => Math.min(rows.length - 1, i + 1));
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => Math.max(0, i - 1));
    }
    if (e.key === "ArrowRight") {
      const row = rows[active];
      if (row?.kind === "more") {
        e.preventDefault();
        openMore();
      }
    }
    if (e.key === "ArrowLeft") {
      if (moreOpen) {
        e.preventDefault();
        setMoreOpen(false);
      }
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const row = rows[active];
      if (row?.kind === "more") {
        openMore();
        return;
      }
      if (row) pick(row);
    }
  };

  const newProjectIndex = previewRecents.length + (hasMore ? 1 : 0);
  const newTerminalIndex = onNewTerminal ? newProjectIndex + 1 : -1;
  const moreIndex = hasMore ? previewRecents.length : -1;

  return (
    <div ref={root} className="relative min-w-0">
      <button
        type="button"
        title={cwd}
        aria-label={`Project ${label}`}
        aria-expanded={open}
        aria-haspopup="menu"
        disabled={!enabled}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => {
          if (!enabled) return;
          if (open) {
            dismiss(true);
            return;
          }
          const rect = root.current?.getBoundingClientRect();
          if (rect) openMenu(rect);
        }}
        onKeyDown={onKeyDown}
        className={`flex min-w-0 items-center gap-1.5 ${
          open ? "text-content" : "text-content/50 hover:text-content"
        } disabled:opacity-40`}
      >
        <ProjectLogoIcon path={projectLogoPath} fallbackStrokeWidth={1.5} />
        <span className="truncate font-mono text-[12px]">{label}</span>
      </button>
      {open && menu ? (
        <div
          role="menu"
          aria-label="Project picker"
          data-cwd-picker
          style={menu}
          className="flex flex-col overflow-hidden rounded-lg border border-content/10 bg-content/10 shadow-xl backdrop-blur-xl"
        >
          <div
            ref={lockOverscroll}
            className="min-h-0 flex-1 overflow-y-auto overscroll-none py-1"
          >
            {inProject ? (
              <>
                <p className="px-2.5 pb-1 pt-2 text-[10px] uppercase tracking-widest text-content/50">
                  Current project
                </p>
                <div className="px-2.5 py-1.5 text-content/50">
                  <p className="truncate text-[13px] text-content">
                    {basename(cwd)}
                  </p>
                  <p className="truncate font-mono text-[11px]">
                    {prettyParent(cwd)}
                  </p>
                </div>
              </>
            ) : null}
            {previewRecents.length > 0 ? (
              <>
                <p className="px-2.5 pb-1 pt-2 text-[10px] uppercase tracking-widest text-content/50">
                  Recent projects
                </p>
                {previewRecents.map((item, index) => (
                  <button
                    key={item.path}
                    type="button"
                    role="menuitem"
                    title={item.path}
                    onMouseDown={(e) => e.stopPropagation()}
                    onMouseEnter={() => {
                      setMoreOpen(false);
                      setActive(index);
                    }}
                    onClick={() => pick({ kind: "recent", path: item.path })}
                    className={`flex w-full items-center justify-between gap-3 px-2.5 py-2 text-left ${
                      active === index
                        ? "bg-content/10 text-content"
                        : "text-content/80 hover:bg-content/5"
                    }`}
                  >
                    <span className="min-w-0 truncate text-[13px]">
                      {basename(item.path)}
                    </span>
                    <span className="max-w-28 shrink-0 truncate font-mono text-[11px] text-content/45">
                      {prettyParent(item.path)}
                    </span>
                  </button>
                ))}
              </>
            ) : null}
            {hasMore ? (
              <button
                ref={moreRef}
                type="button"
                role="menuitem"
                aria-haspopup="menu"
                aria-expanded={moreOpen}
                onMouseDown={(e) => e.stopPropagation()}
                onMouseEnter={() => {
                  setActive(moreIndex);
                  openMore();
                }}
                onMouseLeave={scheduleCloseMore}
                onFocus={() => {
                  setActive(moreIndex);
                  openMore();
                }}
                className={`flex w-full items-center justify-between gap-3 px-2.5 py-2 text-left ${
                  active === moreIndex || moreOpen
                    ? "bg-content/10 text-content"
                    : "text-content/80 hover:bg-content/5"
                }`}
              >
                <span className="text-[13px]">More Projects</span>
                <ChevronRight
                  className="size-3.5 shrink-0"
                  strokeWidth={1.75}
                />
              </button>
            ) : null}
          </div>
          <div className="shrink-0 border-t border-content/10 py-1">
            <button
              type="button"
              role="menuitem"
              onMouseDown={(e) => e.stopPropagation()}
              onMouseEnter={() => {
                setMoreOpen(false);
                setActive(newProjectIndex);
              }}
              onClick={() => pick({ kind: "new-project" })}
              className={`flex w-full items-center justify-between gap-3 px-2.5 py-2 text-left ${
                active === newProjectIndex
                  ? "bg-content/10 text-content"
                  : "text-content/80 hover:bg-content/5"
              }`}
            >
              <span className="text-[13px]">New project</span>
              <span className="shrink-0 font-mono text-[11px] text-content/45">
                {MOD}N
              </span>
            </button>
            {onNewTerminal ? (
              <button
                type="button"
                role="menuitem"
                onMouseDown={(e) => e.stopPropagation()}
                onMouseEnter={() => {
                  setMoreOpen(false);
                  setActive(newTerminalIndex);
                }}
                onClick={() => pick({ kind: "new-terminal" })}
                className={`flex w-full items-center justify-between gap-3 px-2.5 py-2 text-left ${
                  active === newTerminalIndex
                    ? "bg-content/10 text-content"
                    : "text-content/80 hover:bg-content/5"
                }`}
              >
                <span className="text-[13px]">New terminal</span>
                <span className="shrink-0 font-mono text-[11px] text-content/45">
                  {MOD}`
                </span>
              </button>
            ) : null}
          </div>
        </div>
      ) : null}
      {open && moreOpen && submenu ? (
        <div
          role="menu"
          aria-label="More projects"
          data-cwd-submenu
          style={submenu}
          className="overflow-y-auto overscroll-none rounded-lg border border-content/10 bg-content/10 py-1 shadow-xl backdrop-blur-xl"
          onMouseEnter={openMore}
          onMouseLeave={scheduleCloseMore}
        >
          {overflowRecents.map((item) => (
            <button
              key={item.path}
              type="button"
              role="menuitem"
              title={item.path}
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => pick({ kind: "recent", path: item.path })}
              className="flex w-full items-center justify-between gap-3 px-2.5 py-2 text-left text-content/80 hover:bg-content/5 hover:text-content"
            >
              <span className="min-w-0 truncate text-[13px]">
                {basename(item.path)}
              </span>
              <span className="max-w-28 shrink-0 truncate font-mono text-[11px] text-content/45">
                {prettyParent(item.path)}
              </span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
