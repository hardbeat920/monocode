import { useEffect, useState, type RefObject } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  EXPLORER_FILE_POINTER_DRAG_EVENT,
  type ExplorerFilePointerDragDetail,
} from "../../../shared/lib/drag";
import { dragPointToClient } from "../../../shared/lib/dragPoint";
import type { Attachment } from "../model/session";
import {
  attachmentsFromFiles,
  attachmentsFromPaths,
  filesFromClipboard,
} from "../model/attachments";

type FileDropState = { attachmentsSupported: boolean; remote: boolean };

/** A waiting follow-up being edited takes drops instead of the composer. */
export type QueuedDropSink = {
  ingestFiles: (files: File[]) => void;
  ingestPaths: (paths: string[]) => void;
};

/**
 * Files dropped anywhere on the session pane around `anchor` attach to its
 * composer: from the OS (native and DOM drops) and from the file explorer.
 * Returns whether a file is being dragged over the pane.
 */
export function useFileDrop({
  anchor,
  enabled,
  state,
  read,
  onError,
  queuedSinkRef,
}: {
  anchor: RefObject<HTMLElement | null>;
  enabled: boolean;
  /** Read at event time; native listener registration crosses IPC hops. */
  state: RefObject<FileDropState>;
  read: (read: () => Promise<Attachment[]>) => void;
  onError: (message: string) => void;
  /** Set while a queued follow-up is being edited. */
  queuedSinkRef?: RefObject<QueuedDropSink | null>;
}): boolean {
  const [fileDrag, setFileDrag] = useState(false);
  const supported = state.current.attachmentsSupported;

  useEffect(() => {
    if (!enabled) {
      setFileDrag(false);
      return;
    }
    const dropRoot = () =>
      anchor.current?.closest("[data-session-drop]") as HTMLElement | null;
    let nativeDropAt = 0;
    let cancelled = false;

    const dropHit = (x: number, y: number) => {
      const root = dropRoot();
      if (!root) return null;
      return sessionDropHit(root, { x, y });
    };
    const ingestPaths = (paths: string[]) => {
      const sink = queuedSinkRef?.current;
      if (sink) {
        sink.ingestPaths(paths);
        return;
      }
      read(() => attachmentsFromPaths(paths));
    };

    const onDragOver = (event: DragEvent) => {
      const data = event.dataTransfer;
      if (!hasFiles(data)) return;
      if (isQueuedMessageEditTarget(event.target)) {
        event.preventDefault();
        data.dropEffect = state.current.attachmentsSupported ? "copy" : "none";
        setFileDrag(false);
        return;
      }
      event.preventDefault();
      const supported = state.current.attachmentsSupported;
      data.dropEffect = supported ? "copy" : "none";
      setFileDrag(supported);
    };
    const onDragLeave = (event: DragEvent) => {
      const root = dropRoot();
      if (!root) return;
      const next = event.relatedTarget as Node | null;
      if (next && root.contains(next)) return;
      setFileDrag(false);
    };
    const onDrop = (event: DragEvent) => {
      const data = event.dataTransfer;
      if (!hasFiles(data)) return;
      if (isQueuedMessageEditTarget(event.target)) {
        event.preventDefault();
        event.stopPropagation();
        setFileDrag(false);
        if (!state.current.attachmentsSupported) return;
        const files = filesFromClipboard(data);
        if (files.length === 0) return;
        queuedSinkRef?.current?.ingestFiles(files);
        return;
      }
      event.preventDefault();
      setFileDrag(false);
      if (!state.current.attachmentsSupported) return;
      if (Date.now() - nativeDropAt < 250) return;
      const files = filesFromClipboard(data);
      if (files.length === 0) return;
      read(() => attachmentsFromFiles(files));
    };

    const onExplorerFilePointerDrag = (event: Event) => {
      if (state.current.remote) return;
      const detail = (event as CustomEvent<ExplorerFilePointerDragDetail>)
        .detail;
      if (!detail || detail.type === "end") {
        setFileDrag(false);
        return;
      }
      const hit = dropHit(detail.x, detail.y);
      const supported = state.current.attachmentsSupported;
      if (detail.type === "move") {
        setFileDrag(hit === "composer" && supported);
        return;
      }
      setFileDrag(false);
      if (!hit || !supported) return;
      if (hit === "queued") {
        queuedSinkRef?.current?.ingestPaths([detail.path]);
        return;
      }
      read(() => attachmentsFromPaths([detail.path]));
    };

    const root = dropRoot();
    root?.addEventListener("dragover", onDragOver);
    root?.addEventListener("dragleave", onDragLeave);
    root?.addEventListener("drop", onDrop);
    window.addEventListener(
      EXPLORER_FILE_POINTER_DRAG_EVENT,
      onExplorerFilePointerDrag,
    );

    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (cancelled) return;
        if (event.payload.type === "leave") {
          setFileDrag(false);
          return;
        }
        const { x, y } = event.payload.position;
        const point = dragPointToClient(x, y);
        const hit = dropHit(point.x, point.y);
        const supported = state.current.attachmentsSupported;
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setFileDrag(hit === "composer" && supported);
          return;
        }
        if (event.payload.type !== "drop") return;
        setFileDrag(false);
        if (!hit || !supported) return;
        if (event.payload.paths.length === 0) {
          onError(
            "This drag did not provide a file. Save the image, then drag the saved file here.",
          );
          return;
        }
        nativeDropAt = Date.now();
        const paths = event.payload.paths;
        if (hit === "queued") {
          queuedSinkRef?.current?.ingestPaths(paths);
          return;
        }
        read(() => attachmentsFromPaths(paths));
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
      root?.removeEventListener("dragover", onDragOver);
      root?.removeEventListener("dragleave", onDragLeave);
      root?.removeEventListener("drop", onDrop);
      window.removeEventListener(
        EXPLORER_FILE_POINTER_DRAG_EVENT,
        onExplorerFilePointerDrag,
      );
      unlisten?.();
    };
  }, [anchor, enabled, state, read, onError, queuedSinkRef]);

  useEffect(() => {
    if (!supported) setFileDrag(false);
  }, [supported]);

  return fileDrag;
}

function pointInRect(rect: DOMRect, point: { x: number; y: number }): boolean {
  return (
    point.x >= rect.left &&
    point.x <= rect.right &&
    point.y >= rect.top &&
    point.y <= rect.bottom
  );
}

function isQueuedMessageEditTarget(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    Boolean(target.closest("[data-queued-message-edit]"))
  );
}

/** Which part of the session pane a drop lands on. */
function sessionDropHit(
  root: Element,
  point: { x: number; y: number },
): "queued" | "composer" | null {
  if (!pointInRect(root.getBoundingClientRect(), point)) return null;
  const queued = root.querySelector("[data-queued-message-edit]");
  if (queued && pointInRect(queued.getBoundingClientRect(), point)) {
    return "queued";
  }
  return "composer";
}

function hasFiles(data: DataTransfer | null): data is DataTransfer {
  if (!data) return false;
  return (
    data.files.length > 0 ||
    [...data.types].some(
      (type) => type === "Files" || type === "application/x-moz-file",
    ) ||
    Array.from(data.items ?? []).some((item) => item.kind === "file")
  );
}
