import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import {
  mobileCatalog,
  mobileConversation,
  mobileModels,
  mobileProjectChanges,
  mobileProjectWorktrees,
  mobileProjectFiles,
  mobileProjectFile,
  mobileProjectGit,
  mobileProjectGitAction,
  mobileProjectSearch,
  mobileFileHighlight,
} from "../model/mobileWorkspace";
import {
  turnsBefore,
  watchUpdate,
  watchWindow,
  type WatchBaseline,
} from "../model/mobileWatch";
import type { RuntimeMode, Session } from "../../sessions/model/session";
import { mobileAccountUsage } from "../model/mobileAccountUsage";

type Request = {
  id: string;
  commandId: string;
  method: string;
  params: Record<string, unknown>;
};
type WatchEvent =
  | {
      id: string;
      action: "start";
      conversationId: string;
      /** How many of the latest turns to open on. */
      tail?: number | null;
      /** The first turn the phone already has, to carry on from instead. */
      from?: string | null;
      incremental?: boolean;
    }
  | { id: string; action: "stop" };
/** At most this often while a reply streams, so the phone gets ~20 frames a second. */
const PUSH_INTERVAL_MS = 50;
/** Turns a watch opens on when the phone does not say; older ones page in. */
const DEFAULT_TAIL = 3;
const MAX_PAGE = 100;
/** How long a git action may run before the phone is told to ask again; under
 *  the desktop's 25 second wait for a reply. */
