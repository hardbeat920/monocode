import { router, Stack, useIsFocused } from "expo-router";
import {
  ConnectionNotice,
  ReconnectingStatus,
  useOffline,
} from "@/features/connection/ConnectionNotice";
import { hasNativeTranscript } from "@/features/connection/nativeLink";
import { useProjectChanges } from "@/features/connection/useProjectChanges";
import { useProjectWorktrees } from "@/features/connection/useProjectWorktrees";
import type { Project, SessionSummary } from "@/features/workspace/types";
import { use, useCallback, useEffect, useState, type ReactNode } from "react";
import { Platform, Pressable, StyleSheet } from "react-native";
import Animated, {
  Easing,
  useAnimatedStyle,
  useDerivedValue,
  useSharedValue,
  withTiming,
  type SharedValue,
} from "react-native-reanimated";
import { scheduleOnRN } from "react-native-worklets";

import { useEmptyBackground } from "@/features/background/useEmptyBackground";
import { ProjectMascot } from "@/features/mascots/ProjectMascot";
import { effortLabel } from "@/features/models/modelChoice";
import {
  useForgetChoiceOnLeave,
  useSessionChoice,
} from "@/features/models/sessionChoice";
import { useModelCatalog } from "@/features/models/useModelCatalog";
import { useOpenSessionAfterStarting } from "@/features/settings/newSessions";
import type { Attachment } from "@/features/sessions/attachments";
import { Composer } from "@/features/sessions/Composer";
import { QuestionCard } from "@/features/sessions/QuestionCard";
import { EmptySession } from "@/features/sessions/EmptySession";
import {
  latestSessionError,
  useReportSessionError,
  useSessionErrorNotice,
} from "@/features/sessions/sessionError";
import { LiveSessions } from "@/features/sessions/LiveSessions";
import { NativeTranscript } from "@/features/sessions/NativeTranscript";
import { FileOpener } from "@/features/sessions/Markdown";
import {
  markStartedHere,
  markViewing,
} from "@/features/sessions/activityStore";
import { SessionContent, SessionScreen } from "@/features/sessions/SessionScreen";
import { SessionsPage } from "@/features/sessions/SessionsPage";
import {
  SessionTitle,
  SessionToolbar,
} from "@/features/sessions/SessionToolbar";
import { Transcript, TranscriptLoading } from "@/features/sessions/Transcript";
import {
  ProjectPages,
  ProjectSwitch,
  useProjectPages,
} from "@/features/workspace/ProjectPages";
import {
  useConversation,
  useConversationTurns,
  useWorkspace,
} from "@/features/workspace/WorkspaceProvider";
import {
  WorkspaceLine,
  WorkspaceMenu,
  type Workspace,
} from "@/features/workspace/WorkspaceMenu";
import { ComposerWorkspace } from "@/features/workspace/ComposerWorkspace";
import { Icon } from "@/ui/Icon";
import { androidBackHeader } from "@/ui/AndroidHeader";
import { GlassBlurTarget, LiquidView } from "@/ui/LiquidView";
import { useColors } from "@/ui/theme";
import { usePushedIn, usePushStarted } from "@/ui/usePushedIn";

/**
 * A project's screen: a new session, or with `sessionId` one of its sessions.
 * A new session comes first, then the project's sessions to its right;
 * swiping left slides the sessions in (and the composer out with the new
 * session), the header switch following the finger. The menu button opens
 * the project picker, and the button on the right settings; a session's
 * right has its changes and a menu of its actions instead. A session, or
 * one just started, is pushed over it as a screen of its own: its chat under
 * the stack's native header (see the session route).
 */
