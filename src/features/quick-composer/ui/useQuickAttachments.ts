import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ClipboardEvent,
  type DragEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  attachmentsFromFiles,
  attachmentsFromPaths,
  filesFromClipboard,
  MAX_ATTACHMENTS,
  pickAttachments,
  revokeAttachment,
} from "../../sessions/model/attachments";
import type { Attachment } from "../../sessions/model/session";
import {
  isFileReferenceText,
  nativeClipboardAttachments,
} from "../../../platform/tauri/clipboard";
import { storeQuickAttachments } from "../model/quickAttachments";
import {
  captureDraft,
  dropPastedText,
  insertRestoredText,
} from "../../../shared/lib/draftRestore";

function releaseCaptures(files: Attachment[]) {
  const paths = files.flatMap((file) => (file.path ? [file.path] : []));
  if (paths.length)
    void invoke("quick_composer_release_capture", { paths }).catch(
      () => undefined,
    );
}

type CapturedClipboardPaste =
  | { files: Attachment[]; warning?: string }
  | { error: unknown };

function discardCapturedPaste(paste: CapturedClipboardPaste) {
  if ("files" in paste) paste.files.forEach(revokeAttachment);
}

export function useQuickAttachments(
  supported: boolean,
  onError: (message: string | null) => void,
) {
  const [files, setFiles] = useState<Attachment[]>([]);
  const [loading, setLoading] = useState(false);
  const [dragging, setDragging] = useState(false);
  const filesRef = useRef(files);
  const loadingRef = useRef(false);
  const alive = useRef(true);
  const nativeDropAt = useRef(0);
  /** Clipboard contents captured while another collection is still running. */
  const queuedScreenshot = useRef<Promise<CapturedClipboardPaste> | null>(null);
  const supportedRef = useRef(supported);
  supportedRef.current = supported;

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      const queued = queuedScreenshot.current;
      queuedScreenshot.current = null;
      if (queued) void queued.then(discardCapturedPaste);
      filesRef.current.forEach(revokeAttachment);
      releaseCaptures(filesRef.current);
    };
  }, []);

  /** `collect` bails on the same conditions, so a paste must ask first. */
  const canCollect = useCallback(
    () => supportedRef.current && !loadingRef.current,
    [],
  );

  const collect = useCallback(
    async (read: () => Promise<Attachment[]>) => {
      if (!canCollect()) return;
      loadingRef.current = true;
      setLoading(true);
      onError(null);
      let incoming: Attachment[] = [];
      try {
        incoming = await read();
        const next = [...filesRef.current];
        const accepted: Attachment[] = [];
        for (const file of incoming) {
          if (
            next.length + accepted.length >= MAX_ATTACHMENTS ||
            [...next, ...accepted].some(
              (item) =>
                item.id === file.id || (item.path && item.path === file.path),
            )
          ) {
            revokeAttachment(file);
            continue;
          }
          accepted.push(file);
        }
        if (
          incoming.length > accepted.length &&
          next.length + accepted.length >= MAX_ATTACHMENTS
        )
          onError(`You can attach up to ${MAX_ATTACHMENTS} files.`);
        const stored = await storeQuickAttachments(accepted);
        if (!alive.current) {
          stored.forEach(revokeAttachment);
          return;
        }
        // Read the current list again: files may have been removed while loading.
        filesRef.current = [...filesRef.current, ...stored];
        setFiles(filesRef.current);
      } catch (reason) {
        incoming.forEach(revokeAttachment);
        if (alive.current)
          onError(reason instanceof Error ? reason.message : String(reason));
      } finally {
        loadingRef.current = false;
        if (alive.current) setLoading(false);
        // The clipboard read started when Paste was pressed. Queue only the
        // captured result so a later clipboard change cannot replace it.
        const queued = queuedScreenshot.current;
        queuedScreenshot.current = null;
        if (queued) {
          if (!alive.current || !supportedRef.current) {
            void queued.then(discardCapturedPaste);
          } else {
            void collect(async () => {
              const result = await queued;
              if ("error" in result) throw result.error;
              if (result.warning) onError(result.warning);
              return result.files;
            });
          }
        }
      }
    },
    [onError, canCollect],
  );

  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent(({ payload }) => {
        if (payload.type === "leave") {
          setDragging(false);
          return;
        }
        if (payload.type === "drop") {
          setDragging(false);
          nativeDropAt.current = Date.now();
          void collect(() => attachmentsFromPaths(payload.paths));
        } else setDragging(supportedRef.current && !loadingRef.current);
      })
      .then((unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      stop?.();
    };
  }, [collect]);

  const onPaste = (event: ClipboardEvent) => {
    const pasted = filesFromClipboard(event.clipboardData);
    if (!pasted.length) {
      // A webview reports a paste as text only, so a screenshot or a file
      // copied in a file manager arrives with nothing to attach; both live on
      // the native clipboard.
      if (!supported) return;
      const text = event.clipboardData.getData("text/plain");
      // Prose and whitespace alike are the webview's to insert, and cost no
      // clipboard read, spinner, or cleared error.
      if (text && !isFileReferenceText(text)) return;
      // A file URI can still be inserted by the webview. A screenshot cannot,
      // so capture it now even when attachment processing must wait.
      if (!canCollect()) {
        if (!text) {
          event.preventDefault();
          if (!queuedScreenshot.current) {
            queuedScreenshot.current = nativeClipboardAttachments("").then(
              ({ files, warning }) => ({ files, warning }),
              (error: unknown) => ({ error }),
            );
          }
        }
        return;
      }
      // A file URI becomes a chip, so it is kept out of the prompt; with no
      // text at all the paste carried an image the webview cannot see.
      event.preventDefault();
      // Captured before the read crosses an IPC hop.
      const captured = isFileReferenceText(text)
        ? captureDraft(event.target)
        : null;
      void collect(async () => {
        const { files, warning } = await nativeClipboardAttachments(text);
        if (warning) onError(warning);
        if (files.length) {
          // WebKit can insert the URI after preventDefault. The chip
          // replaces it, so the prompt must not keep that text.
          if (captured) dropPastedText(captured, text);
        } else if (captured) insertRestoredText(captured, text);
        return files;
      });
      return;
    }
    if (!supported) return;
    event.preventDefault();
    void collect(() => attachmentsFromFiles(pasted));
  };
  const onDragOver = (event: DragEvent) => {
    if (!Array.from(event.dataTransfer.types).includes("Files")) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = supported && !loading ? "copy" : "none";
    setDragging(supported && !loading);
  };
  const onDragLeave = (event: DragEvent) => {
    if (
      event.relatedTarget instanceof Node &&
      event.currentTarget.contains(event.relatedTarget)
    )
      return;
    setDragging(false);
  };
  const onDrop = (event: DragEvent) => {
    event.preventDefault();
    setDragging(false);
    if (Date.now() - nativeDropAt.current < 250) return;
    const dropped = Array.from(event.dataTransfer.files);
    if (dropped.length) void collect(() => attachmentsFromFiles(dropped));
  };
  const remove = (id: string) => {
    const removed = filesRef.current.find((file) => file.id === id);
    if (removed) {
      revokeAttachment(removed);
      releaseCaptures([removed]);
    }
    filesRef.current = filesRef.current.filter((file) => file.id !== id);
    setFiles(filesRef.current);
  };
  const clear = () => {
    filesRef.current.forEach(revokeAttachment);
    releaseCaptures(filesRef.current);
    filesRef.current = [];
    setFiles([]);
  };

  return {
    files,
    loading,
    dragging,
    remove,
    clear,
    onPaste,
    onDragOver,
    onDragLeave,
    onDrop,
    chooseFiles: () => collect(pickAttachments),
    takeScreenshot: async () => {
      let captured: string | null = null;
      try {
        await collect(async () => {
          captured = await invoke<string | null>("quick_composer_capture");
          return captured ? attachmentsFromPaths([captured]) : [];
        });
      } finally {
        // Inspection, capacity checks, and unmounting can reject a new capture.
        if (
          captured &&
          !filesRef.current.some((file) => file.path === captured)
        ) {
          await invoke("quick_composer_release_capture", {
            paths: [captured],
          }).catch(() => undefined);
        }
      }
    },
  };
}