const GIT_ACTION_WAIT_MS = 20_000;
type Options = {
  sessions: readonly Session[];
  /** The permission mode a new desktop tab would start with. */
  defaultRuntimeMode?: RuntimeMode;
  command: (
    method: string,
    params: Record<string, unknown>,
    commandId: string,
  ) => Promise<unknown>;
};
export function useMobileWorkspace(options: Options) {
  const current = useRef(options);
  current.current = options;
  const wake = useRef<(() => void) | null>(null);
  /** The sessions a watch on a conversation has read from disk, if one is open. */
  const savedFor = useRef<(conversationId: string) => Map<string, Session> | undefined>(
    () => undefined,
  );
  // Every session change, a streamed token included, may be news for a phone.
  useEffect(() => wake.current?.(), [options.sessions]);
  useEffect(() => {
    let disposed = false;
    const watches = new Map<
      string,
      {
        conversationId: string;
        baseline: WatchBaseline | null;
        tail: number;
        incremental: boolean;
        /** The first turn the watch carries; the window grows from here. */
        from: string | null;
        /** Sessions read from disk, kept for the watch's lifetime. */
        saved: Map<string, Session>;
      }
    >();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let pushing = false;
    let again = false;
    const push = async () => {
      timer = undefined;
      if (pushing) {
        again = true;
        return;
      }
      pushing = true;
      for (const [id, watch] of [...watches]) {
        let frame: ReturnType<typeof watchUpdate>;
        try {
          const started = performance.now();
          let older = false;
          const conversation = await mobileConversation(
            watch.conversationId,
            current.current.sessions,
            watch.saved,
            (turnIds) => {
              const window = watchWindow(turnIds, watch.from, watch.tail);
              watch.from = window.from;
              older = window.older;
              return [window.start, turnIds.length];
            },
          );
          frame = watchUpdate(
            watch.baseline,
            {
              busy: conversation.busy,
              older,
              turns: conversation.turns,
              question: conversation.question,
            },
            watch.incremental,
          );
          if (!watch.baseline && frame)
            console.info(
              `[mobile] first frame for ${watch.conversationId}: ${Math.round(performance.now() - started)}ms, ` +
                `${conversation.turns.length} turns, ${JSON.stringify(frame.update).length} bytes`,
            );
        } catch (error) {
          frame = {
            update: {
              error: error instanceof Error ? error.message : String(error),
            },
            baseline: watch.baseline ?? {
              busy: false,
              older: false,
              question: "null",
              order: [],
              turns: new Map(),
            },
          };
        }
        if (!frame || disposed || !watches.has(id)) continue;
        try {
          await invoke("mobile_push", { id, update: frame.update });
          watch.baseline = frame.baseline;
        } catch (error) {
          // "busy": the phone is behind; the next push sends from the old baseline.
          if (String(error) === "gone") watches.delete(id);
          else if (String(error) === "busy") again = true;
        }
      }
      pushing = false;
      if (again && !disposed) {
        again = false;
        schedule();
      }
    };
    const schedule = () => {
      if (watches.size > 0 && timer === undefined)
        timer = setTimeout(() => void push(), PUSH_INTERVAL_MS);
    };
    wake.current = schedule;
    savedFor.current = (conversationId) =>
      [...watches.values()].find((watch) => watch.conversationId === conversationId)?.saved;
    const listening = listen<WatchEvent>(
      "monocode-mobile-watch",
      ({ payload }) => {
        if (payload.action === "start") {
          watches.set(payload.id, {
            conversationId: payload.conversationId,
            incremental: payload.incremental === true,
            baseline: null,
            tail: Math.min(MAX_PAGE, Math.max(1, payload.tail ?? DEFAULT_TAIL)),
            from: payload.from ?? null,
            saved: new Map(),
          });
          // The phone waits for its first frame; send it straight away.
          clearTimeout(timer);
          timer = undefined;
          void push();
        } else watches.delete(payload.id);
      },
    );
    return () => {
      disposed = true;
      wake.current = null;
      clearTimeout(timer);
      void listening.then((unlisten) => unlisten());
    };
  }, []);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const receipts = new Map<
      string,
      { signature: string; result: Promise<unknown> }
    >();
    // Git actions by command ID. Generating a message or pushing can outlast a
    // phone request, so the job keeps running and the phone asks again with the
    // same ID until it is done; asking again never runs it twice.
    const gitJobs = new Map<
      string,
      { signature: string; result: Promise<unknown> }
    >();
    // Publish in order: an older poll must not overwrite the catalog that makes
    // a newly created session routable before its ID is returned to the phone.
    let publication = Promise.resolve();
    const publishCatalog = () => {
      const next = publication.then(async () => {
        if (disposed) return;
        const catalog = await mobileCatalog(
          current.current.sessions,
          current.current.defaultRuntimeMode,
        );
        if (!disposed) await invoke("mobile_publish", { catalog });
      });
      publication = next.catch(() => {});
      return next;
    };
    const publish = async () => {
      try {
        const status = await invoke<{ enabled: boolean }>("mobile_status");
        if (status.enabled && !disposed) await publishCatalog();
      } catch (error) {
        if (!disposed) console.error("Mobile workspace publish failed", error);
      }
      if (!disposed) timer = setTimeout(() => void publish(), 3000);
    };
    const listening = listen<Request>(
      "monocode-mobile-request",
      ({ payload }) => {
        void (async () => {
          let response: unknown;
          try {
            let result: unknown;
            if (payload.method === "conversation.read") {
              result = await mobileConversation(
                String(payload.params.conversationId ?? ""),
                current.current.sessions,
              );
            } else if (payload.method === "conversation.older") {
              const conversationId = String(payload.params.conversationId ?? "");
              // A watch on it has the session at hand already.
              let older = false;
              const conversation = await mobileConversation(
                conversationId,
                current.current.sessions,
                savedFor.current(conversationId),
                (turnIds) => {
                  const page = turnsBefore(
                    turnIds,
                    String(payload.params.before ?? ""),
                    Math.min(MAX_PAGE, Math.max(1, Number(payload.params.limit) || DEFAULT_TAIL)),
                  );
                  older = page.older;
                  return [page.start, page.end];
                },
              );
              result = { turns: conversation.turns, older };
            } else if (payload.method === "models.list") {
              const conversationId =
                typeof payload.params.conversationId === "string"
                  ? payload.params.conversationId
                  : undefined;
              result = await mobileModels(
                {
                  projectId: String(payload.params.projectId ?? ""),
                  conversationId,
                },
                current.current.sessions,
                conversationId ? savedFor.current(conversationId) : undefined,
                current.current.defaultRuntimeMode,
              );
            } else if (payload.method === "accounts.usage") {
              result = await mobileAccountUsage(
                payload.params.provider,
                payload.params.refresh === true,
              );
            } else if (payload.method === "project.changes") {
              result = await mobileProjectChanges(
                String(payload.params.projectId ?? ""),
                typeof payload.params.worktreePath === "string"
                  ? payload.params.worktreePath
                  : undefined,
              );
            } else if (
              payload.method === "project.files" ||
              payload.method === "project.file"
            ) {
              const read =
                payload.method === "project.files"
                  ? mobileProjectFiles
                  : mobileProjectFile;
              result = await read(
                String(payload.params.projectId ?? ""),
                String(payload.params.path ?? ""),
                typeof payload.params.worktreePath === "string"
                  ? payload.params.worktreePath
                  : undefined,
              );
            } else if (payload.method === "project.fileHighlight") {
              result = mobileFileHighlight(
                String(payload.params.job ?? ""),
                Number(payload.params.from) || 0,
              );
            } else if (payload.method === "project.search") {
              result = await mobileProjectSearch(
                String(payload.params.projectId ?? ""),
                String(payload.params.query ?? ""),
                typeof payload.params.worktreePath === "string"
                  ? payload.params.worktreePath
                  : undefined,
              );
            } else if (payload.method === "project.git") {
              result = await mobileProjectGit(
                String(payload.params.projectId ?? ""),
                typeof payload.params.worktreePath === "string"
                  ? payload.params.worktreePath
                  : undefined,
              );
            } else if (payload.method === "project.gitAction") {
              const signature = JSON.stringify(payload.params);
              let job = gitJobs.get(payload.commandId);
              if (job && job.signature !== signature)
                throw new Error("Command ID was already used with different input");
              if (!job) {
                if (gitJobs.size >= 100) {
                  const oldest = gitJobs.keys().next().value;
                  if (oldest) gitJobs.delete(oldest);
                }
                const started = mobileProjectGitAction(
                  String(payload.params.projectId ?? ""),
                  typeof payload.params.worktreePath === "string"
                    ? payload.params.worktreePath
                    : undefined,
                  payload.params,
                );
                // Settled for the race below; a failure still reaches the phone.
                started.catch(() => {});
                job = { signature, result: started };
                gitJobs.set(payload.commandId, job);
              }
              let timer: ReturnType<typeof setTimeout> | undefined;
              result = await Promise.race([
                job.result,
                new Promise((resolve) => {
                  timer = setTimeout(() => resolve({ pending: true }), GIT_ACTION_WAIT_MS);
                }),
              ]).finally(() => clearTimeout(timer));
            } else if (payload.method === "project.worktrees") {
              result = await mobileProjectWorktrees(
                String(payload.params.projectId ?? ""),
              );
            } else {
              const signature = JSON.stringify([
                payload.method,
                payload.params,
              ]);
              const existing = receipts.get(payload.commandId);
              if (existing && existing.signature !== signature)
                throw new Error(
                  "Command ID was already used with different input",
                );
              if (!existing && receipts.size >= 1000) {
                const oldest = receipts.keys().next().value;
                if (oldest) receipts.delete(oldest);
              }
              const promise =
                existing?.result ??
                current.current.command(
                  payload.method,
                  payload.params,
                  payload.commandId,
                );
              if (!existing)
                receipts.set(payload.commandId, { signature, result: promise });
              try {
                result = await promise;
              } catch (error) {
                receipts.delete(payload.commandId);
                throw error;
              }
            }
            // Conversation reads and watches are routed through the published
            // catalog. Make this session visible there before acknowledging it.
            if (payload.method === "session.start") await publishCatalog();
            response = { ok: true, result };
          } catch (error) {
            response = {
              ok: false,
              error: error instanceof Error ? error.message : String(error),
            };
          }
          await invoke("mobile_reply", { id: payload.id, response }).catch(
            (error) => {
              if (!disposed) console.error("Mobile reply failed", error);
            },
          );
        })();
      },
    );
    void publish();
    return () => {
      disposed = true;
      clearTimeout(timer);
      void listening.then((unlisten) => unlisten());
    };
  }, []);
}