export function ProjectScreen({
  project,
  sessionId,
}: {
  project: Project;
  sessionId: string | undefined;
}) {
  const { startSession, sendMessage, stop } = useWorkspace();
  const [openAfterStarting] = useOpenSessionAfterStarting();
  const colors = useColors();
  const blurTarget = use(GlassBlurTarget);
  // What it runs with, as the model picker, a route over it, sets it.
  const { session, chosen, startAccess, accounts } = useSessionChoice(project, sessionId);
  useForgetChoiceOnLeave(project.id, sessionId);
  // Open, however it was reached, a session is off the finished deck, and
  // stays off it as it finishes in front of the user.
  useEffect(() => (sessionId ? markViewing(sessionId) : undefined), [sessionId]);
  // The session's watch opens as its push starts: its first turns, landing
  // before, would hold the slide back.
  const pushStarted = usePushStarted(!!sessionId);
  const conversation = useConversation(sessionId ?? null, pushStarted);

  // Where a new session works, shared by the header and expanded composer.
  const [workspace, setWorkspace] = useState<Workspace>({ mode: "current" });
  // Each session, and each new one, starts in the project's checkout, as it
  // would on a screen of its own.
  const scope = `${project.id}\n${sessionId ?? ""}`;
  const [shownScope, setShownScope] = useState(scope);
  if (shownScope !== scope) {
    setShownScope(scope);
    setWorkspace({ mode: "current" });
  }

  // A session's reads from the desktop wait for its push, so their replies
  // don't re-render it as it slides in; meanwhile it shows the last ones.
  const pushedIn = usePushedIn(!!sessionId);
  const { worktrees, loaded } = useProjectWorktrees(project.id, pushedIn);
  const { catalog } = useModelCatalog(
    pushedIn ? project.id : undefined,
    sessionId,
  );
  // Only a new session has pages; a session is the chat alone.
  const pages = useProjectPages(project.id, !sessionId);
  // Between pages, mid-swipe or springing to one.
  const swiping = useDerivedValue(() => {
    const at = pages.progress.get();
    return Math.abs(at - Math.round(at)) > 0.001;
  });

  // The working copy the composer, Changes and Files show. A new session's is
  // the one the header menu picked; a new worktree has no changes yet and its
  // files are its project's, as it starts off HEAD. A session's is the
  // worktree the desktop says it works in, or else the project's checkout; an
  // older desktop doesn't say, so it goes by the worktree on its branch.
  const fresh = !sessionId && workspace.mode === "worktree";
  const sessionBranch = session?.branch || project.branch || "";
  const offCheckout =
    !!sessionId &&
    !session?.worktreePath &&
    !!sessionBranch &&
    sessionBranch !== project.branch;
  const worktreePath = sessionId
    ? (session?.worktreePath ??
      (offCheckout
        ? worktrees.find((tree) => tree.name === sessionBranch)?.path
        : undefined))
    : workspace.mode === "existing"
      ? workspace.worktree.path
      : undefined;
  // Read while the screen is up so the switch can mark that there are some,
  // as the desktop's Changes tab does. Guessing from its branch, a session's
  // wait for the worktree list rather than read the checkout's and throw them away.
  const changesState = useProjectChanges(
    project.id,
    sessionId ? pushedIn && (loaded || !offCheckout) : !fresh,
    worktreePath,
  );
  const changes = fresh ? [] : (changesState.result?.files ?? []);
  const branch = sessionId
    ? changesState.result?.branch || sessionBranch
    : fresh
      ? "new worktree"
      : changesState.result?.branch ||
        (workspace.mode === "existing"
          ? workspace.worktree.name
          : project.branch);

  const harness =
    chosen?.pick.harness ?? session?.harness ?? project.defaultHarness;
  const model =
    chosen?.name ??
    session?.modelName ??
    session?.model ??
    project.defaultModelName ??
    project.defaultModel ??
    "";
  // The effort shown by the model's name, read from the desktop's models: for
  // a pick here or in Settings, else what the session or project runs with.
  const effortPick = chosen?.pick ?? catalog?.current;
  const effort = effortPick
    ? effortLabel(
        catalog?.models.find((entry) => entry.id === effortPick.model),
        effortPick,
      )
    : undefined;

  // When opening a new session, the empty screen fades away and is back once
  // that session is closed again.
  const emptyOpacity = useSharedValue(1);
  const emptyStyle = useAnimatedStyle(() => ({ opacity: emptyOpacity.get() }));
  const focused = useIsFocused();
  useEffect(() => {
    if (focused) emptyOpacity.set(1);
  }, [focused, emptyOpacity]);

  const start = async (text: string, attachments: Attachment[]) => {
    // When opening the session, wait for the background and intro to fade,
    // even if the desktop replies first. Otherwise stay ready for another.
    const faded = openAfterStarting
      ? new Promise<void>((resolve) => {
          emptyOpacity.set(
            withTiming(
              0,
              { duration: 320, easing: Easing.out(Easing.cubic) },
              (finished) => {
                if (finished) scheduleOnRN(resolve);
              },
            ),
          );
        })
      : Promise.resolve();
    try {
      const [started] = await Promise.all([
        startSession(
          project.id,
          text,
          {
            ...chosen?.pick,
            ...(harness && accounts[harness]
              ? { providerAccountId: accounts[harness] }
              : {}),
            // Unpicked here or in Settings, the desktop starts it with its own
            // mode at send time.
            ...(startAccess ? { runtimeMode: startAccess } : {}),
            ...(workspace.mode === "worktree"
              ? { workspaceMode: "worktree" as const }
              : {}),
            ...(workspace.mode === "existing"
              ? { worktreePath: workspace.worktree.path }
              : {}),
          },
          attachments,
        ),
        faded,
      ]);
      markStartedHere(started);
      if (openAfterStarting) openSession(started);
    } catch (reason) {
      emptyOpacity.set(
        withTiming(1, { duration: 200, easing: Easing.out(Easing.cubic) }),
      );
      throw reason;
    }
  };

  function openSession(id: string) {
    router.push({
      pathname: "/project/[projectId]/session/[sessionId]",
      params: { projectId: project.id, sessionId: id },
    });
  }

  const sessionNotice = useSessionErrorNotice(sessionId);
  const offline = useOffline();

  const openSettings = () =>
    router.push({ pathname: "/settings", params: { projectId: project.id } });

  // Native headers render outside the screen's context tree. Carry the page's
  // blur target into each button callback so its glass samples page content.
  const headerSurface = (content: ReactNode) => (
    <GlassBlurTarget value={blurTarget}>{content}</GlassBlurTarget>
  );
  const projectAvatar = (
    <ProjectHeaderButton
      label={`Switch project, ${project.name}`}
      onPress={() => router.push({
        pathname: "/projects",
        params: { current: project.id },
      })}
    >
      <ProjectMascot
        project={project.path}
        name={project.mascot}
        color={project.color}
        size={20}
      />
    </ProjectHeaderButton>
  );
  const settingsButton = (
    <ProjectHeaderButton
      label="Settings"
      onPress={openSettings}
    >
      <Icon name="settings" size={22} color={colors.content} />
    </ProjectHeaderButton>
  );

  const sessionTitle = sessionId ? (
    <SessionTitle
      title={session?.title ?? "Session"}
      subtitle={[project.name, branch].filter(Boolean).join(" · ")}
    />
  ) : null;
  const sessionToolbar = sessionId ? (
    <SessionToolbar
      projectId={project.id}
      sessionId={sessionId}
      session={session}
      worktreePath={worktreePath}
      harness={harness}
      changed={changes.length}
      onSettings={openSettings}
    />
  ) : null;

  return (
    <SessionScreen
      // Where a new session will work, under the tabs and in view across its
      // pages; a session's composer shows where it does.
      below={
        sessionId ? undefined : offline ? (
          // Nothing to pick from while the desktop can't be reached.
          <ReconnectingStatus />
        ) : (
          <WorkspaceMenu
            value={workspace}
            worktrees={worktrees}
            onChange={setWorkspace}
            branch={branch}
            checkoutBranch={project.branch}
            files={changes}
            onOpenChanges={() =>
              router.push({
                pathname: "/changes",
                params: {
                  projectId: project.id,
                  ...(worktreePath ? { worktreePath } : {}),
                  ...(harness ? { harness } : {}),
                },
              })
            }
            // A new worktree has nothing to read; the others, until their
            // first changes come in, or fail to.
            loading={!fresh && !changesState.result && !changesState.error}
          />
        )
      }
      // The composer, its blur and all, slides off with the new session's page.
      dockStyle={sessionId ? undefined : pages.slide}
      composer={
        <>
          {sessionId && conversation.question ? (
            <QuestionCard conversationId={sessionId} prompt={conversation.question} />
          ) : null}
          <Composer
            key={sessionId ?? project.id}
            notice={sessionNotice}
            context={project.name}
            branch={sessionId ? (session?.branch ?? project.branch) : branch}
            model={model}
            effort={effort}
            harness={harness}
            onModelPress={() =>
              router.push({
                pathname: "/models",
                params: {
                  projectId: project.id,
                  ...(sessionId ? { sessionId } : {}),
                },
              })
            }
            {...(sessionId
              ? {
                  busy: conversation.busy,
                  onStop: () => stop(sessionId),
                  onSend: (text: string, attachments: Attachment[]) => {
                    markStartedHere(sessionId);
                    return sendMessage(sessionId, text, attachments);
                  },
                  workspace: (
                    <WorkspaceLine
                      current={!worktreePath}
                      branch={branch}
                      files={changes}
                    />
                  ),
                }
              : {
                  onSend: start,
                  workspace: (
                    <ComposerWorkspace
                      value={workspace}
                      onChange={setWorkspace}
                      disabled={offline}
                    />
                  ),
                })}
          />
        </>
      }
    >
      {/* The stack's native header: native items, so the same ones morph
          into place as a session is pushed and back. The project's has the
          picker on the left and its pages' switch for a title; a session's,
          the back button and its title beside it. */}
      <Stack.Screen
        options={
          sessionId
            ? // A session's title is on the left, beside the back button.
              Platform.OS === "android"
              ? {
                  ...androidBackHeader(sessionTitle),
                  headerTitleAlign: "left",
                  headerRight: () => headerSurface(sessionToolbar),
                }
              : { headerTitle: "", headerBackVisible: true }
            : {
                headerTitle: () => <ProjectSwitch pages={pages} />,
                ...(Platform.OS === "android"
                  ? {
                      headerTitleAlign: "center" as const,
                      headerLeft: () => headerSurface(projectAvatar),
                      headerRight: () => headerSurface(settingsButton),
                      headerBackVisible: false,
                    }
                  : {}),
              }
        }
      />
      {sessionId ? (
        Platform.OS === "android" ? null : (
          sessionTitle
        )
      ) : Platform.OS === "android" ? null : (
        <Stack.Toolbar placement="left">
          {/* The project's mascot in the bar's glass, for the button. */}
          <Stack.Toolbar.View>{projectAvatar}</Stack.Toolbar.View>
        </Stack.Toolbar>
      )}
      {sessionId ? (
        Platform.OS === "android" ? null : (
          sessionToolbar
        )
      ) : Platform.OS === "android" ? null : (
        <Stack.Toolbar placement="right">
          {/* The app's own cog in the bar's glass, as the mascot on the left. */}
          <Stack.Toolbar.View>{settingsButton}</Stack.Toolbar.View>
        </Stack.Toolbar>
      )}
      <SessionContent>
        {sessionId ? (
          <SessionTranscript
            sessionId={sessionId}
            projectId={project.id}
            worktreePath={worktreePath}
            busy={conversation.busy}
            failed={!!conversation.error}
          />
        ) : (
          <ProjectPages
            pages={pages}
            chat={
              <Animated.View style={[styles.chat, emptyStyle]}>
                <ProjectEmptySession
                  project={project}
                  inView={focused && pages.view === "chat"}
                  swiping={swiping}
                  onOpen={(session) => openSession(session.id)}
                />
              </Animated.View>
            }
            sessions={
              <SessionsPage
                projectId={project.id}
                active={pages.view === "sessions"}
                worktree={
                  workspace.mode === "existing" ? workspace.worktree : undefined
                }
                onOpen={(session) => openSession(session.id)}
              />
            }
          />
        )}
      </SessionContent>
      {/* After the content: iOS blurs under the header the scroll view it
          finds first, down each view's first child. */}
      <ConnectionNotice error={conversation.error} />
    </SessionScreen>
  );
}

