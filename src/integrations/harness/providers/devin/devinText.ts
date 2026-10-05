import { nativeModelId } from "../../../../features/sessions/model/models";
import { abortTextPromptRace } from "../../core/abortTextPrompt";
import { killChild, unwatchChild, watchChild } from "../../core/child";
import { JsonRpcClient } from "../../core/jsonRpc";
import type { TextPromptInput } from "../../core/registry";
import { asRecord, permissionOptionId, permissionRequestFromAcp, sessionIdFromResult } from "../antigravity/antigravityProtocol";
import { startDevinAcp } from "./devin";
import {
  devinAgentMessageText,
  devinModelChoices,
  devinModelFamilies,
  devinModelValue,
} from "./devinProtocol";

const TEXT_CHILD_PREFIX = "monocode-devin-text-";
const REQUEST_TIMEOUT_MS = 30_000;
const PROMPT_TIMEOUT_MS = 5 * 60_000;

const activeChildren = new Set<string>();
let turns: Promise<void> = Promise.resolve();

/**
 * Isolated, read-only prompt for side questions. Each run gets its own
 * throwaway `devin acp` session in Ask mode, deleted once it answers, so the
 * main conversation and `devin list` stay untouched.
 */
export function runDevinTextPrompt(input: TextPromptInput): Promise<string> {
  const run = turns.catch(() => undefined).then(() => promptOnce(input));
  turns = run.then(
    () => undefined,
    () => undefined,
  );
  return run;
}

export async function stopDevinTextPrompt(): Promise<void> {
  await Promise.all(
    [...activeChildren].map(async (childId) => {
      activeChildren.delete(childId);
      unwatchChild(childId);
      await killChild(childId).catch(() => undefined);
    }),
  );
}

async function promptOnce(input: TextPromptInput): Promise<string> {
  input.signal?.throwIfAborted();
  const childId = `${TEXT_CHILD_PREFIX}${crypto.randomUUID()}`;
  activeChildren.add(childId);
  let output = "";
  let collecting = false;
  const rpc = new JsonRpcClient(
    childId,
    {
      onNotification: (method, params) => {
        if (method !== "session/update" || !collecting) return;
        const text = devinAgentMessageText(params);
        if (!text) return;
        output += text;
        input.onEvent?.({ type: "message.delta", text });
      },
      onRequest: (id, method, params) => {
        // Side questions are read-only: refuse every tool permission.
        const response =
          method === "session/request_permission"
            ? (() => {
                const request = permissionRequestFromAcp(params);
                const optionId = permissionOptionId(
                  "deny",
                  request.optionIds,
                  request.optionKinds,
                );
                return rpc.respond(id, {
                  outcome: optionId
                    ? { outcome: "selected", optionId }
                    : { outcome: "cancelled" },
                });
              })()
            : rpc.respondError(id, {
                code: -32601,
                message: `Method not found: ${method}`,
              });
        void response.catch(() => undefined);
      },
    },
    { includeJsonrpc: true, label: "devin-text" },
  );
  watchChild(
    childId,
    (line) => rpc.pushLine(line),
    () => rpc.close(new Error("Devin text prompt exited")),
  );

  let sessionId: string | undefined;
  const abort = abortTextPromptRace(input.signal, () =>
    sessionId
      ? rpc.notify("session/cancel", { sessionId }).catch(() => undefined)
      : undefined,
  );
  try {
    const run = async () => {
      await startDevinAcp(childId, input.cwd, rpc, { allowBrowser: false });
      const created = await rpc.request(
        "session/new",
        { cwd: input.cwd, mcpServers: [] },
        REQUEST_TIMEOUT_MS,
      );
      sessionId = sessionIdFromResult(created);
      if (!sessionId) throw new Error("Devin did not return a session id");
      await rpc.request(
        "session/set_mode",
        { sessionId, modeId: "ask" },
        REQUEST_TIMEOUT_MS,
      );
      const options = asRecord(created)?.configOptions;
      const model = input.model
        ? devinModelValue(
            devinModelFamilies(devinModelChoices(created)),
            nativeModelId(input.model).trim(),
            input.modelSettings,
          )
        : "";
      const advertised =
        Array.isArray(options) &&
        options.some((option) => asRecord(option)?.id === "model");
      if (model && model !== "default" && advertised) {
        await rpc
          .request(
            "session/set_config_option",
            { sessionId, configId: "model", value: model },
            REQUEST_TIMEOUT_MS,
          )
          .catch(() => undefined);
      }
      collecting = true;
      await rpc.request(
        "session/prompt",
        { sessionId, prompt: [{ type: "text", text: input.prompt }] },
        input.timeoutMs ?? PROMPT_TIMEOUT_MS,
      );
      collecting = false;
      await rpc
        .request("session/delete", { sessionId }, REQUEST_TIMEOUT_MS)
        .catch(() => undefined);
      return output;
    };
    return await Promise.race([run(), ...(abort.promise ? [abort.promise] : [])]);
  } finally {
    collecting = false;
    abort.detach();
    activeChildren.delete(childId);
    rpc.close();
    unwatchChild(childId);
    await killChild(childId).catch(() => undefined);
  }
}
