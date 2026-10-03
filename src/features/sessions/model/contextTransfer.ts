import type { PortableContext } from "./portableContext";
import { renderPortableContext } from "./portableContext";
import type { SendTurnInput } from "../../../integrations/harness/core/types";

export type ContextTransferCapabilities = {
  nativeMessages: boolean;
  resumedAppend: boolean;
};

export type ContextTransferReceipt = {
  mode: "native" | "inline";
  providerSessionId?: string;
  includedIds?: string[];
  omittedIds?: string[];
  throughBlockId?: string;
};

export type ContextTransferInput = {
  context: PortableContext;
  /** Full eligible history for a target whose native resume failed. */
  fallbackContext?: PortableContext;
  /** History delivery succeeded. The current user turn may still fail. */
  onDelivered?: (receipt: ContextTransferReceipt) => void | Promise<void>;
};

export class ContextTransferError extends Error {
  readonly uncertain = true;
  constructor(message: string, readonly cause: unknown) {
    super(message);
    this.name = "ContextTransferError";
  }
}

/** Share acceptance evidence between the desktop registry and owning host. */
export function prepareContextTransferInput(
  input: SendTurnInput,
  capabilities?: ContextTransferCapabilities,
): SendTurnInput {
  let accepted = false;
  let boundId: string | undefined;
  const inline = input.contextTransfer && !capabilities?.nativeMessages;
  const onAccepted = () => {
    if (accepted) return;
    accepted = true;
    if (inline) reportInlineContextDelivery(input, {
      mode: "inline",
      providerSessionId: boundId,
      includedIds: input.contextTransfer!.context.items.map((item) => item.id),
      omittedIds: input.contextTransfer!.context.omitted.map((item) => item.id),
      throughBlockId: input.contextTransfer!.context.throughBlockId,
    });
    input.onAccepted?.();
  };
  return {
    ...input,
    ...(inline ? {
      text: renderPortableContext(input.contextTransfer!.context, input.text),
      contextTransfer: undefined,
    } : {}),
    onAccepted,
    onEvent: (event) => {
      if (event.type === "session.providerBound") boundId = event.providerSessionId;
      if (!capabilities?.nativeMessages &&
          (event.type === "turn.started" || event.type === "message.delta" ||
           event.type === "tool.started" || event.type === "plan" || event.type === "image.generated")) {
        onAccepted();
      }
      input.onEvent(event);
    },
  };
}

export function reportInlineContextDelivery(input: SendTurnInput, receipt: ContextTransferReceipt): void {
  try {
    void Promise.resolve(input.contextTransfer?.onDelivered?.(receipt)).catch((error: unknown) => {
      input.onEvent({ type: "session.error", message: `MonoCode could not save the shared-history delivery receipt. ${error instanceof Error ? error.message : String(error)}` });
    });
  } catch (error) {
    input.onEvent({ type: "session.error", message: `MonoCode could not save the shared-history delivery receipt. ${error instanceof Error ? error.message : String(error)}` });
  }
}
