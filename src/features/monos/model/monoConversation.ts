import {
  sessionConversationPage,
  type SessionReadOptions,
} from "../../agent-app/model/sessionConversation";
import { getMonoTranscriptPage } from "../../sessions/data/sessionStore";
import type { Session } from "../../sessions/model/session";

/** The Mono's CLI reads saved pages independently of the writable live window. */
export async function readMonoConversation(
  session: Session,
  options: SessionReadOptions,
) {
  // Validate limits before fetching an archived cursor.
  sessionConversationPage(
    { ...session, blocks: [] },
    { ...options, before: undefined },
  );
  let blocks = session.blocks;
  let before = session.monoTranscript?.before ?? null;
  let readOptions = options;
  if (options.before && !blocks.some((block) => block.id === options.before)) {
    const page = await getMonoTranscriptPage(session.id, {
      beforeBlockId: options.before,
    });
    blocks = page.blocks;
    before = page.before;
    readOptions = { ...options, before: undefined };
  }
  let result = sessionConversationPage({ ...session, blocks }, readOptions);
  // Habits and handoffs have their own transcript turns. Continue paging if
  // the UI window contains fewer than the requested user exchanges.
  while (result.turns.length < (options.limit ?? 3) && before != null) {
    const page = await getMonoTranscriptPage(session.id, { before });
    blocks = [...page.blocks, ...blocks];
    before = page.before;
    result = sessionConversationPage({ ...session, blocks }, readOptions);
  }
  if (!result.nextBefore && before != null && result.turns.length)
    result.nextBefore = result.turns[0].turnId;
  return result;
}
