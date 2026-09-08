import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { homeDir, listDir, type FsEntry } from "../lib/fs";
import { LAYER } from "../lib/layers";
import { displayPath } from "../lib/paths";

type Props = {
  open: boolean;
  initialCwd: string;
  onSelect: (path: string) => void;
  onClose: () => void;
};

/**
 * Host-side project picker for companion mode, where the native folder
 * sheet would open on the wrong device. Browses host directories through
 * `list_dir` (which already routes remotely) instead of `pickFolder`.
 */
export function RemoteProjectPicker({
  open,
  initialCwd,
  onSelect,
  onClose,
}: Props) {
  const [cwd, setCwd] = useState(initialCwd);
  const [entries, setEntries] = useState<FsEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setError(null);
    setLoading(true);
    let cancelled = false;
    void (async () => {
      try {
        const start =
          initialCwd && initialCwd !== "~" ? initialCwd : await homeDir();
        if (cancelled) return;
        setCwd(start);
        setEntries(await listDir(start));
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open, initialCwd]);

  const navigate = useCallback(async (path: string) => {
    setError(null);
    setLoading(true);
    try {
      setEntries(await listDir(path));
      setCwd(path);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  if (!open) return null;

  const parent =
    cwd === "/" ? null : cwd.replace(/\/+$/, "").split("/").slice(0, -1).join("/") || "/";
  const dirs = entries.filter((entry) => entry.isDir);

  return createPortal(
    <div className="fixed inset-0" style={{ zIndex: LAYER.dialog }}>
      <div className="absolute inset-0" onMouseDown={onClose} />
      <div
        role="dialog"
        aria-label="Open project on host"
        onMouseDown={(e) => e.stopPropagation()}
        className="absolute left-1/2 top-[12%] flex max-h-[70vh] w-[min(560px,calc(100vw-24px))] -translate-x-1/2 flex-col overflow-hidden rounded-lg border border-content/10 bg-content/5 backdrop-blur-xl"
      >
        <div className="border-b border-content/10 px-3 py-2.5">
          <div className="text-[13px] font-medium text-content">
            Open project on host
          </div>
          <p className="mt-0.5 truncate font-mono text-[12px] text-content/50">
            {displayPath(cwd)}
          </p>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto overscroll-none px-1.5 py-1.5">
          {loading ? (
            <p className="px-2 py-3 text-[12px] text-content/50">Loading…</p>
          ) : error ? (
            <p className="px-2 py-3 text-[12px] text-red-400">{error}</p>
          ) : (
            <>
              {parent ? (
                <Row label=".." onClick={() => void navigate(parent)} />
              ) : null}
              {dirs.map((entry) => (
                <Row
                  key={entry.path}
                  label={entry.name}
                  onClick={() => void navigate(entry.path)}
                />
              ))}
              {dirs.length === 0 && !parent ? (
                <p className="px-2 py-3 text-[12px] text-content/50">
                  No folders here
                </p>
              ) : null}
            </>
          )}
        </div>
        <div className="flex gap-2 border-t border-content/10 p-2">
          <button
            type="button"
            onClick={onClose}
            className="min-h-10 flex-1 rounded-md px-3 py-2 text-[13px] text-content/60 hover:bg-content/10 hover:text-content"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={() => onSelect(cwd)}
            className="min-h-10 flex-1 rounded-md bg-content px-3 py-2 text-[13px] font-medium text-background-base"
          >
            Open this folder
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

function Row({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex min-h-10 w-full items-center gap-2 rounded-md px-2 text-left text-sm text-content hover:bg-content/10"
    >
      <span className="text-content/50">▸</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
    </button>
  );
}