/**
 * A new session's empty screen over the project's background, read here so a
 * session, which has no empty screen, doesn't load it as it is pushed. Its
 * sessions at work rest on the composer, a tap away.
 */
function ProjectEmptySession({
  project,
  inView,
  swiping,
  onOpen,
}: {
  project: Project;
  /** In front, on its own page; the working sessions rest only then. */
  inView: boolean;
  swiping: SharedValue<boolean>;
  onOpen: (session: SessionSummary) => void;
}) {
  const { background, settings } = useEmptyBackground(project.id);
  return (
    <EmptySession
      project={project}
      background={background}
      hasPhoto={!!settings.image}
      accessory={
        <LiveSessions
          projectId={project.id}
          onOpen={onOpen}
          inView={inView}
          swiping={swiping}
        />
      }
    />
  );
}

/**
 * The session's turns, read here rather than by the screen, so a streaming
 * reply re-renders only the transcript and not the header and composer.
 */
function SessionTranscript({
  sessionId,
  projectId,
  worktreePath,
  busy,
  failed,
}: {
  sessionId: string;
  projectId: string;
  /** The working copy the session's files are read from. */
  worktreePath?: string;
  busy: boolean;
  failed: boolean;
}) {
  const { turns, hasOlder, loadOlder } = useConversationTurns(sessionId);
  // The latest error, for the composer; the native transcript reports its own.
  useReportSessionError(
    hasNativeTranscript ? undefined : sessionId,
    busy ? undefined : latestSessionError(turns),
  );
  // A file the agent names opens in the file sheet, as written; the desktop
  // finds it when the name is shortened.
  const openFile = useCallback(
    (path: string) =>
      router.push({
        pathname: "/file",
        params: { projectId, path, ...(worktreePath ? { worktreePath } : {}) },
      }),
    [projectId, worktreePath],
  );
  // Mounted with the screen, ahead of the spinner over it, so its scroll
  // view is the one iOS finds to blur under the native header.
  return (
    <FileOpener value={openFile}>
      {hasNativeTranscript ? <NativeTranscript key={sessionId} conversationId={sessionId} pushed /> : <Transcript
        key={sessionId}
        pushed
        busy={busy}
        turns={turns}
        hasOlder={hasOlder}
        loadOlder={loadOlder}
      />}
      {!hasNativeTranscript && turns.length === 0 && !failed ? (
        <TranscriptLoading key={`loading:${sessionId}`} />
      ) : null}
    </FileOpener>
  );
}

function ProjectHeaderButton({ label, onPress, children }: {
  label: string;
  onPress: () => void;
  children: ReactNode;
}) {
  if (Platform.OS === "android")
    return (
      <LiquidView
        accessibilityLabel={label}
        onPress={onPress}
        borderRadius={22}
        innerStyle={styles.androidProjectButton}
      >
        {children}
      </LiquidView>
    );
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      onPress={onPress}
      style={styles.projectButton}
    >
      {children}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  chat: {
    flex: 1,
  },
  // A bar button's size, the mascot centred in it.
  projectButton: {
    width: 36,
    height: 36,
    alignItems: "center",
    justifyContent: "center",
  },
  androidProjectButton: {
    width: 44,
    height: 44,
    borderRadius: 22,
    alignItems: "center",
    justifyContent: "center",
  },
});
