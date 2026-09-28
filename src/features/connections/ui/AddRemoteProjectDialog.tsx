import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { LAYER } from "../../../shared/lib/layers";
import { SearchableSelect } from "../../../shared/ui/SearchableSelect";
import { ChevronRight, Folder } from "../../../shared/ui/icons";
import {
  OPEN_CONNECTIONS_EVENT,
  remoteRequest,
  useRemoteMachines,
} from "../model/connections";
import { rememberRemoteProject } from "../model/remoteProjects";
import type { HostDirectory, HostProject } from "../model/protocol";

/** Adds a project whose folder is on a connected machine. Sessions in it run
 * on that machine; the project otherwise behaves like any other in the rail. */
export function AddRemoteProjectDialog({
  onCancel,
  onOpen,
}: {
  onCancel: () => void;
  /** Receives the new project's rail key. */
  onOpen: (key: string) => void;
}) {
  const { machines, loaded } = useRemoteMachines();
  const [machineId, setMachineId] = useState<string>();
  const machine =
    machines.find((entry) => entry.id === machineId) ?? machines[0];
  const [path, setPath] = useState("");
  const [directory, setDirectory] = useState<HostDirectory>();
  const [loading, setLoading] = useState(false);
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState("");
  const alive = useRef(true);
  // Set on every mount: development StrictMode mounts, unmounts and mounts
  // again, and responses after the first cleanup must still be shown.
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onCancel();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onCancel]);

  const browse = async (next?: string) => {
    if (!machine) return;
    setLoading(true);
    setError("");
    try {
      const value = await remoteRequest<HostDirectory>(
        machine.id,
        "projects.browse",
        { path: next },
      );
      if (!alive.current) return;
      setDirectory(value);
      setPath(value.path);
    } catch (reason) {
      if (alive.current) setError(String(reason).replace(/^Error: /, ""));
    } finally {
      if (alive.current) setLoading(false);
    }
  };

  useEffect(() => {
    setDirectory(undefined);
    setPath("");
    if (machine) void browse();
  }, [machine?.id]);

  const open = async () => {
    if (!machine || !path.trim()) return;
    setOpening(true);
    setError("");
    try {
      const project = await remoteRequest<HostProject>(
        machine.id,
        "projects.open",
        { cwd: path.trim() },
      );
      onOpen(rememberRemoteProject(machine.environmentId, project).key);
    } catch (reason) {
      if (alive.current) setError(String(reason).replace(/^Error: /, ""));
    } finally {
      if (alive.current) setOpening(false);
    }
  };

  return createPortal(
    <div className="fixed inset-0" style={{ zIndex: LAYER.dialog }}>
      <div className="absolute inset-0 bg-black/30" onMouseDown={onCancel} />
      <form
        role="dialog"
        aria-modal="true"
        aria-label="Open folder on a machine"
        onMouseDown={(event) => event.stopPropagation()}
        onSubmit={(event) => {
          event.preventDefault();
          void open();
        }}
        className="absolute left-1/2 top-[16%] flex max-h-[70vh] w-[min(480px,calc(100vw-24px))] -translate-x-1/2 flex-col gap-3 rounded-lg border border-content/10 bg-content/5 p-4 shadow-xl backdrop-blur-xl"
      >
        <div className="flex flex-col gap-1">
          <h2 className="text-[13px] font-medium leading-tight text-content">
            Open folder on a machine
          </h2>
          <p className="text-[12px] leading-snug text-content/55">
            Sessions in this project run on that machine, using its checkout and
            its Codex or Claude Code sign-in. They keep running when you close
            MonoCode here.
          </p>
        </div>
        {!loaded ? null : !machine ? (
          <>
            <p className="text-[12px] leading-snug text-content/55">
              No machines are connected yet. Add one in Settings, then open a
              folder on it here.
            </p>
            <div className="flex justify-end gap-2">
              <button
                type="button"
                onClick={onCancel}
                className="rounded-md px-3 py-1.5 text-[12px] text-content/70 hover:bg-content/8 hover:text-content"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => {
                  onCancel();
                  window.dispatchEvent(new Event(OPEN_CONNECTIONS_EVENT));
                }}
                className="rounded-md bg-selection px-3 py-1.5 text-[12px] font-medium hover:bg-selection-hover"
              >
                Add a machine
              </button>
            </div>
          </>
        ) : (
          <>
            {machines.length > 1 ? (
              <SearchableSelect
                label="Machine"
                value={machine.id}
                options={machines.map((entry) => ({
                  value: entry.id,
                  label: entry.name,
                  keywords: entry.ssh?.target ?? entry.endpoint,
                }))}
                onChange={setMachineId}
                searchable={false}
              />
            ) : (
              <p className="text-[12px] text-content/55">
                On <span className="text-content/80">{machine.name}</span>
              </p>
            )}
            <input
              aria-label="Folder path on the machine"
              className="h-8 shrink-0 rounded-md border border-content/10 bg-content/3 px-2.5 font-mono text-[12px] text-content outline-none focus:border-content/25"
              placeholder="/home/me/code/my-app"
              value={path}
              spellCheck={false}
              autoCorrect="off"
              autoCapitalize="off"
              autoComplete="off"
              onChange={(event) => setPath(event.target.value)}
            />
            <div
              aria-label="Folders"
              className="min-h-24 flex-1 overflow-y-auto overscroll-contain rounded-md border border-content/10"
            >
              <div className="p-1">
                {directory?.parent ? (
                  <FolderRow
                    name=".."
                    onOpen={() => void browse(directory.parent!)}
                  />
                ) : null}
                {directory?.entries.map((entry) => (
                  <FolderRow
                    key={entry.path}
                    name={entry.name}
                    onOpen={() => void browse(entry.path)}
                  />
                ))}
                {directory && !directory.entries.length ? (
                  <p className="px-2 py-1.5 text-[12px] text-content/45">
                    No subfolders
                  </p>
                ) : null}
                {!directory && loading ? (
                  <p className="px-2 py-1.5 text-[12px] text-content/45">
                    Loading folders…
                  </p>
                ) : null}
              </div>
            </div>
            {error ? (
              <p
                role="alert"
                className="whitespace-pre-wrap break-words text-[12px] leading-snug text-red-400/90"
              >
                {error}
              </p>
            ) : null}
            <div className="flex justify-end gap-2">
              <button
                type="button"
                onClick={onCancel}
                className="rounded-md px-3 py-1.5 text-[12px] text-content/70 hover:bg-content/8 hover:text-content"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={opening || !path.trim()}
                className="rounded-md bg-selection px-3 py-1.5 text-[12px] font-medium hover:bg-selection-hover disabled:opacity-40"
              >
                {opening ? "Opening…" : "Open"}
              </button>
            </div>
          </>
        )}
      </form>
    </div>,
    document.body,
  );
}

function FolderRow({ name, onOpen }: { name: string; onOpen: () => void }) {
  return (
    <button
      type="button"
      onClick={onOpen}
      className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] text-content/75 hover:bg-content/8 hover:text-content"
    >
      <Folder
        className="size-3.5 shrink-0 text-content/45"
        strokeWidth={1.75}
      />
      <span className="min-w-0 flex-1 truncate">{name}</span>
      <ChevronRight className="size-3 shrink-0 text-content/30" />
    </button>
  );
}
