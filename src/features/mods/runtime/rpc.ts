import { invoke } from "@tauri-apps/api/core";
import type { RpcMethod, SessionInfo, ToolCallEvent } from "../api/types";
import { copyText } from "../../../platform/tauri/clipboard";
import type { Session } from "../../sessions/model/session";
import { sessionInfo, toolCallEvent } from "./events";

/** What the window lends the runtime; App fills it in. */
export type ModHostContext = {
  /** Active project root; `$PROJECT` for fs globs and project mod discovery. */
  project(): string | null;
  sessions(): readonly Session[];
  activeSessionId(): string | null;
  openView(view: {
    name: string;
    id: string;
    title: string;
    site: "Pane" | "SidebarTab";
    icon?: string;
  }): void;
  openFile(path: string, line?: number): void;
};

type Options = Record<string, unknown>;

/**
 * Answers one `WorkerToHost` call. Privileged methods go to Rust, which
 * checks them against the installed manifest; the rest read window state.
 */
export async function callRpc(
  ctx: ModHostContext,
  name: string,
  method: RpcMethod,
  args: unknown[],
): Promise<unknown> {
  const project = ctx.project();
  const base = { project, name };
  const session = (id: unknown) =>
    ctx
      .sessions()
      .find(
        (s) => s.id === (typeof id === "string" ? id : ctx.activeSessionId()),
      );
  switch (method) {
    case "process.run": {
      const [argv, options = {}] = args as [string[], Options?];
      return invoke("mod_exec", {
        ...base,
        argv,
        cwd: options.cwd ?? project,
        timeoutMs: options.timeoutMs,
        env: options.env,
      });
    }
    case "fs.read":
    case "fs.list":
    case "fs.stat":
      return invoke("mod_fs", { ...base, op: method.slice(3), path: args[0] });
    case "http.request": {
      const [url, options = {}] = args as [string, Options?];
      return invoke("mod_http", {
        ...base,
        url,
        method: options.method ?? "GET",
        headers: options.headers,
        body: options.body,
      });
    }
    case "store.get":
    case "store.set":
    case "store.delete":
      return invoke("mod_store", {
        ...base,
        op: method.slice(6),
        key: args[0],
        value: args[1] ?? null,
      });
    case "env.get":
      return invoke("mod_env", { ...base, key: args[0] });
    case "session.current": {
      const found = session(args[0]);
      return found ? (sessionInfo(found) satisfies SessionInfo) : null;
    }
    case "session.agents":
      return (session(args[0])?.blocks ?? []).flatMap((block) =>
        block.agentRun && block.tool?.callId
          ? [
              {
                id: block.tool.callId,
                name: block.agentRun.name,
                type: block.agentRun.agentType,
              },
            ]
          : [],
      );
    case "session.toolCalls": {
      const found = session(args[0]);
      if (!found) return [];
      return found.blocks.flatMap((block): ToolCallEvent[] => {
        const tool = block.tool;
        if (!tool?.callId) return [];
        return [
          toolCallEvent(found.id, {
            type: "tool.started",
            callId: tool.callId,
            title: tool.title ?? "",
            kind: tool.kind,
            preview: tool.preview,
            paths: tool.paths,
          }),
        ];
      });
    }
    case "workspace.projectCwd":
      return project;
    case "workspace.isLightScheme":
      return document.documentElement.classList.contains("theme-light");
    case "ui.copy":
      return copyText(String(args[0])).then(
        () => ({ isCopied: true }),
        () => ({ isCopied: false }),
      );
  }
}
