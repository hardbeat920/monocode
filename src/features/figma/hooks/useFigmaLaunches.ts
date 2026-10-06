import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { sameProjectPath } from "../../projects/model/recents";
import type { QuickLaunch } from "../../quick-composer/model/quickComposer";
import { prepareAttachments } from "../../sessions/model/attachments";
import type {
  Attachment,
  HarnessId,
  Session,
} from "../../sessions/model/session";
import {
  FIGMA_GENERATION_EVENT,
  FIGMA_LAUNCH_EVENT,
  FIGMA_SESSION_MODEL_EVENT,
  figmaGenerationLaunch,
  figmaGenerationMessage,
  figmaProjectError,
  prepareFigmaPreview,
  type FigmaGeneration,
  type FigmaLaunchRequest,
  type FigmaPreviewWorkspace,
  type FigmaSessionModelRequest,
} from "../model/figma";
import { reportFigmaActivity } from "../model/figmaActivity";
import { resolveFigmaModel, type FigmaModelChoice } from "../model/figmaModels";
import {
  figmaSessionTargetFrom,
  setFigmaSessionTarget,
} from "../model/figmaTarget";

export type FigmaWorkspace = {
  launch: (launch: QuickLaunch, id: string) => Promise<void>;
  submit: (
    sessionId: string,
    text: string,
    attachments: Attachment[],
  ) => boolean | Promise<boolean>;
  changeModel: (sessionId: string, harness: HarnessId, model: string) => void;
  changeModelSettings: (
    sessionId: string,
    modelSettings: Record<string, string>,
  ) => void;
  currentProject: () => string;
  session: (id: string) => Session | null;
};

export function figmaLaunchId(generation: FigmaGeneration): string {
  return `figma-${generation.id}`;
}

export async function deliverFigmaGeneration(
  generation: FigmaGeneration,
  cwd: string,
  choice: FigmaModelChoice,
  workspace: Pick<FigmaWorkspace, "launch" | "submit"> & {
    selected: Session | null;
    prepare: (
      generationId: string,
      cwd: string,
    ) => Promise<FigmaPreviewWorkspace>;
  },
): Promise<void> {
  const error = figmaProjectError(cwd);
  if (error) {
    reportFigmaActivity({ status: "failed", cwd, message: error });
    return;
  }
  const target = figmaSessionTargetFrom(workspace.selected);
  try {
    if (target && sameProjectPath(target.cwd, cwd)) {
      const preview = await workspace.prepare(generation.id, target.workCwd);
      const message = figmaGenerationMessage(
        generation,
        preview,
        target.harness,
      );
      const accepted = await workspace.submit(
        target.sessionId,
        message.prompt,
        await prepareAttachments(message.attachments),
      );
      if (!accepted)
        throw new Error(
          `${target.title} cannot take the component right now. Wait for its turn to finish or select another session.`,
        );
      reportFigmaActivity({
        status: "started",
        cwd,
        generation,
        choice: {
          harness: target.harness,
          model: target.model,
          modelSettings: target.modelSettings,
        },
        target: {
          kind: "session",
          sessionId: target.sessionId,
          title: target.title,
        },
      });
      return;
    }
    const preview = await workspace.prepare(generation.id, cwd);
    await workspace.launch(
      figmaGenerationLaunch(generation, preview, cwd, choice),
      figmaLaunchId(generation),
    );
    reportFigmaActivity({
      status: "started",
      cwd,
      generation,
      choice,
      target: { kind: "new" },
    });
  } catch (reason) {
    reportFigmaActivity({
      status: "failed",
      cwd,
      message: String(reason instanceof Error ? reason.message : reason),
    });
  }
}

export function applyFigmaSessionModel(
  request: FigmaSessionModelRequest,
  workspace: Pick<FigmaWorkspace, "changeModel" | "changeModelSettings">,
): void {
  if (request.kind === "model") {
    workspace.changeModel(request.sessionId, request.harness, request.model);
    return;
  }
  workspace.changeModelSettings(request.sessionId, request.modelSettings);
}

export function useFigmaLaunches(
  workspace: FigmaWorkspace,
  selected: Session | null,
) {
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;
  const selectedIdRef = useRef<string | null>(null);
  selectedIdRef.current = selected?.id ?? null;

  useEffect(() => {
    setFigmaSessionTarget(figmaSessionTargetFrom(selected));
  }, [selected]);

  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | null = null;
    const deliver = (
      generation: FigmaGeneration,
      cwd: string,
      choice: FigmaModelChoice,
    ) => {
      const current = workspaceRef.current;
      const selectedId = selectedIdRef.current;
      void deliverFigmaGeneration(generation, cwd, choice, {
        launch: current.launch,
        submit: current.submit,
        selected: selectedId ? current.session(selectedId) : null,
        prepare: prepareFigmaPreview,
      });
    };
    const onRequest = (event: Event) => {
      const request = (event as CustomEvent<FigmaLaunchRequest>).detail;
      if (request) deliver(request.generation, request.cwd, request.choice);
    };
    const onSessionModel = (event: Event) => {
      const request = (event as CustomEvent<FigmaSessionModelRequest>).detail;
      if (request) applyFigmaSessionModel(request, workspaceRef.current);
    };
    window.addEventListener(FIGMA_LAUNCH_EVENT, onRequest);
    window.addEventListener(FIGMA_SESSION_MODEL_EVENT, onSessionModel);
    void listen<FigmaGeneration>(FIGMA_GENERATION_EVENT, (event) => {
      const cwd = workspaceRef.current.currentProject();
      deliver(event.payload, cwd, resolveFigmaModel(cwd).choice);
    }).then(
      (unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      },
      (reason: unknown) => {
        if (disposed) return;
        reportFigmaActivity({
          status: "failed",
          cwd: workspaceRef.current.currentProject(),
          message: `Components sent from the Figma plugin cannot reach MonoCode: ${String(reason instanceof Error ? reason.message : reason)}`,
        });
      },
    );
    return () => {
      disposed = true;
      stop?.();
      window.removeEventListener(FIGMA_LAUNCH_EVENT, onRequest);
      window.removeEventListener(FIGMA_SESSION_MODEL_EVENT, onSessionModel);
      setFigmaSessionTarget(null);
    };
  }, []);
}
