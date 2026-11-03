import {
  HANDOFF_TITLE,
  buildDeterministicHandoff,
  buildHandoffComposerCard,
  sessionThroughTurn,
} from "../../sessions/model/handoff";
import { mergeModelSettings, resolveModel } from "../../sessions/model/models";
import {
  harnessForTurn,
  turnEditedFiles,
  turnUserRequest,
} from "../../sessions/model/secondOpinion";
import {
  formatSessionTitle,
  newSession,
  sessionDisplayTitle,
  sessionWorkCwd,
  type Session,
} from "../../sessions/model/session";
import { groupTurns } from "../../sessions/model/transcriptActivity";
import { mobileChoice } from "./mobileWorkspace";
import { mobileTranscript } from "./mobileTranscript";

/** Creates the same draft as the desktop reply's handoff action. */
export function mobileHandoff(
  source: Session,
  params: Record<string, unknown>,
): Session {
  if (source.worktreeRemoved || source.id.startsWith("mono:"))
    throw new Error("This session can't be handed off");
  const turns = groupTurns(source.blocks);
  const index = turns.findIndex((turn) => turn[0]?.id === params.turnId);
  const turn = turns[index];
  const shown =
    index < 0
      ? undefined
      : mobileTranscript(source, {
          busy: !!source.busy,
          range: () => [index, index + 1],
        })[0];
  if (!turn || !shown?.footer)
    throw new Error("This reply is no longer available for handoff");
  const { harness, model, modelSettings } = mobileChoice(params);
  const from = harnessForTurn(source.blocks, turn, source.harness);
  if (!harness || !model || harness === from)
    throw new Error("Choose another provider and model");
  const sliced = sessionThroughTurn(source, turn);
  const session = newSession(harness, source.cwd, model, source.runtimeMode);
  const title = sessionDisplayTitle(source.title, source.harness);
  return {
    ...session,
    title: formatSessionTitle(
      harness,
      title === "New session" ? HANDOFF_TITLE : title,
    ),
    worktreeCwd: source.worktreeCwd,
    branch: source.branch,
    modelSettings: mergeModelSettings(
      resolveModel(harness, model),
      modelSettings,
    ),
    handoffCard: buildHandoffComposerCard({
      from,
      to: harness,
      brief: buildDeterministicHandoff(sliced),
      userRequest: turnUserRequest(turn),
      files: turnEditedFiles(sliced.blocks, sessionWorkCwd(source)),
    }),
  };
}
