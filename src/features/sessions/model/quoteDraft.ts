import {
  addChatContext,
  composeChatContext,
  type ChatContextItem,
} from "./chatContext";

export const ADD_TO_CHAT_EVENT = "monocode:add-to-chat";

/** Something to put in a composer: text for the draft, or a context chip. */
export type ComposerInsert =
  | { kind: "text"; text: string }
  | { kind: "context"; item: ChatContextItem };

export type ComposerInsertRequest = ComposerInsert & { id: number };

/** Sends a context chip to the focused session, or to a new one. */
export function requestAddToChat(item: ChatContextItem) {
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<ChatContextItem>(ADD_TO_CHAT_EVENT, { detail: item }),
  );
}

/** Initial composer draft for an add-to-chat request that opens a new session. */
export function composerSeedForAddToChat(item: ChatContextItem): string {
  return composeChatContext("", [item]);
}

export type ComposerInsertConsumption = {
  draft: string;
  context: ChatContextItem[];
  consumedId: number | null;
  changed: boolean;
};

export function isMarkdownBlockquotePosition(
  text: string,
  position: number,
): boolean {
  const index = Math.max(0, Math.min(position, text.length));
  const lineStart = text.lastIndexOf("\n", Math.max(0, index - 1)) + 1;
  return /^ {0,3}>/.test(text.slice(lineStart, index));
}

export function appendComposerInsert(draft: string, text: string): string {
  const selected = text.replace(/\r\n?/g, "\n").trim();
  if (!selected) return draft;
  return joinComposerInsert(draft, selected);
}

export function consumeComposerInsert(
  draft: string,
  context: ChatContextItem[],
  consumedId: number | null,
  request: ComposerInsertRequest | undefined,
): ComposerInsertConsumption {
  if (!request || request.id === consumedId) {
    return { draft, context, consumedId, changed: false };
  }

  if (request.kind === "context") {
    const next = addChatContext(context, request.item);
    return {
      draft,
      context: next,
      consumedId: request.id,
      changed: next.length !== context.length,
    };
  }

  const next = appendComposerInsert(draft, request.text);
  return {
    draft: next,
    context,
    consumedId: request.id,
    changed: next !== draft,
  };
}

export function acknowledgeComposerInsert(
  current: ComposerInsertRequest | undefined,
  handledId: number,
): ComposerInsertRequest | undefined {
  return current?.id === handledId ? undefined : current;
}

function joinComposerInsert(draft: string, block: string): string {
  const separator =
    draft.length === 0
      ? ""
      : draft.endsWith("\n\n")
        ? ""
        : draft.endsWith("\n")
          ? "\n"
          : "\n\n";
  return `${draft}${separator}${block}\n\n`;
}
