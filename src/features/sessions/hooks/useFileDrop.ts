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
}: {
  anchor: RefObject<HTMLElement | null>;
  enabled: boolean;
  /** Read at event time; native listener registration crosses IPC hops. */
  state: RefObject<FileDropState>;
  read: (read: () => Promise<Attachment[]>) => void;
  onError: (message: string) => void;
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

    const overTarget = (x: number, y: number) => {
      const root = dropRoot();
      if (!root) return false;
      const rect = root.getBoundingClientRect();
      return (
        x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
      );
    };

    const onDragOver = (event: DragEvent) => {
      const data = event.dataTransfer;
      if (!hasFiles(data)) return;
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
      const over = overTarget(detail.x, detail.y);
      const supported = state.current.attachmentsSupported;
      if (detail.type === "move") {
        setFileDrag(over && supported);
        return;
      }
      setFileDrag(false);
      if (!over || !supported) return;
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
        const over = overTarget(point.x, point.y);
        const supported = state.current.attachmentsSupported;
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setFileDrag(over && supported);
          return;
        }
        if (event.payload.type !== "drop") return;
        setFileDrag(false);
        if (!over || !supported) return;
        if (event.payload.paths.length === 0) {
          onError(
            "This drag did not provide a file. Save the image, then drag the saved file here.",
          );
          return;
        }
        nativeDropAt = Date.now();
        const paths = event.payload.paths;
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
  }, [anchor, enabled, state, read, onError]);

  useEffect(() => {
    if (!supported) setFileDrag(false);
  }, [supported]);

  return fileDrag;
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
