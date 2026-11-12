import { useEffect, useRef, useState } from "react";
import type { Element, RenderSite } from "../api/types";
import { useModHost, useModRenderVersion } from "../runtime/host";
import { CircleAlert } from "../../../shared/ui/icons";
import { ModElement } from "./ModElement";

/** Rough width of one 12px character, for the `bodyColumns` hint. */
const COLUMN_PX = 7;

/**
 * One mod view: asks the host for a tree, then again whenever this mod starts
 * or invalidates. A failed or stale render keeps the last tree.
 */
export function ModSurface({
  name,
  site,
  requestId,
  title,
  sessionId,
  cwd,
  focused = false,
  className = "flex min-h-0 flex-1 flex-col overflow-auto p-2",
}: {
  name: string;
  site: RenderSite;
  requestId: string;
  title: string;
  sessionId?: string;
  cwd?: string;
  focused?: boolean;
  className?: string;
}) {
  const host = useModHost();
  const ref = useRef<HTMLDivElement>(null);
  const [tree, setTree] = useState<Element | null>(null);
  const [columns, setColumns] = useState(40);
  const renderVersion = useModRenderVersion(name);
  const mod = host.mods().find((entry) => entry.manifest.name === name);
  const failed = !!mod && (mod.crashed || !!mod.error);
  const off = !mod || !mod.enabled;

  useEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const measure = () =>
      setColumns(Math.max(1, Math.floor(el.clientWidth / COLUMN_PX)));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    let cancelled = false;
    let latest = 0;
    const render = () => {
      const request = ++latest;
      host
        .render(name, site, requestId, { title, isFocused: focused, bodyColumns: columns }, sessionId)
        .then(
          (next) => {
            if (!cancelled && request === latest) setTree(next);
          },
          () => {},
        );
    };
    render();
    return () => {
      cancelled = true;
    };
  }, [host, name, site, requestId, title, focused, columns, sessionId, renderVersion]);

  useEffect(() => {
    if (focused) host.focus(name, site, requestId);
  }, [host, focused, name, site, requestId]);

  const showError =
    (failed || off) && (site === "Pane" || site === "SidebarTab");

  return (
    <div
      ref={ref}
      className={`text-[12px] leading-5 text-content ${className} ${
        showError || tree ? "" : "hidden"
      }`}
      data-mod-surface={name}
    >
      {showError ? (
        <div
          role="alert"
          className="flex items-start gap-2 p-2 text-content/55"
        >
          <CircleAlert
            className="mt-0.5 size-3.5 shrink-0 text-red-400"
            strokeWidth={1.75}
          />
          <span>
            {!failed
              ? `${name} is turned off. Turn it on in Settings › Mods.`
              : (
                <>
                  {mod?.crashed ? `${name} stopped responding.` : `${name} failed to load.`}{" "}
                  {mod?.error ? <span className="text-content/45">{mod.error} </span> : null}
                  Reload it in Settings › Mods.
                </>
              )}
          </span>
        </div>
      ) : tree ? (
        <ModElement
          node={tree}
          cwd={cwd}
          onInvoke={(callback, args) => host.invoke(name, callback, args)}
        />
      ) : null}
    </div>
  );
}

/** The `AbovePrompt` band of every running mod, above a session's composer. */
export function ModPromptBands({ sessionId }: { sessionId: string }) {
  const host = useModHost();
  const running = host
    .mods()
    .filter((mod) => mod.enabled && !mod.crashed && !mod.error);
  if (!running.length) return null;
  return (
    <>
      {running.map((mod) => (
        <ModSurface
          key={mod.manifest.name}
          name={mod.manifest.name}
          site="AbovePrompt"
          requestId={sessionId}
          sessionId={sessionId}
          title={mod.manifest.name}
          className="mb-1.5 flex min-w-0 flex-col rounded-lg border border-stroke bg-content/3 px-3 py-1.5"
        />
      ))}
    </>
  );
}
