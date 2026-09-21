import { openUrl } from "@tauri-apps/plugin-opener";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  ArrowDownCircle,
  Check,
  ChevronDown,
  Globe,
  ImagePlus,
  Loader,
  Pencil,
  Plus,
  RefreshCw,
  RotateCcw,
  Search,
  SlidersHorizontal,
  Trash2,
  X,
} from "../../../shared/ui/icons";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { HarnessIcon } from "../../sessions/ui/HarnessIcon";
import {
  ColorPickerPopover,
  ColorSwatchRow,
} from "../../../shared/ui/ColorPickerPopover";
import { Popover } from "../../../shared/ui/Popover";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { JiraSettings } from "./JiraSettings";
import { GradientBlurBackground } from "./GradientBlurBackground";
import { InboxProviderMark } from "../../inbox/ui/InboxProviderMark";
import { RemoveProjectDialog } from "../../projects/ui/RemoveProjectDialog";
import { WindowControls } from "../../../app/shell/WindowControls";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { useColorScheme } from "../../../shared/hooks/useColorScheme";
import {
  applyChatBackground,
  applyChatBackgroundEmptyOpacity,
  applyChatBackgroundSessionOpacity,
  applyChatBackgroundScope,
  applyAccentColor,
  applyBodyGlass,
  applySidebarBlur,
  applySidebarOpacity,
  applyThemeDarkLightness,
  applyThemePreference,
  applyThemeTint,
  BODY_GLASS_DEFAULT,
  ACCENT_COLOR_DEFAULT,
  CHAT_BACKGROUND_EMPTY_OPACITY_DEFAULT,
  CHAT_BACKGROUND_OPACITY_MAX,
  CHAT_BACKGROUND_OPACITY_MIN,
  CHAT_BACKGROUND_SESSION_OPACITY_DEFAULT,
  CHAT_BACKGROUND_SCOPE_DEFAULT,
  THEME_PREFERENCE_DEFAULT,
  chatBackgroundSrc,
  loadBodyGlass,
  loadAccentColor,
  loadChatBackgroundEmptyOpacity,
  loadChatBackgroundPath,
  loadChatBackgroundSessionOpacity,
  loadChatBackgroundScope,
  loadNewThreadBackgroundEffect,
  loadThemeDarkLightness,
  loadThemePreference,
  loadSidebarBlur,
  loadSidebarOpacity,
  loadThemeHue,
  loadThemeSaturation,
  loadTranscriptLayout,
  loadTranscriptAnchor,
  saveBodyGlass,
  saveAccentColor,
  saveChatBackgroundEmptyOpacity,
  saveChatBackgroundPath,
  saveChatBackgroundSessionOpacity,
  saveChatBackgroundScope,
  setNewThreadBackgroundEffect,
  saveThemeDarkLightness,
  saveThemePreference,
  saveSidebarBlur,
  saveSidebarOpacity,
  saveThemeHue,
  saveThemeSaturation,
  saveTranscriptLayout,
  saveTranscriptAnchor,
  TRANSCRIPT_ANCHOR_CHANGE_EVENT,
  loadShowExcludedFiles,
  saveShowExcludedFiles,
  SHOW_EXCLUDED_FILES_DEFAULT,
  SIDEBAR_BLUR_DEFAULT,
  SIDEBAR_BLUR_MAX,
  SIDEBAR_BLUR_MIN,
  SIDEBAR_OPACITY_DEFAULT,
  SIDEBAR_OPACITY_MAX,
  SIDEBAR_OPACITY_MIN,
  THEME_DARK_LIGHTNESS_DEFAULT,
  THEME_DARK_LIGHTNESS_MAX,
  THEME_DARK_LIGHTNESS_MIN,
  THEME_HUE_DEFAULT,
  THEME_HUE_MAX,
  THEME_HUE_MIN,
  THEME_SATURATION_DEFAULT,
  THEME_SATURATION_MAX,
  THEME_SATURATION_MIN,
  type ThemePreference,
  type ChatBackgroundScope,
  NEW_THREAD_BACKGROUND_EFFECTS,
  NEW_THREAD_BACKGROUND_EFFECT_LABELS,
  NEW_THREAD_BACKGROUND_EFFECT_DESCRIPTIONS,
  NEW_THREAD_BACKGROUND_EFFECT_DEFAULT,
  type NewThreadBackgroundEffect,
  type TranscriptLayout,
} from "../model/appearance";
import {
  pickAndSaveChatBackground,
  removeChatBackground,
} from "../../projects/model/chatBackground";
import {
  applyUiScale,
  loadUiScale,
  saveUiScale,
  subscribeUiScale,
  UI_SCALE_DEFAULT,
  UI_SCALE_MAX,
  UI_SCALE_MIN,
} from "../model/uiScale";
import {
  getHarnessAvailabilitySnapshot,
  harnessUnavailableHint,
  isHarnessAvailable,
  probeHarnessAvailability,
  subscribeHarnessAvailability,
} from "../../../integrations/harness/core/availability";
import { refreshHarnessCatalogs } from "../../../integrations/harness/core/registry";
import { loginHarness } from "../../../integrations/harness/core/auth";
import {
  defaultModelId,
  firstEnabledHarness,
  getModelSnapshot,
  loadDefaultModels,
  loadHiddenPickerProviders,
  loadLastModelChoice,
  modelsFor,
  resolveModel,
  saveDefaultModel,
  saveLastModelChoice,
  savePickerProviderVisible,
  subscribeModels,
} from "../../sessions/model/models";
import {
  pathKey,
  prettyCwd,
  projectKey,
  projectName,
} from "../../../shared/lib/paths";
import { IS_MAC, IS_WIN } from "../../../platform/tauri/platform";
import {
  loadArchivedProjects,
  looksLikeProject,
  subscribeArchivedProjects,
  type ArchivedProject,
  type RecentProject,
} from "../../projects/model/recents";
import {
  HARNESSES,
  HARNESS_TITLE,
  sessionDisplayTitle,
  type HarnessId,
} from "../../sessions/model/session";
import {
  loadProjectProviderSettings,
  projectProvidersRevision,
  setProjectDefaultModel,
  setProjectDefaultProvider,
  setProjectProviderHidden,
  subscribeProjectProviders,
} from "../../sessions/model/projectProviders";
import {
  newProviderAccount,
  providerAccounts,
  PROVIDER_ACCOUNT_PROVIDERS,
  removeProviderAccount,
  renameProviderAccount,
  saveProviderAccount,
  subscribeProviderAccounts,
  type ProviderAccount,
  type ProviderAccountProvider,
} from "../../providers/model/providerAccounts";
import { removeProviderAccountCredentials } from "../../providers/model/providerAccountCredentials";
import {
  loadSessionSidebarFilters,
  saveSessionSidebarFilters,
} from "../../sessions/model/sessionFilters";
import type { SessionSummary } from "../../sessions/data/sessionStore";
import {
  clearInboxCache,
  githubStatus,
  type GithubStatus,
} from "../../inbox/model/githubTasks";
import {
  disconnectGitlab,
  gitlabConnected,
  saveGitlabConfig,
} from "../../inbox/model/gitlab";
import {
  azureDevOpsConnected,
  disconnectAzureDevOps,
  saveAzureDevOpsConfig,
} from "../../inbox/model/azureDevOps";
import {
  disconnectLinear,
  LINEAR_CHANGE_EVENT,
  linearConnected,
  listLinearTeams,
  loadHiddenLinearTeamIds,
  notifyLinearChange,
  saveHiddenLinearTeamIds,
  saveLinearToken,
  type LinearTeam,
} from "../../inbox/model/linear";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupLabels,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupLabel,
  resolveTabGroupLogo,
  resolveTabGroupMascot,
} from "../../workspace/model/tabGroups";
import { useTabGroupLogos } from "../../projects/hooks/useTabGroupLogos";
import { ProjectLogoIcon } from "../../projects/ui/ProjectLogoIcon";
import { ProjectMascot } from "../../projects/ui/ProjectMascot";
import {
  filterKeybindings,
  KEYBINDINGS,
  loadClaudeConfigDir,
  loadClaudeHooks,
  loadCloseToTray,
  loadHarnessRuntime,
  loadCollapsedProjectRailMode,
  loadComposerRunner,
  loadDiffViewer,
  loadFileTabMode,
  loadFollowUpBehavior,
  loadFormatOnSave,
  loadGridArcadeEnabled,
  loadLiveAgentsEnabled,
  loadModelControls,
  loadNotesEnabled,
  loadQuickComposerEnabled,
  loadTabAnimationsEnabled,
  saveClaudeConfigDir,
  saveClaudeHooks,
  saveCloseToTray,
  saveHarnessRuntime,
  saveCollapsedProjectRailMode,
  saveComposerRunner,
  saveDiffViewer,
  saveFileTabMode,
  saveFollowUpBehavior,
  saveFormatOnSave,
  saveGridArcadeEnabled,
  saveLiveAgentsEnabled,
  saveModelControls,
  saveNotesEnabled,
  saveQuickComposerEnabled,
  saveTabAnimationsEnabled,
  searchSettings,
  settingsSectionDescription,
  settingsSectionLabel,
  COLLAPSED_PROJECT_RAIL_MODE_DEFAULT,
  type CollapsedProjectRailMode,
  type HarnessRuntimeEnvVar,
  type HarnessRuntimeSettings,
  type DiffViewer,
  type FileTabMode,
  type FollowUpBehavior,
  type ModelControls,
  type SettingsSearchResult,
  type SettingsSectionId,
} from "../model/settings";
import { loadSoundsEnabled, playCue, saveSoundsEnabled } from "../model/sounds";
import { setQuickComposerShortcut } from "../../quick-composer/model/quickComposer";
import {
  cachedNotificationPermission,
  loadNotificationsEnabled,
  openNotificationSettings,
  probeNotificationPermission,
  requestNotificationPermission,
  saveNotificationsEnabled,
  type NotificationPermission,
} from "../../notifications/model/notifications";
import {
  installPendingUpdate,
  readAppVersion,
  runUpdateFlow,
  type UpdaterSnapshot,
} from "../../../app/model/updater";

import { SkillsPage } from "../../skills/ui/SkillsPage";
import { ProjectNotificationSettings } from "../../notifications/ui/ProjectNotificationSettings";
import { WorktreesPage } from "../../source-control/ui/WorktreesPage";
import {
  removeWorktree,
  type RemoveWorktree,
} from "../../source-control/model/worktrees";
import type { Session } from "../../sessions/model/session";

/**
 * The `data-setting-id` Settings should reveal when it opens: one of the ids in
 * `SETTINGS_INDEX`. Inbox integrations pass their provider id.
 */
export type SettingsAnchor = string;

const settingDomId = (id: string) => `setting-${id}`;

/** The row or group Settings just jumped to, so it can flash where you landed. */
const RevealedSetting = createContext<string | null>(null);

type Props = {
  section: SettingsSectionId;
  /** Card to scroll to; the General page is too long to land at the top. */
  anchor?: SettingsAnchor | null;
  /** Project to focus when opening notification settings from a quick action. */
  notificationProjectPath?: string | null;
  /** Changes for each quick action, including repeated requests for one project. */
  notificationSettingsRequest?: number;
  recents?: RecentProject[];
  cwd: string;
  sessions: SessionSummary[];
  liveSessions?: Session[];
  onRemoveWorktree?: RemoveWorktree;
  onCheckWorktreeRemoval?: RemoveWorktree;
  onDeleteWorktreeSessions?: (
    sessionIds: readonly string[],
  ) => Promise<boolean>;
  besideRail?: boolean;
  onClose: () => void;
  /** Lets search jump to a setting that lives on another page. */
  onSelectSection?: (section: SettingsSectionId) => void;
  onOpenSession: (sessionId: string) => void;
  onArchiveSession: (sessionId: string, archived: boolean) => void;
  onDeleteSession: (sessionId: string) => void;
  onRestoreProject?: (path: string) => void;
  onDeleteProject?: (path: string) => void;
  onOpenWhatsNew: (version: string) => void;
  collapsedProjectRailMode?: CollapsedProjectRailMode;
  onCollapsedProjectRailModeChange?: (mode: CollapsedProjectRailMode) => void;
};

export function SettingsView({
  section,
  anchor = null,
  notificationProjectPath = null,
  notificationSettingsRequest = 0,
  recents,
  cwd,
  sessions,
  liveSessions,
  onRemoveWorktree = removeWorktree,
  onCheckWorktreeRemoval,
  onDeleteWorktreeSessions,
  besideRail = false,
  onClose,
  onSelectSection,
  onOpenSession,
  onArchiveSession,
  onDeleteSession,
  onRestoreProject,
  onDeleteProject,
  onOpenWhatsNew,
  collapsedProjectRailMode,
  onCollapsedProjectRailModeChange,
}: Props) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const [revealed, setRevealed] = useState<string | null>(anchor);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const appearance = useAppearanceSettings(
    collapsedProjectRailMode,
    onCollapsedProjectRailModeChange,
  );

  useEffect(() => setRevealed(anchor), [anchor, notificationSettingsRequest]);

  // Section is a dependency so a search result on another page scrolls once
  // that page has mounted the row.
  useEffect(() => {
    if (!revealed) return;
    // A project quick action lets the project card focus itself after discovery.
    if (!(revealed === "project-notifications" && notificationProjectPath)) {
      document
        .getElementById(settingDomId(revealed))
        ?.scrollIntoView?.({ block: "center" });
    }
    const timer = window.setTimeout(() => setRevealed(null), 1800);
    return () => window.clearTimeout(timer);
  }, [revealed, section, notificationProjectPath, notificationSettingsRequest]);

  const onReveal = useCallback(
    (next: SettingsSectionId, settingId: string | null) => {
      if (next !== section) onSelectSection?.(next);
      setRevealed(settingId);
    },
    [onSelectSection, section],
  );

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      event.stopPropagation();
      onCloseRef.current();
    };
    // Let dialogs and other Settings controls handle Escape first.
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div
      role="region"
      aria-label="Settings"
      data-app-settings
      className="flex min-h-0 min-w-0 flex-1 flex-col text-content"
    >
      <div
        className="flex h-10 shrink-0 select-none items-center border-b border-stroke"
        data-tauri-drag-region="deep"
      >
        {IS_MAC && !besideRail ? <div className="w-[78px] shrink-0" /> : null}
        <div className="flex min-w-0 flex-1 items-center gap-2 px-3 text-[13px]">
          <span className="shrink-0 text-content/45">Settings</span>
          <span aria-hidden className="shrink-0 text-content/25">
            /
          </span>
          <span className="min-w-0 truncate text-content">
            {settingsSectionLabel(section)}
          </span>
        </div>
        <div
          className="flex shrink-0 items-center gap-1.5 pr-2"
          data-tauri-drag-region="false"
        >
          {section === "appearance" ? (
            <button
              type="button"
              onClick={appearance.restoreDefaults}
              className="flex shrink-0 items-center gap-1.5 rounded-md px-2 py-1 text-[12px] text-content/50 hover:bg-content/10 hover:text-content"
            >
              <RotateCcw className="size-3.5" strokeWidth={1.75} />
              Restore defaults
            </button>
          ) : null}
          <SettingsSearch onReveal={onReveal} />
        </div>
        {IS_MAC ? null : <WindowControls />}
      </div>

      {section === "skills" ? (
        <SkillsPage
          key={cwd}
          cwd={cwd}
          header={
            <PageHeader
              title={settingsSectionLabel(section)}
              description={settingsSectionDescription(section)}
            />
          }
        />
      ) : (
        <RevealedSetting.Provider value={revealed}>
          <div
            ref={lockOverscroll}
            className="@container/settings min-h-0 flex-1 overflow-y-auto overscroll-none"
          >
            <div className="mx-auto w-full max-w-5xl px-5 py-6 pb-16 @min-[560px]/settings:px-8 @min-[560px]/settings:py-8">
              <PageHeader
                title={settingsSectionLabel(section)}
                description={settingsSectionDescription(section)}
              />
              {section === "general" ? (
                <GeneralPage onOpenWhatsNew={onOpenWhatsNew} />
              ) : null}
              {section === "appearance" ? (
                <AppearancePage appearance={appearance} />
              ) : null}
              {section === "chat" ? <ChatPage /> : null}
              {section === "keybindings" ? <KeybindingsPage /> : null}
              {section === "providers" ? (
                <ProvidersPage cwd={cwd} recents={recents} />
              ) : null}
              {section === "worktrees" ? (
                <WorktreesPage
                  cwd={cwd}
                  recents={recents}
                  liveSessions={liveSessions}
                  onRemove={onRemoveWorktree}
                  onCheckRemove={onCheckWorktreeRemoval}
                  onDeleteSessions={onDeleteWorktreeSessions}
                />
              ) : null}
              {section === "inbox" ? (
                <InboxPage
                  cwd={cwd}
                  recents={recents}
                  notificationProjectPath={notificationProjectPath}
                  notificationSettingsRequest={notificationSettingsRequest}
                />
              ) : null}
              {section === "archive" ? (
                <ArchivePage
                  cwd={cwd}
                  sessions={sessions}
                  onOpenSession={onOpenSession}
                  onArchiveSession={onArchiveSession}
                  onDeleteSession={onDeleteSession}
                  onRestoreProject={onRestoreProject}
                  onDeleteProject={onDeleteProject}
                />
              ) : null}
            </div>
          </div>
        </RevealedSetting.Provider>
      )}
    </div>
  );
}

/** Jumps to any setting by name, including ones on another page. */
function SettingsSearch({
  onReveal,
}: {
  onReveal: (section: SettingsSectionId, settingId: string | null) => void;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const listId = useId();
  const results = useMemo(() => searchSettings(query), [query]);
  const open = query.trim().length > 0;

  useEffect(() => setActive(0), [query]);

  const go = (result: SettingsSearchResult | undefined) => {
    if (!result) return;
    onReveal(result.section, result.settingId);
    setQuery("");
    input.current?.blur();
  };

  const onKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((index) => Math.min(results.length - 1, index + 1));
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((index) => Math.max(0, index - 1));
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      go(results[active]);
    }
  };

  return (
    <div ref={root} className="relative shrink-0">
      <label className="flex h-7 w-48 items-center gap-2 rounded-md border border-content/10 px-2 text-content/45 focus-within:border-content/20">
        <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
        <input
          ref={input}
          role="combobox"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Search settings"
          aria-label="Search settings"
          aria-expanded={open}
          aria-controls={listId}
          spellCheck={false}
          autoComplete="off"
          className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
        />
        {query ? (
          <button
            type="button"
            aria-label="Clear settings search"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => {
              setQuery("");
              input.current?.focus();
            }}
            className="grid size-4 shrink-0 place-items-center rounded text-content/45 hover:text-content"
          >
            <X className="size-3" strokeWidth={2} />
          </button>
        ) : null}
      </label>
      {open ? (
        <Popover
          anchor={root}
          side="bottom"
          align="end"
          width={300}
          maxHeight={320}
          onDismiss={(reason) => {
            setQuery("");
            if (reason === "escape") input.current?.focus();
          }}
          id={listId}
          role="listbox"
          aria-label="Settings search results"
          className="overflow-y-auto overscroll-contain p-1"
        >
          {results.length === 0 ? (
            <p className="px-2 py-1.5 text-[12px] text-content/45">
              No matching settings
            </p>
          ) : (
            results.map((result, index) => (
              <button
                key={`${result.section}:${result.settingId ?? "*"}`}
                type="button"
                role="option"
                aria-selected={index === active}
                onMouseDown={(event) => event.preventDefault()}
                onMouseEnter={() => setActive(index)}
                onClick={() => go(result)}
                className={`flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-[12px] ${
                  index === active
                    ? "bg-selection text-content"
                    : "text-content hover:bg-content/5"
                }`}
              >
                <span className="min-w-0 flex-1 truncate">{result.label}</span>
                <span className="shrink-0 text-[11px] text-content/40">
                  {result.settingId ? result.sectionLabel : "Page"}
                </span>
              </button>
            ))
          )}
        </Popover>
      ) : null}
    </div>
  );
}

function GeneralPage({
  onOpenWhatsNew,
}: {
  onOpenWhatsNew: (version: string) => void;
}) {
  const [soundsEnabled, setSoundsEnabled] = useState(loadSoundsEnabled);
  const [notificationsEnabled, setNotificationsEnabled] = useState(
    loadNotificationsEnabled,
  );
  const [notificationPermission, setNotificationPermission] =
    useState<NotificationPermission>(cachedNotificationPermission);
  const [notesEnabled, setNotesEnabled] = useState(loadNotesEnabled);
  const [liveAgentsEnabled, setLiveAgentsEnabled] = useState(
    loadLiveAgentsEnabled,
  );
  const [fileTabMode, setFileTabMode] = useState<FileTabMode>(loadFileTabMode);
  const [tabAnimationsEnabled, setTabAnimationsEnabled] = useState(
    loadTabAnimationsEnabled,
  );
  const [closeToTray, setCloseToTray] = useState(loadCloseToTray);
  const [quickComposerEnabled, setQuickComposerEnabled] = useState(
    loadQuickComposerEnabled,
  );
  const [quickComposerError, setQuickComposerError] = useState<string | null>(
    null,
  );

  // The user may flip the switch in System Settings and come back: re-read
  // the OS state whenever the window regains focus while the toggle is on.
  useEffect(() => {
    if (!notificationsEnabled) return;
    const refresh = () => {
      void probeNotificationPermission().then(setNotificationPermission);
    };
    refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [notificationsEnabled]);

  const onSoundsEnabled = (next: boolean) => {
    saveSoundsEnabled(next);
    setSoundsEnabled(next);
  };

  const onNotificationsEnabled = (next: boolean) => {
    saveNotificationsEnabled(next);
    setNotificationsEnabled(next);
    if (!next) return;
    void requestNotificationPermission().then(setNotificationPermission);
  };

  const onNotesEnabled = (next: boolean) => {
    saveNotesEnabled(next);
    setNotesEnabled(next);
  };

  const onQuickComposerEnabled = (next: boolean) => {
    saveQuickComposerEnabled(next);
    setQuickComposerEnabled(next);
    setQuickComposerError(null);
    void setQuickComposerShortcut(next).catch((error: unknown) => {
      // Another app already owns the combination. Leave the switch where the
      // user put it so the next launch tries again, but say why it is dead.
      setQuickComposerError(String(error));
    });
  };

  const onLiveAgentsEnabled = (next: boolean) => {
    saveLiveAgentsEnabled(next);
    setLiveAgentsEnabled(next);
  };

  const onFileTabMode = (next: FileTabMode) => {
    saveFileTabMode(next);
    setFileTabMode(next);
  };

  const onTabAnimationsEnabled = (next: boolean) => {
    saveTabAnimationsEnabled(next);
    setTabAnimationsEnabled(next);
  };

  const onCloseToTray = (next: boolean) => {
    saveCloseToTray(next);
    setCloseToTray(next);
  };

  return (
    <>
      <Group
        title="Alerts"
        description="How MonoCode reaches you while you are looking somewhere else."
      >
        <Row
          id="sounds"
          label="Sounds"
          description="Short cues for project activity, finished turns, and available updates. Choose project notification categories in Inbox settings. Switches and Copy on a finished turn also play."
        >
          <Toggle
            label="Sounds"
            on={soundsEnabled}
            onChange={onSoundsEnabled}
          />
        </Row>
        <Row
          id="notifications"
          label="Notifications"
          description="Notify when a reminder is due, or when an agent finishes or needs input in another session or while MonoCode is in the background. Click the notification to open that session."
        >
          {notificationsEnabled && notificationPermission === "denied" ? (
            <NotificationsBlocked />
          ) : null}
          {notificationsEnabled && notificationPermission === "unsupported" ? (
            <span className="text-[12px] text-content/45">
              Not available on this platform
            </span>
          ) : null}
          <Toggle
            label="Notifications"
            on={notificationsEnabled}
            onChange={onNotificationsEnabled}
          />
        </Row>
      </Group>

      <Group
        title="Workspace"
        description="How project navigation and workspace tabs behave."
      >
        <Row
          id="file-tabs"
          label="File tabs"
          description="Open files beside the active chat, or give each file a normal tab in the top bar. Top-bar files can still be combined into split panes."
        >
          <Segmented
            label="File tabs"
            value={fileTabMode}
            options={[
              { value: "pane", label: "Beside chat" },
              { value: "workspace", label: "Top bar" },
            ]}
            onChange={onFileTabMode}
          />
        </Row>
        <Row
          id="tab-animations"
          label="Tab animations"
          description="Animate tabs as they open and close. Turn this off for instant tab changes."
        >
          <Toggle
            label="Tab animations"
            on={tabAnimationsEnabled}
            onChange={onTabAnimationsEnabled}
          />
        </Row>
        <Row
          id="notes"
          label="Notes"
          description="A global markdown notebook on the project rail. Save a finished turn from the transcript, then mention it later with @note or add it to chat."
        >
          <Toggle label="Notes" on={notesEnabled} onChange={onNotesEnabled} />
        </Row>
        {IS_MAC && (
          <Row
            id="quick-composer"
            label="Quick composer"
            description="Press ⌘⇧Space in any app to float a prompt over it and start a session without switching to MonoCode. Return starts it in the background; ⌘Return starts it and brings the session forward."
          >
            {quickComposerError ? (
              <span className="text-[12px] text-content/45">
                {quickComposerError}
              </span>
            ) : null}
            <Toggle
              label="Quick composer"
              on={quickComposerEnabled}
              onChange={onQuickComposerEnabled}
            />
          </Row>
        )}
        <Row
          id="working-agents"
          label="Working agents"
          description="When two or more chats are in flight, a card on the project rail lists them so you can jump across projects. Finished turns stay until you open that session."
        >
          <Toggle
            label="Working agents"
            on={liveAgentsEnabled}
            onChange={onLiveAgentsEnabled}
          />
        </Row>
        {IS_WIN && (
          <Row
            id="close-to-tray"
            label="Close to tray"
            description="Closing a window hides it to the system tray instead of quitting, so running agents keep going. Reopen from the tray icon, and quit for real from its menu. Turn this off to have close end the window."
          >
            <Toggle
              label="Close to tray"
              on={closeToTray}
              onChange={onCloseToTray}
            />
          </Row>
        )}
      </Group>

      <Group title="About">
        <UpdateRow onOpenWhatsNew={onOpenWhatsNew} />
      </Group>
    </>
  );
}

function ChatPage() {
  const [transcriptLayout, setTranscriptLayout] =
    useState<TranscriptLayout>(loadTranscriptLayout);
  const [transcriptAnchor, setTranscriptAnchor] =
    useState(loadTranscriptAnchor);
  const [followUpBehavior, setFollowUpBehavior] =
    useState<FollowUpBehavior>(loadFollowUpBehavior);
  const [modelControls, setModelControls] =
    useState<ModelControls>(loadModelControls);
  const [diffViewer, setDiffViewer] = useState<DiffViewer>(loadDiffViewer);
  const [formatOnSave, setFormatOnSave] = useState(loadFormatOnSave);
  const [composerRunner, setComposerRunner] = useState(loadComposerRunner);
  const [gridArcadeEnabled, setGridArcadeEnabled] = useState(
    loadGridArcadeEnabled,
  );

  useEffect(() => {
    const onAnchor = (event: Event) => {
      setTranscriptAnchor((event as CustomEvent<boolean>).detail === true);
    };
    window.addEventListener(TRANSCRIPT_ANCHOR_CHANGE_EVENT, onAnchor);
    return () => {
      window.removeEventListener(TRANSCRIPT_ANCHOR_CHANGE_EVENT, onAnchor);
    };
  }, []);

  const onTranscriptLayout = (next: TranscriptLayout) => {
    saveTranscriptLayout(next);
    setTranscriptLayout(next);
  };

  const onTranscriptAnchor = (next: boolean) => {
    saveTranscriptAnchor(next);
    setTranscriptAnchor(next);
  };

  const onFollowUpBehavior = (next: FollowUpBehavior) => {
    saveFollowUpBehavior(next);
    setFollowUpBehavior(next);
  };

  const onModelControls = (next: ModelControls) => {
    saveModelControls(next);
    setModelControls(next);
  };

  const onDiffViewer = (next: DiffViewer) => {
    saveDiffViewer(next);
    setDiffViewer(next);
  };

  const onFormatOnSave = (next: boolean) => {
    saveFormatOnSave(next);
    setFormatOnSave(next);
  };

  const onComposerRunner = (next: boolean) => {
    saveComposerRunner(next);
    setComposerRunner(next);
  };

  const onGridArcadeEnabled = (next: boolean) => {
    saveGridArcadeEnabled(next);
    setGridArcadeEnabled(next);
  };

  return (
    <>
      <Group
        title="Transcript"
        description="How a conversation reads as it grows."
      >
        <Row
          id="transcript-layout"
          label="Transcript layout"
          description="Full width keeps user prompts as a spanning card. Chat aligns them to the right with a max width, like a messaging app."
        >
          <Segmented
            label="Transcript layout"
            value={transcriptLayout}
            options={[
              { value: "full", label: "Full width" },
              { value: "chat", label: "Chat" },
            ]}
            onChange={onTranscriptLayout}
          />
        </Row>
        <Row
          id="anchor-prompts"
          label="Anchor prompts to top"
          description="When you send, the new prompt sits at the top of the transcript and the reply grows into the space below. Turn this off to keep the classic layout, with the latest message resting on the composer."
        >
          <Toggle
            label="Anchor prompts to top"
            on={transcriptAnchor}
            onChange={onTranscriptAnchor}
          />
        </Row>
      </Group>

      <Group
        title="Composer"
        description="What the composer does with what you type."
      >
        <Row
          id="follow-up"
          label="Follow-up behavior"
          description="Queue follow-ups until the active turn finishes, or steer the active turn immediately."
        >
          <Segmented
            label="Follow-up behavior"
            value={followUpBehavior}
            options={[
              { value: "queue", label: "Queue" },
              { value: "steer", label: "Steer" },
            ]}
            onChange={onFollowUpBehavior}
          />
        </Row>
        <Row
          id="model-controls"
          label="Model controls"
          description="Show model options beside the picker instead of inside the model menu."
        >
          <Segmented
            label="Model controls"
            value={modelControls}
            options={[
              { value: "menu", label: "Menu" },
              { value: "beside", label: "Beside" },
            ]}
            onChange={onModelControls}
          />
        </Row>
      </Group>

      <Group
        title="Editor"
        description="What happens when you save a file in the workspace editor."
      >
        <Row
          id="format-on-save"
          label="Format on save"
          description="Run Prettier on supported files before writing. Off keeps the text you typed, including quote style."
        >
          <Toggle
            label="Format on save"
            on={formatOnSave}
            onChange={onFormatOnSave}
          />
        </Row>
      </Group>

      <Group
        title="Code review"
        description="Where a turn's changes open when you go to read them."
      >
        <Row
          id="diff-view"
          label="Diff view"
          description="Editor keeps working-tree changes in the file. Unified stacks every changed file in one review, with sticky headers and collapsed unchanged lines."
        >
          <Segmented
            label="Diff view"
            value={diffViewer}
            options={[
              { value: "editor", label: "Editor" },
              { value: "unified", label: "Unified" },
            ]}
            onChange={onDiffViewer}
          />
        </Row>
      </Group>

      <Group
        title="Extras"
        description="Idle animation, and nothing else. Turn both off for a still workspace."
      >
        <Row
          id="composer-mascot"
          label="Composer mascot"
          description="When a turn is running, the project mascot runs along the composer, bonks the scroll-to-latest button the first time, then jumps it, and sometimes grabs a coin."
        >
          <Toggle
            label="Composer mascot"
            on={composerRunner}
            onChange={onComposerRunner}
          />
        </Row>
        <Row
          id="empty-session-games"
          label="Empty session games"
          description="Pac-man and snake idle on the empty-session grid. Hover the band to take control of whichever is on screen. Turn this off to keep the pane still."
        >
          <Toggle
            label="Empty session games"
            on={gridArcadeEnabled}
            onChange={onGridArcadeEnabled}
          />
        </Row>
      </Group>
    </>
  );
}

function InboxPage({
  cwd,
  recents,
  notificationProjectPath,
  notificationSettingsRequest,
}: {
  cwd: string;
  recents?: RecentProject[];
  notificationProjectPath?: string | null;
  notificationSettingsRequest?: number;
}) {
  const revealed = useContext(RevealedSetting);
  return (
    <>
      <div
        id={settingDomId("project-notifications")}
        data-setting-id="project-notifications"
      >
        <ProjectNotificationSettings
          cwd={cwd}
          recents={recents}
          notificationProjectPath={notificationProjectPath}
          notificationSettingsRequest={notificationSettingsRequest}
          highlighted={revealed === "project-notifications"}
        />
      </div>
      <Group
        id="github"
        title={
          <span className="flex items-center gap-2">
            <InboxProviderMark provider="github" className="size-4 shrink-0" />
            GitHub
          </span>
        }
        description="Pull requests, reviews, and issues, read through the GitHub CLI."
      >
        <GithubSettings />
      </Group>

      <Group
        id="gitlab"
        title={
          <span className="flex items-center gap-2">
            <InboxProviderMark provider="gitlab" className="size-4 shrink-0" />
            GitLab
          </span>
        }
        description="Merge requests from GitLab.com or a self-managed instance."
      >
        <GitlabSettings />
      </Group>

      <Group
        id="azuredevops"
        title={
          <span className="flex items-center gap-2">
            <InboxProviderMark
              provider="azuredevops"
              className="size-4 shrink-0"
            />
            ADO
          </span>
        }
        description="Pull requests and Boards work items from your ADO organization."
      >
        <AzureDevOpsSettings />
      </Group>

      <Group
        id="jira"
        title={
          <span className="flex items-center gap-2">
            <InboxProviderMark provider="jira" className="size-4 shrink-0" />
            Jira
          </span>
        }
        description="Jira Cloud issues from the projects you pick."
      >
        <JiraSettings />
      </Group>

      <Group
        id="linear"
        title={
          <span className="flex items-center gap-2">
            <InboxProviderMark provider="linear" className="size-4 shrink-0" />
            Linear
          </span>
        }
        description="Issues assigned to you, from the teams you pick."
      >
        <LinearSettings />
      </Group>
    </>
  );
}

function GithubSettings() {
  const [status, setStatus] = useState<GithubStatus | null>(null);
  const [checking, setChecking] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);

  const checkStatus = useCallback(async () => {
    const generation = ++request.current;
    setChecking(true);
    setError(null);
    try {
      const next = await githubStatus();
      if (generation === request.current) setStatus(next);
    } catch (err: unknown) {
      if (generation === request.current) {
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      if (generation === request.current) setChecking(false);
    }
  }, []);

  useEffect(() => {
    void checkStatus();
    return () => {
      request.current += 1;
    };
  }, [checkStatus]);

  const description = status?.connected
    ? "GitHub CLI is installed and authenticated. MonoCode uses it for GitHub inbox items."
    : status?.installed
      ? "Run gh auth login in a terminal, complete the sign-in flow, then check again."
      : "Install GitHub CLI from cli.github.com, run gh auth login in a terminal, then check again.";
  const label = checking
    ? "Checking"
    : status?.connected
      ? "Connected"
      : status?.installed
        ? "Sign in required"
        : "Not installed";

  return (
    <>
      <Row label="Connection" description={description}>
        <span className="text-[12px] text-content/50">{label}</span>
        {!checking && !status?.installed ? (
          <SecondaryButton
            onClick={() => {
              void openUrl("https://cli.github.com/").catch(() => {});
            }}
          >
            Installation guide
          </SecondaryButton>
        ) : null}
        <SecondaryButton onClick={() => void checkStatus()} disabled={checking}>
          {checking ? "Checking" : "Check again"}
        </SecondaryButton>
      </Row>
      {error ? (
        <p className="border-b border-content/5 px-4 pb-3 text-[12px] text-red-400/90 last:border-b-0">
          {error}
        </p>
      ) : null}
    </>
  );
}

function GitlabSettings() {
  const [url, setUrl] = useState("https://gitlab.com");
  const [token, setToken] = useState("");
  const [connected, setConnected] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void gitlabConnected()
      .then((status) => {
        if (cancelled) return;
        setConnected(status.connected);
        setUrl(status.url);
      })
      .catch((err: unknown) => {
        if (!cancelled)
          setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const onSave = async () => {
    if (!token.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      const status = await saveGitlabConfig(url, token);
      setUrl(status.url);
      setToken("");
      setConnected(status.connected);
      clearInboxCache();
    } catch (err: unknown) {
      setConnected(false);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const onDisconnect = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const status = await disconnectGitlab(url);
      setConnected(false);
      setUrl(status.url);
      clearInboxCache();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Row
        label="Connection"
        description="Connect GitLab.com or a self-managed GitLab instance. Use a personal access token with API access; the token is stored locally and Disconnect deletes it."
      >
        {connected ? (
          <div className="flex min-w-0 max-w-full flex-wrap items-center gap-2">
            <span className="max-w-56 truncate text-[12px] text-content/50">
              {url}
            </span>
            <SecondaryButton
              onClick={() => void onDisconnect()}
              disabled={busy}
            >
              Disconnect
            </SecondaryButton>
          </div>
        ) : (
          <div className="flex min-w-0 max-w-full flex-wrap items-center gap-2">
            <label className="flex h-7 w-52 max-w-full shrink-0 items-center rounded-md border border-content/10 px-2 focus-within:border-content/20">
              <input
                type="url"
                value={url}
                onChange={(event) => setUrl(event.target.value)}
                placeholder="https://gitlab.com"
                aria-label="GitLab URL"
                autoComplete="url"
                spellCheck={false}
                className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
              />
            </label>
            <label className="flex h-7 w-52 max-w-full shrink-0 items-center rounded-md border border-content/10 px-2 focus-within:border-content/20">
              <input
                type="password"
                value={token}
                onChange={(event) => setToken(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void onSave();
                }}
                placeholder="glpat-…"
                aria-label="GitLab access token"
                autoComplete="off"
                spellCheck={false}
                className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
              />
            </label>
            <SecondaryButton
              onClick={() => void onSave()}
              disabled={busy || !token.trim()}
            >
              {busy ? "Saving" : "Connect"}
            </SecondaryButton>
          </div>
        )}
      </Row>
      {error ? (
        <p className="border-b border-content/5 px-4 pb-3 text-[12px] text-red-400/90 last:border-b-0">
          {error}
        </p>
      ) : null}
    </>
  );
}

function AzureDevOpsSettings() {
  const [url, setUrl] = useState("https://dev.azure.com/myorg");
  const [token, setToken] = useState("");
  const [connected, setConnected] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void azureDevOpsConnected()
      .then((status) => {
        if (cancelled) return;
        setConnected(status.connected);
        if (status.url) setUrl(status.url);
      })
      .catch((err: unknown) => {
        if (!cancelled)
          setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const onSave = async () => {
    if (!token.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      const status = await saveAzureDevOpsConfig(url, token);
      setUrl(status.url);
      setToken("");
      setConnected(status.connected);
      clearInboxCache();
    } catch (err: unknown) {
      setConnected(false);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const onDisconnect = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const status = await disconnectAzureDevOps(url);
      setConnected(false);
      setUrl(status.url || url);
      clearInboxCache();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Row
        label="Connection"
        description="Connect your ADO organization with a personal access token (Boards + Repos read & write for comments). The token is stored locally and Disconnect deletes it."
      >
        {connected ? (
          <div className="flex min-w-0 max-w-full flex-wrap items-center gap-2">
            <span className="max-w-56 truncate text-[12px] text-content/50">
              {url}
            </span>
            <SecondaryButton
              onClick={() => void onDisconnect()}
              disabled={busy}
            >
              Disconnect
            </SecondaryButton>
          </div>
        ) : (
          <div className="flex min-w-0 max-w-full flex-wrap items-center gap-2">
            <label className="flex h-7 w-52 max-w-full shrink-0 items-center rounded-md border border-content/10 px-2 focus-within:border-content/20">
              <input
                type="url"
                value={url}
                onChange={(event) => setUrl(event.target.value)}
                placeholder="https://dev.azure.com/myorg"
                aria-label="Azure DevOps organization URL"
                autoComplete="url"
                spellCheck={false}
                className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
              />
            </label>
            <label className="flex h-7 w-52 max-w-full shrink-0 items-center rounded-md border border-content/10 px-2 focus-within:border-content/20">
              <input
                type="password"
                value={token}
                onChange={(event) => setToken(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void onSave();
                }}
                placeholder="PAT…"
                aria-label="Azure DevOps personal access token"
                autoComplete="off"
                spellCheck={false}
                className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
              />
            </label>
            <SecondaryButton
              onClick={() => void onSave()}
              disabled={busy || !token.trim()}
            >
              {busy ? "Saving" : "Connect"}
            </SecondaryButton>
          </div>
        )}
      </Row>
      {error ? (
        <p className="border-b border-content/5 px-4 pb-3 text-[12px] text-red-400/90 last:border-b-0">
          {error}
        </p>
      ) : null}
    </>
  );
}

function LinearSettings() {
  const [token, setToken] = useState("");
  const [connected, setConnected] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [teams, setTeams] = useState<LinearTeam[]>([]);
  const [hiddenTeamIds, setHiddenTeamIds] = useState(loadHiddenLinearTeamIds);

  const loadTeams = useCallback(async () => {
    try {
      const next = await listLinearTeams();
      setTeams(next);
    } catch {
      setTeams([]);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    void linearConnected()
      .then((status) => {
        if (cancelled) return;
        setConnected(status.connected);
        if (status.connected) void loadTeams();
      })
      .catch(() => {
        if (!cancelled) setConnected(false);
      });
    return () => {
      cancelled = true;
    };
  }, [loadTeams]);

  // The inbox filter menu writes the same list, so follow it while both are mounted.
  useEffect(() => {
    const onChange = () => setHiddenTeamIds(loadHiddenLinearTeamIds());
    window.addEventListener(LINEAR_CHANGE_EVENT, onChange);
    return () => window.removeEventListener(LINEAR_CHANGE_EVENT, onChange);
  }, []);

  const onSave = async () => {
    if (!token.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await saveLinearToken(token);
      setToken("");
      setConnected(true);
      clearInboxCache();
      notifyLinearChange();
      await loadTeams();
    } catch (err: unknown) {
      setConnected(false);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const onDisconnect = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await disconnectLinear();
      setConnected(false);
      setTeams([]);
      clearInboxCache();
      notifyLinearChange();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const toggleTeam = (id: string) => {
    const next = new Set(hiddenTeamIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    const ids = [...next];
    setHiddenTeamIds(ids);
    saveHiddenLinearTeamIds(ids);
    clearInboxCache();
  };

  return (
    <>
      <Row
        label="API key"
        description="Create a personal API key in Linear → Settings → Security & Access. Disconnect deletes it."
      >
        {connected ? (
          <SecondaryButton onClick={() => void onDisconnect()} disabled={busy}>
            Disconnect
          </SecondaryButton>
        ) : (
          <div className="flex max-w-full flex-wrap items-center gap-2">
            <label className="flex h-7 w-52 max-w-full shrink-0 items-center rounded-md border border-content/10 px-2 focus-within:border-content/20">
              <input
                type="password"
                value={token}
                onChange={(event) => setToken(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void onSave();
                }}
                placeholder="lin_api_…"
                aria-label="Linear API key"
                autoComplete="off"
                spellCheck={false}
                className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
              />
            </label>
            <SecondaryButton
              onClick={() => void onSave()}
              disabled={busy || !token.trim()}
            >
              {busy ? "Saving" : "Connect"}
            </SecondaryButton>
          </div>
        )}
      </Row>
      {error ? (
        <p className="border-b border-content/5 px-4 pb-3 text-[12px] text-red-400/90 last:border-b-0">
          {error}
        </p>
      ) : null}
      {connected && teams.length > 0 ? (
        <div className="border-b border-content/5 px-4 py-3.5 last:border-b-0">
          <div className="text-[13px] font-medium text-content">Teams</div>
          <p className="mt-1 text-[12px] leading-relaxed text-content/45">
            Unchecked teams stay out of the inbox.
          </p>
          <div className="-mx-2 mt-2 flex flex-col gap-0.5">
            {teams.map((team) => {
              const checked = !hiddenTeamIds.includes(team.id);
              return (
                <button
                  key={team.id}
                  type="button"
                  onClick={() => toggleTeam(team.id)}
                  className="flex h-7 items-center gap-2 rounded-md px-2 text-left text-[13px] text-content hover:bg-content/5"
                >
                  <span className="min-w-0 flex-1 truncate">
                    {team.name}
                    {team.key ? (
                      <span className="ml-1.5 text-content/40">{team.key}</span>
                    ) : null}
                  </span>
                  {checked ? (
                    <Check className="size-3.5 shrink-0" strokeWidth={2.25} />
                  ) : null}
                </button>
              );
            })}
          </div>
        </div>
      ) : null}
    </>
  );
}

function UpdateRow({
  onOpenWhatsNew,
}: {
  onOpenWhatsNew: (version: string) => void;
}) {
  const [snapshot, setSnapshot] = useState<UpdaterSnapshot>({
    phase: "idle",
    currentVersion: "…",
  });

  useEffect(() => {
    let cancelled = false;
    void readAppVersion().then((currentVersion) => {
      if (cancelled) return;
      setSnapshot((current) => ({ ...current, currentVersion }));
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const busy =
    snapshot.phase === "checking" || snapshot.phase === "downloading";
  const hasUpdate = snapshot.phase === "available";

  const onClick = async () => {
    if (busy) return;
    if (hasUpdate) {
      await installPendingUpdate(setSnapshot);
      return;
    }
    await runUpdateFlow(true, setSnapshot);
  };

  const status =
    snapshot.phase === "available"
      ? `Version ${snapshot.availableVersion} is available.`
      : snapshot.phase === "downloading"
        ? `Downloading${snapshot.progress != null ? ` ${snapshot.progress}%` : "…"}`
        : snapshot.phase === "checking"
          ? "Checking for updates…"
          : snapshot.phase === "current"
            ? "You're on the latest version."
            : snapshot.phase === "error"
              ? (snapshot.error ?? "Update check failed.")
              : "MonoCode updates itself from the release feed.";

  return (
    <Row
      id="update"
      label={
        <span className="flex items-baseline gap-2">
          Version
          <span className="font-mono text-[12px] text-content/45">
            {snapshot.currentVersion}
          </span>
        </span>
      }
      description={status}
    >
      <div className="flex items-center gap-2">
        <SecondaryButton
          onClick={() => onOpenWhatsNew(snapshot.currentVersion)}
          disabled={snapshot.currentVersion === "…"}
        >
          What's new
        </SecondaryButton>
        <SecondaryButton onClick={() => void onClick()} disabled={busy}>
          {busy ? (
            <Loader className="size-3.5 animate-spin" aria-hidden />
          ) : hasUpdate ? (
            <ArrowDownCircle className="size-3.5 text-accent" aria-hidden />
          ) : (
            <RefreshCw className="size-3.5" strokeWidth={1.75} aria-hidden />
          )}
          {hasUpdate ? "Download" : "Check for updates"}
        </SecondaryButton>
      </div>
    </Row>
  );
}

type AppearanceSettings = ReturnType<typeof useAppearanceSettings>;

function useAppearanceSettings(
  controlledCollapsedProjectRailMode?: CollapsedProjectRailMode,
  onControlledCollapsedProjectRailModeChange?: (
    mode: CollapsedProjectRailMode,
  ) => void,
) {
  const [themePreference, setThemePreference] =
    useState<ThemePreference>(loadThemePreference);
  const [accentColor, setAccentColor] = useState(loadAccentColor);
  const [opacity, setOpacity] = useState(loadSidebarOpacity);
  const [blur, setBlur] = useState(loadSidebarBlur);
  const [themeHue, setThemeHue] = useState(loadThemeHue);
  const [themeSaturation, setThemeSaturation] = useState(loadThemeSaturation);
  const [themeDarkLightness, setThemeDarkLightness] = useState(
    loadThemeDarkLightness,
  );
  const [bodyGlass, setBodyGlass] = useState(loadBodyGlass);
  const [showExcludedFiles, setShowExcludedFiles] = useState(
    loadShowExcludedFiles,
  );
  const [chatBackgroundPath, setChatBackgroundPath] = useState(
    loadChatBackgroundPath,
  );
  const [chatBackgroundEmptyOpacity, setChatBackgroundEmptyOpacity] = useState(
    loadChatBackgroundEmptyOpacity,
  );
  const [chatBackgroundSessionOpacity, setChatBackgroundSessionOpacity] =
    useState(loadChatBackgroundSessionOpacity);
  const [chatBackgroundScope, setChatBackgroundScope] =
    useState<ChatBackgroundScope>(loadChatBackgroundScope);
  const [newThreadBackgroundEffect, setBackgroundEffect] =
    useState<NewThreadBackgroundEffect>(loadNewThreadBackgroundEffect);
  const [chatBackgroundBusy, setChatBackgroundBusy] = useState(false);
  const [chatBackgroundError, setChatBackgroundError] = useState<string | null>(
    null,
  );
  const [uiScale, setUiScale] = useState(loadUiScale);
  const [storedCollapsedProjectRailMode, setStoredCollapsedProjectRailMode] =
    useState<CollapsedProjectRailMode>(loadCollapsedProjectRailMode);
  const collapsedProjectRailMode =
    controlledCollapsedProjectRailMode ?? storedCollapsedProjectRailMode;

  useEffect(() => subscribeUiScale(() => setUiScale(loadUiScale())), []);

  const onThemePreference = useCallback((next: ThemePreference) => {
    applyThemePreference(next);
    saveThemePreference(next);
    setThemePreference(next);
  }, []);

  const onAccentColor = useCallback((value: string | null) => {
    const next = applyAccentColor(value);
    saveAccentColor(next);
    setAccentColor(next);
  }, []);

  const onOpacity = useCallback((percent: number) => {
    const next = applySidebarOpacity(percent / 100);
    saveSidebarOpacity(next);
    setOpacity(next);
  }, []);

  const onBlur = useCallback((radius: number) => {
    const next = applySidebarBlur(radius);
    saveSidebarBlur(next);
    setBlur(next);
  }, []);

  const onTint = useCallback((hue: number, saturation: number) => {
    const next = applyThemeTint(hue, saturation);
    saveThemeHue(next.hue);
    saveThemeSaturation(next.saturation);
    setThemeHue(next.hue);
    setThemeSaturation(next.saturation);
  }, []);

  const onDarkLightness = useCallback((value: number) => {
    const next = applyThemeDarkLightness(value);
    saveThemeDarkLightness(next);
    setThemeDarkLightness(next);
  }, []);

  const onBodyGlass = useCallback((next: boolean) => {
    applyBodyGlass(next);
    saveBodyGlass(next);
    setBodyGlass(next);
  }, []);

  const onShowExcludedFiles = useCallback((next: boolean) => {
    saveShowExcludedFiles(next);
    setShowExcludedFiles(next);
  }, []);

  const onChooseChatBackground = useCallback(async () => {
    setChatBackgroundBusy(true);
    setChatBackgroundError(null);
    try {
      const path = await pickAndSaveChatBackground();
      if (!path) return;
      saveChatBackgroundPath(path);
      applyChatBackground(path);
      setChatBackgroundPath(path);
    } catch (error) {
      setChatBackgroundError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setChatBackgroundBusy(false);
    }
  }, []);

  const onClearChatBackground = useCallback(async () => {
    setChatBackgroundBusy(true);
    setChatBackgroundError(null);
    try {
      await removeChatBackground();
      saveChatBackgroundPath(null);
      applyChatBackground(null);
      setChatBackgroundPath(null);
    } catch (error) {
      setChatBackgroundError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setChatBackgroundBusy(false);
    }
  }, []);

  const onChatBackgroundEmptyOpacity = useCallback((percent: number) => {
    const next = applyChatBackgroundEmptyOpacity(percent / 100);
    saveChatBackgroundEmptyOpacity(next);
    setChatBackgroundEmptyOpacity(next);
  }, []);

  const onChatBackgroundSessionOpacity = useCallback((percent: number) => {
    const next = applyChatBackgroundSessionOpacity(percent / 100);
    saveChatBackgroundSessionOpacity(next);
    setChatBackgroundSessionOpacity(next);
  }, []);

  const onChatBackgroundScope = useCallback((next: ChatBackgroundScope) => {
    applyChatBackgroundScope(next);
    saveChatBackgroundScope(next);
    setChatBackgroundScope(next);
  }, []);

  const onNewThreadBackgroundEffect = useCallback(
    (next: NewThreadBackgroundEffect) => {
      setNewThreadBackgroundEffect(next);
      setBackgroundEffect(next);
    },
    [],
  );

  const onUiScale = useCallback((percent: number) => {
    const next = saveUiScale(percent / 100);
    setUiScale(next);
    void applyUiScale(next);
  }, []);

  const onCollapsedProjectRailMode = useCallback(
    (next: CollapsedProjectRailMode) => {
      saveCollapsedProjectRailMode(next);
      setStoredCollapsedProjectRailMode(next);
      onControlledCollapsedProjectRailModeChange?.(next);
    },
    [onControlledCollapsedProjectRailModeChange],
  );

  const restoreDefaults = useCallback(() => {
    onThemePreference(THEME_PREFERENCE_DEFAULT);
    onAccentColor(ACCENT_COLOR_DEFAULT);
    onOpacity(Math.round(SIDEBAR_OPACITY_DEFAULT * 100));
    onBlur(SIDEBAR_BLUR_DEFAULT);
    onTint(THEME_HUE_DEFAULT, THEME_SATURATION_DEFAULT);
    onDarkLightness(THEME_DARK_LIGHTNESS_DEFAULT);
    onBodyGlass(BODY_GLASS_DEFAULT);
    onShowExcludedFiles(SHOW_EXCLUDED_FILES_DEFAULT);
    onChatBackgroundEmptyOpacity(
      Math.round(CHAT_BACKGROUND_EMPTY_OPACITY_DEFAULT * 100),
    );
    onChatBackgroundSessionOpacity(
      Math.round(CHAT_BACKGROUND_SESSION_OPACITY_DEFAULT * 100),
    );
    onChatBackgroundScope(CHAT_BACKGROUND_SCOPE_DEFAULT);
    onNewThreadBackgroundEffect(NEW_THREAD_BACKGROUND_EFFECT_DEFAULT);
    if (chatBackgroundPath) void onClearChatBackground();
    onUiScale(Math.round(UI_SCALE_DEFAULT * 100));
    onCollapsedProjectRailMode(COLLAPSED_PROJECT_RAIL_MODE_DEFAULT);
  }, [
    chatBackgroundPath,
    onBlur,
    onBodyGlass,
    onChatBackgroundEmptyOpacity,
    onChatBackgroundSessionOpacity,
    onChatBackgroundScope,
    onNewThreadBackgroundEffect,
    onClearChatBackground,
    onAccentColor,
    onShowExcludedFiles,
    onThemePreference,
    onOpacity,
    onTint,
    onDarkLightness,
    onUiScale,
    onCollapsedProjectRailMode,
  ]);

  return {
    themePreference,
    accentColor,
    opacity,
    blur,
    themeHue,
    themeSaturation,
    themeDarkLightness,
    bodyGlass,
    showExcludedFiles,
    chatBackgroundPath,
    chatBackgroundEmptyOpacity,
    chatBackgroundSessionOpacity,
    chatBackgroundScope,
    newThreadBackgroundEffect,
    chatBackgroundBusy,
    chatBackgroundError,
    uiScale,
    collapsedProjectRailMode,
    onThemePreference,
    onAccentColor,
    onOpacity,
    onBlur,
    onTint,
    onDarkLightness,
    onBodyGlass,
    onShowExcludedFiles,
    onChooseChatBackground,
    onClearChatBackground,
    onChatBackgroundEmptyOpacity,
    onChatBackgroundSessionOpacity,
    onChatBackgroundScope,
    onNewThreadBackgroundEffect,
    onUiScale,
    onCollapsedProjectRailMode,
    restoreDefaults,
  };
}

function AppearancePage({ appearance }: { appearance: AppearanceSettings }) {
  const percent = Math.round(appearance.opacity * 100);
  const glassDisabled = useColorScheme() === "light";

  return (
    <>
      <Group
        title="Theme"
        description="Dark and light share the same tint, so the color settings below apply to both."
      >
        <Row
          id="theme"
          label="Theme"
          description="System follows the OS appearance."
        >
          <Segmented
            label="Theme"
            value={appearance.themePreference}
            options={[
              { value: "system", label: "System" },
              { value: "dark", label: "Dark" },
              { value: "light", label: "Light" },
            ]}
            onChange={appearance.onThemePreference}
          />
        </Row>
        <Row
          id="accent-color"
          label="Accent color"
          description="Used for the composer send button and your message bubbles."
        >
          <AccentColorPicker
            value={appearance.accentColor}
            onChange={appearance.onAccentColor}
          />
        </Row>
      </Group>

      <Group
        title="Color"
        description="Hue and saturation tint every surface. Lightness only moves the dark theme."
      >
        <Row
          id="hue"
          label="Hue"
          description="Base hue for accents and tinted surfaces."
        >
          <Slider
            label="Hue"
            value={appearance.themeHue}
            display={`${appearance.themeHue}°`}
            min={THEME_HUE_MIN}
            max={THEME_HUE_MAX}
            onChange={(value) =>
              appearance.onTint(value, appearance.themeSaturation)
            }
          />
        </Row>
        <Row
          id="saturation"
          label="Saturation"
          description="How strongly the hue tints the interface. Zero keeps it neutral."
        >
          <Slider
            label="Saturation"
            value={appearance.themeSaturation}
            display={`${appearance.themeSaturation}%`}
            min={THEME_SATURATION_MIN}
            max={THEME_SATURATION_MAX}
            onChange={(value) => appearance.onTint(appearance.themeHue, value)}
          />
        </Row>
        <Row
          id="dark-lightness"
          label="Dark-mode lightness"
          description={
            glassDisabled
              ? "This only affects dark mode. Your dark-mode value is preserved."
              : "Base brightness of the dark theme. Lower values are darker; zero is true black."
          }
        >
          <Slider
            label="Dark-mode lightness"
            value={appearance.themeDarkLightness}
            display={`${appearance.themeDarkLightness}%`}
            min={THEME_DARK_LIGHTNESS_MIN}
            max={THEME_DARK_LIGHTNESS_MAX}
            onChange={appearance.onDarkLightness}
            disabled={glassDisabled}
          />
        </Row>
      </Group>

      <Group
        title="Translucency"
        description={
          glassDisabled
            ? "Light mode always uses an opaque window, so these are off. Your dark-mode values are preserved."
            : "How much of the desktop shows through MonoCode. Blur costs more to composite the higher it goes."
        }
      >
        <Row
          id="sidebar-opacity"
          label="Sidebar opacity"
          description="Applies to the project rail and the other glass panes."
        >
          <Slider
            label="Sidebar opacity"
            value={percent}
            display={`${percent}%`}
            min={Math.round(SIDEBAR_OPACITY_MIN * 100)}
            max={Math.round(SIDEBAR_OPACITY_MAX * 100)}
            onChange={appearance.onOpacity}
            disabled={glassDisabled}
          />
        </Row>
        <Row
          id="blur"
          label="Blur radius"
          description="Background blur behind the window."
        >
          <Slider
            label="Blur radius"
            value={appearance.blur}
            display={String(appearance.blur)}
            min={SIDEBAR_BLUR_MIN}
            max={SIDEBAR_BLUR_MAX}
            onChange={appearance.onBlur}
            disabled={glassDisabled}
          />
        </Row>
        <Row
          id="main-pane-glass"
          label="Main pane glass"
          description="Extend the translucent treatment to the main pane behind sessions and editors."
        >
          <Toggle
            label="Main pane glass"
            on={appearance.bodyGlass}
            onChange={appearance.onBodyGlass}
            disabled={glassDisabled}
          />
        </Row>
      </Group>

      <ChatBackgroundCard appearance={appearance} />

      <Group title="Layout">
        <Row
          id="collapsed-project-rail"
          label="Collapsed project rail"
          description="Keep project navigation available as a compact icon rail, or hide the rail completely."
        >
          <Segmented
            label="Collapsed project rail"
            value={appearance.collapsedProjectRailMode}
            options={[
              { value: "compact", label: "Icon rail" },
              { value: "hidden", label: "Hidden" },
            ]}
            onChange={appearance.onCollapsedProjectRailMode}
          />
        </Row>
        <Row
          id="interface-scale"
          label="Interface scale"
          description="Zoom the whole interface. You can also use Ctrl+=, Ctrl+-, and Ctrl+0 (Cmd on macOS)."
        >
          <Slider
            label="Interface scale"
            value={Math.round(appearance.uiScale * 100)}
            display={`${Math.round(appearance.uiScale * 100)}%`}
            min={Math.round(UI_SCALE_MIN * 100)}
            max={Math.round(UI_SCALE_MAX * 100)}
            step={10}
            onChange={appearance.onUiScale}
          />
        </Row>
        <Row
          id="show-excluded-files"
          label="Show excluded files"
          description="Show files and folders Git excludes, such as build output and dependencies, in the explorer."
        >
          <Toggle
            label="Show excluded files"
            on={appearance.showExcludedFiles}
            onChange={appearance.onShowExcludedFiles}
          />
        </Row>
      </Group>
    </>
  );
}

function ChatBackgroundCard({
  appearance,
}: {
  appearance: AppearanceSettings;
}) {
  const src = chatBackgroundSrc(appearance.chatBackgroundPath);
  const hasImage = Boolean(appearance.chatBackgroundPath && src);
  const emptyVisibility = Math.round(
    appearance.chatBackgroundEmptyOpacity * 100,
  );
  const sessionVisibility = Math.round(
    appearance.chatBackgroundSessionOpacity * 100,
  );
  const busy = appearance.chatBackgroundBusy;

  return (
    <Group
      id="chat-background"
      title="Chat background"
      description="An image behind your chat panes. It stays on this device."
    >
      <div className="border-b border-content/5 p-4 last:border-b-0">
        <div className="overflow-hidden rounded-lg border border-content/10">
          {hasImage ? (
            <div
              className={`relative h-36 ${appearance.newThreadBackgroundEffect === "gradient-blur" ? "bg-background-base" : ""}`}
            >
              {appearance.newThreadBackgroundEffect === "gradient-blur" ? (
                <GradientBlurBackground
                  className="gradient-blur-preview absolute inset-0"
                  style={{ opacity: appearance.chatBackgroundEmptyOpacity }}
                />
              ) : (
                <div
                  aria-hidden
                  className="size-full bg-cover bg-center bg-no-repeat"
                  style={{
                    backgroundImage: "var(--chat-background-image)",
                    opacity: appearance.chatBackgroundEmptyOpacity,
                  }}
                />
              )}
              <span className="pointer-events-none absolute bottom-2 left-2 text-[11px] text-content/40">
                Empty chat preview at {emptyVisibility}%
              </span>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => void appearance.onChooseChatBackground()}
              disabled={busy}
              className="flex h-36 w-full flex-col items-center justify-center gap-2 text-content/40 hover:bg-content/5 hover:text-content/70 disabled:cursor-default disabled:opacity-40"
            >
              {busy ? (
                <Loader className="size-5 animate-spin" aria-hidden />
              ) : (
                <ImagePlus className="size-5" aria-hidden />
              )}
              <span className="text-[12px]">Choose an image</span>
            </button>
          )}
        </div>
        {hasImage ? (
          <div className="mt-3 flex items-center justify-end gap-2">
            <SecondaryButton
              onClick={() => void appearance.onChooseChatBackground()}
              disabled={busy}
            >
              {busy ? (
                <Loader className="size-3.5 animate-spin" aria-hidden />
              ) : null}
              Change
            </SecondaryButton>
            <SecondaryButton
              onClick={() => void appearance.onClearChatBackground()}
              disabled={busy}
              danger
            >
              Remove
            </SecondaryButton>
          </div>
        ) : null}
        {appearance.chatBackgroundError ? (
          <p className="mt-2 text-[12px] text-red-400">
            {appearance.chatBackgroundError}
          </p>
        ) : null}
      </div>
      {hasImage ? (
        <>
          <Row
            label="Background effect"
            description={
              NEW_THREAD_BACKGROUND_EFFECT_DESCRIPTIONS[
                appearance.newThreadBackgroundEffect
              ]
            }
          >
            <Segmented
              label="Background effect"
              value={appearance.newThreadBackgroundEffect}
              options={NEW_THREAD_BACKGROUND_EFFECTS.map((effect) => ({
                value: effect,
                label: NEW_THREAD_BACKGROUND_EFFECT_LABELS[effect],
              }))}
              onChange={appearance.onNewThreadBackgroundEffect}
              optionIdPrefix="new-thread-background-effect"
            />
          </Row>
          <Row
            label="Show on"
            description="Empty sessions only, or every conversation."
          >
            <Segmented
              label="Show background on"
              value={appearance.chatBackgroundScope}
              options={[
                { value: "empty", label: "Empty only" },
                { value: "all", label: "All sessions" },
              ]}
              onChange={appearance.onChatBackgroundScope}
            />
          </Row>
          <Row
            label="Empty chat visibility"
            description="Background strength before a chat has messages."
          >
            <Slider
              label="Empty chat background visibility"
              value={emptyVisibility}
              display={`${emptyVisibility}%`}
              min={Math.round(CHAT_BACKGROUND_OPACITY_MIN * 100)}
              max={Math.round(CHAT_BACKGROUND_OPACITY_MAX * 100)}
              onChange={appearance.onChatBackgroundEmptyOpacity}
            />
          </Row>
          <Row
            label="Session visibility"
            description="Background strength once the conversation has messages."
          >
            <Slider
              label="Session background visibility"
              value={sessionVisibility}
              display={`${sessionVisibility}%`}
              min={Math.round(CHAT_BACKGROUND_OPACITY_MIN * 100)}
              max={Math.round(CHAT_BACKGROUND_OPACITY_MAX * 100)}
              onChange={appearance.onChatBackgroundSessionOpacity}
            />
          </Row>
        </>
      ) : null}
    </Group>
  );
}

function KeybindingsPage() {
  const [query, setQuery] = useState("");
  const rows = useMemo(() => filterKeybindings(KEYBINDINGS, query), [query]);

  return (
    <Group
      title="Shortcuts"
      description="Bindings come from the app menu and the workspace key handler; they aren’t customizable yet."
      action={
        <div className="flex items-center gap-3">
          <span className="shrink-0 text-[12px] text-content/40 tabular-nums">
            {rows.length} {rows.length === 1 ? "binding" : "bindings"}
          </span>
          <label className="flex h-7 w-44 shrink-0 items-center gap-2 rounded-md border border-content/10 px-2 text-content/45 focus-within:border-content/20">
            <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Filter"
              aria-label="Filter keybindings"
              spellCheck={false}
              autoComplete="off"
              className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
            />
          </label>
        </div>
      }
    >
      <div className="flex items-center border-b border-stroke bg-content/5 px-4 py-2 text-[11px] font-semibold uppercase tracking-[0.08em] text-content/40">
        <span className="min-w-0 flex-1">Command</span>
        <span className="w-40 shrink-0">Keybinding</span>
        <span className="w-28 shrink-0">When</span>
      </div>
      {rows.length === 0 ? (
        <p className="px-4 py-3 text-[12px] text-content/45">
          No matching bindings
        </p>
      ) : (
        rows.map((row) => (
          <div
            key={`${row.command}-${row.keys}`}
            className="flex items-center border-b border-content/5 px-4 py-2 text-[12px] last:border-b-0"
          >
            <span className="min-w-0 flex-1 truncate">{row.command}</span>
            <span className="w-40 shrink-0 font-mono text-[12px] text-content/80">
              {row.keys}
            </span>
            <span className="w-28 shrink-0 font-mono text-[11px] text-content/40">
              {row.when}
            </span>
          </div>
        ))
      )}
    </Group>
  );
}

const GLOBAL_PROVIDER_SCOPE = "global";

function ProvidersPage({
  cwd,
  recents,
}: {
  cwd?: string;
  recents?: RecentProject[];
}) {
  useSyncExternalStore(subscribeModels, getModelSnapshot, getModelSnapshot);
  useSyncExternalStore(
    subscribeHarnessAvailability,
    getHarnessAvailabilitySnapshot,
    getHarnessAvailabilitySnapshot,
  );
  const providersRevision = useSyncExternalStore(
    subscribeProjectProviders,
    projectProvidersRevision,
    projectProvidersRevision,
  );
  void providersRevision;
  const [choice, setChoice] = useState(loadLastModelChoice);
  const [defaultModels, setDefaultModels] = useState(loadDefaultModels);
  const [claudeHooks, setClaudeHooks] = useState(loadClaudeHooks);
  const [scope, setScope] = useState<string>(GLOBAL_PROVIDER_SCOPE);
  const [hiddenGlobally, setHiddenGlobally] = useState(loadHiddenPickerProviders);

  const scopeOptions = useMemo(() => {
    const options: { value: string; label: string; icon?: ReactNode }[] = [
      {
        value: GLOBAL_PROVIDER_SCOPE,
        label: "Global",
        icon: (
          <Globe
            className="size-3.5 shrink-0 text-content/60"
            strokeWidth={1.75}
          />
        ),
      },
    ];
    const seen = new Set<string>();
    for (const path of [cwd, ...(recents ?? []).map((entry) => entry.path)]) {
      if (!path || !looksLikeProject(path)) continue;
      const key = pathKey(path);
      if (seen.has(key)) continue;
      seen.add(key);
      options.push({
        value: path,
        label: projectName(path),
        icon: <ProjectScopeIcon path={path} />,
      });
    }
    return options;
  }, [cwd, recents]);

  const project = scope === GLOBAL_PROVIDER_SCOPE ? null : scope;
  const projectSettings = project ? loadProjectProviderSettings(project) : {};
  // A project without overrides inherits the global default provider, the same
  // way `defaultSessionChoice` resolves it for new conversations.
  const effectiveDefaultHarness = project
    ? firstEnabledHarness(
        project,
        projectSettings.defaultHarness ?? choice?.harness ?? "cursor",
      )
    : (choice?.harness ?? null);

  useEffect(() => {
    void probeHarnessAvailability();
  }, []);

  useEffect(() => {
    if (!scopeOptions.some((option) => option.value === scope)) {
      setScope(GLOBAL_PROVIDER_SCOPE);
    }
  }, [scope, scopeOptions]);

  const onClaudeHooks = (next: boolean) => {
    saveClaudeHooks(next);
    setClaudeHooks(next);
  };

  const onModelChange = (harness: HarnessId, model: string) => {
    if (project) {
      setProjectDefaultModel(project, harness, model);
      return;
    }
    saveDefaultModel(harness, model);
    setDefaultModels((prev) => ({ ...prev, [harness]: model }));
    if (choice?.harness === harness) {
      saveLastModelChoice(harness, model);
      setChoice({ harness, model });
    }
  };

  const onDefault = (harness: HarnessId, model: string) => {
    if (project) {
      setProjectDefaultProvider(project, harness, model);
      return;
    }
    saveLastModelChoice(harness, model);
    setDefaultModels((prev) => ({ ...prev, [harness]: model }));
    setChoice({ harness, model });
  };

  const onPickerVisible = (harness: HarnessId, visible: boolean) => {
    if (project) {
      setProjectProviderHidden(project, harness, !visible);
      return;
    }
    savePickerProviderVisible(harness, visible);
    setHiddenGlobally((prev) =>
      visible
        ? prev.filter((id) => id !== harness)
        : [...new Set([...prev, harness])],
    );
  };

  return (
    <>
      <ProviderAccountsSettings />

      <Group
        id="provider-runtime"
        title="Agent CLIs"
        action={
          <Select
            label="Provider defaults scope"
            value={scope}
            options={scopeOptions}
            onChange={setScope}
          />
        }
        description={
          project
            ? `These defaults apply to ${projectName(project)} only. A provider with Show in picker off is also kept out of new conversations started in this project.`
            : "A provider is listed as installed once its CLI is found on your PATH. Uninstalled CLIs stay listed but are left out of the model picker, as are installed ones with Show in picker off. The model beside a provider is what its new conversations start with; Use by default picks the provider itself. Open Runtime on any provider to override its binary, launch flags, and environment."
        }
      >
        {HARNESSES.map((harness) => {
          const inPicker = project
            ? !(projectSettings.hidden ?? []).includes(harness) &&
              !hiddenGlobally.includes(harness)
            : !hiddenGlobally.includes(harness);
          // A globally hidden provider stays out of every project's picker, so
          // the project toggle is shown locked rather than appearing to work.
          const pickerLocked =
            project != null && hiddenGlobally.includes(harness);
          const selectedModel = project
            ? (projectSettings.models?.[harness] ??
              (projectSettings.defaultHarness === harness
                ? projectSettings.defaultModel
                : undefined) ??
              defaultModels[harness] ??
              (choice?.harness === harness
                ? choice.model
                : defaultModelId(harness)))
            : (defaultModels[harness] ??
              (choice?.harness === harness
                ? choice.model
                : defaultModelId(harness)));
          const isDefault = project
            ? effectiveDefaultHarness === harness
            : choice?.harness === harness;
          return (
            <ProviderRow
              key={harness}
              harness={harness}
              selectedModel={selectedModel}
              isDefault={isDefault}
              inPicker={inPicker}
              pickerLocked={pickerLocked}
              onDefault={onDefault}
              onModelChange={onModelChange}
              onPickerVisible={(visible) => onPickerVisible(harness, visible)}
            />
          );
        })}
      </Group>

      <Group title="Advanced">
        <Row
          id="claude-hooks"
          label="Claude Code hooks"
          description="Run the hooks configured in your settings.json files — PreToolUse command rewrites, blocks, notifications, and the rest — just as the Claude Code CLI would. Turn this off if a hook is misbehaving and you need the session back. Takes effect on the next turn."
        >
          <Toggle
            label="Claude Code hooks"
            on={claudeHooks}
            onChange={onClaudeHooks}
          />
        </Row>
      </Group>
    </>
  );
}

type AccountEditor = {
  provider: ProviderAccountProvider;
  accountId?: string;
  label: string;
};

function ProviderAccountsSettings() {
  const [, setVersion] = useState(0);
  const [editor, setEditor] = useState<AccountEditor | null>(null);
  const [working, setWorking] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(
    () => subscribeProviderAccounts(() => setVersion((value) => value + 1)),
    [],
  );

  const startAdd = (provider: ProviderAccountProvider) => {
    setError(null);
    setEditor({ provider, label: "" });
  };

  const startRename = (account: ProviderAccount) => {
    setError(null);
    setEditor({
      provider: account.provider,
      accountId: account.id,
      label: account.label,
    });
  };

  const submitEditor = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!editor || !editor.label.trim() || working) return;
    const key = editor.accountId
      ? `rename:${editor.provider}:${editor.accountId}`
      : `add:${editor.provider}`;
    setWorking(key);
    setError(null);
    try {
      if (editor.accountId) {
        renameProviderAccount(editor.provider, editor.accountId, editor.label);
      } else {
        const account = newProviderAccount(editor.provider, editor.label);
        await loginHarness(editor.provider, account.id);
        saveProviderAccount(account);
      }
      setEditor(null);
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : "Could not save this account",
      );
    } finally {
      setWorking(null);
    }
  };

  const removeAccount = async (account: ProviderAccount) => {
    if (account.isDefault || working) return;
    const confirmed = await ask(
      `Remove “${account.label}”? Its stored credentials will be deleted and any running turns for this account will stop. Existing conversations stay in history, but cannot continue until you switch accounts.`,
      {
        title: `Remove ${HARNESS_TITLE[account.provider]} account`,
        kind: "warning",
        okLabel: "Remove account",
        cancelLabel: "Cancel",
      },
    );
    if (!confirmed) return;
    const key = `remove:${account.provider}:${account.id}`;
    setWorking(key);
    setError(null);
    try {
      await removeProviderAccountCredentials(account.provider, account.id);
      removeProviderAccount(account.provider, account.id);
      if (
        editor?.provider === account.provider &&
        editor.accountId === account.id
      ) {
        setEditor(null);
      }
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : "Could not remove this account",
      );
    } finally {
      setWorking(null);
    }
  };

  return (
    <Group
      id="provider-accounts"
      title="Accounts"
      description="Create isolated sign-ins for providers that support account profiles. Account switching stays available from the usage control in the footer."
    >
      {PROVIDER_ACCOUNT_PROVIDERS.map((provider) => {
        const accounts = providerAccounts(provider);
        const adding = editor?.provider === provider && !editor.accountId;
        return (
          <div
            key={provider}
            className="border-b border-content/5 last:border-b-0"
          >
            <div className="flex items-center gap-4 px-4 py-3.5">
              <div className="flex min-w-0 flex-1 items-center gap-2.5">
                <span className="grid size-7 shrink-0 place-items-center rounded-lg bg-content/[0.05] ring-1 ring-inset ring-content/[0.06]">
                  <HarnessIcon harness={provider} className="size-4" />
                </span>
                <div className="min-w-0">
                  <div className="text-[13px] font-medium text-content">
                    {HARNESS_TITLE[provider]}
                  </div>
                  <div className="mt-0.5 text-[11px] text-content/40">
                    {accounts.length}{" "}
                    {accounts.length === 1 ? "account" : "accounts"}
                  </div>
                </div>
              </div>
              <button
                type="button"
                disabled={Boolean(working)}
                onClick={() => startAdd(provider)}
                className="flex shrink-0 items-center gap-1.5 rounded-md border border-content/10 px-2.5 py-1 text-[12px] text-content/70 transition-transform duration-150 hover:bg-content/10 hover:text-content active:scale-[0.97] disabled:cursor-default disabled:opacity-40"
              >
                <Plus className="size-3.5" strokeWidth={1.75} aria-hidden />
                Add account
              </button>
            </div>
            <div className="border-t border-content/5 bg-content/[0.015] pl-10">
              {accounts.map((account) => {
                const editing =
                  editor?.provider === provider &&
                  editor.accountId === account.id;
                const removing = working === `remove:${provider}:${account.id}`;
                return editing ? (
                  <ProviderAccountEditor
                    key={account.id}
                    editor={editor}
                    working={Boolean(working)}
                    onLabel={(label) =>
                      setEditor((current) =>
                        current ? { ...current, label } : current,
                      )
                    }
                    onCancel={() => setEditor(null)}
                    onSubmit={submitEditor}
                  />
                ) : (
                  <div
                    key={account.id}
                    className="flex h-12 items-center gap-3 border-b border-content/5 px-4 py-2 last:border-b-0"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[12px] text-content/85">
                        {account.label}
                      </div>
                      <div className="mt-0.5 text-[10px] text-content/35">
                        {account.isDefault
                          ? "Provider CLI profile"
                          : "Isolated profile"}
                      </div>
                    </div>
                    <div className="flex shrink-0 items-center gap-1">
                      {account.isDefault ? (
                        <span className="mr-1 text-[10px] font-medium uppercase tracking-wide text-content/30">
                          Default
                        </span>
                      ) : null}
                      <button
                        type="button"
                        disabled={Boolean(working)}
                        aria-label={`Rename ${account.label}`}
                        title="Rename account"
                        onClick={() => startRename(account)}
                        className="grid size-7 place-items-center rounded-md text-content/40 transition-transform duration-150 hover:bg-content/10 hover:text-content active:scale-[0.96] disabled:opacity-35"
                      >
                        <Pencil className="size-3.5" strokeWidth={1.75} />
                      </button>
                      {!account.isDefault ? (
                        <button
                          type="button"
                          disabled={Boolean(working)}
                          aria-label={`Remove ${account.label}`}
                          title="Remove account"
                          onClick={() => void removeAccount(account)}
                          className="grid size-7 place-items-center rounded-md text-content/35 transition-transform duration-150 hover:bg-red-400/10 hover:text-red-400 active:scale-[0.96] disabled:opacity-35"
                        >
                          {removing ? (
                            <Loader className="size-3.5 animate-spin" />
                          ) : (
                            <Trash2 className="size-3.5" strokeWidth={1.75} />
                          )}
                        </button>
                      ) : null}
                    </div>
                  </div>
                );
              })}
              {adding && editor ? (
                <ProviderAccountEditor
                  editor={editor}
                  working={Boolean(working)}
                  onLabel={(label) =>
                    setEditor((current) =>
                      current ? { ...current, label } : current,
                    )
                  }
                  onCancel={() => setEditor(null)}
                  onSubmit={submitEditor}
                />
              ) : null}
            </div>
          </div>
        );
      })}
      {error ? (
        <p
          className="border-t border-content/5 px-4 py-2.5 text-[11px] leading-4 text-red-400"
          role="alert"
        >
          {error}
        </p>
      ) : null}
    </Group>
  );
}

function ProviderAccountEditor({
  editor,
  working,
  onLabel,
  onCancel,
  onSubmit,
}: {
  editor: AccountEditor;
  working: boolean;
  onLabel: (label: string) => void;
  onCancel: () => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
}) {
  const adding = !editor.accountId;
  return (
    <form
      className="flex h-12 items-center border-b border-content/5 px-4 py-2 last:border-b-0"
      onSubmit={onSubmit}
    >
      <div
        data-provider-account-editor-field
        className="flex items-center pr-1 h-8 min-w-0 flex-1 overflow-hidden rounded-md border border-content/10 bg-content/[0.04] focus-within:border-accent/45"
      >
        <label className="h-full min-w-0 flex-1">
          <span className="sr-only">Account name</span>
          <input
            autoFocus
            type="text"
            maxLength={48}
            value={editor.label}
            disabled={working}
            placeholder="Work or Personal"
            aria-label={`${adding ? "New" : "Rename"} ${HARNESS_TITLE[editor.provider]} account`}
            onChange={(event) => onLabel(event.target.value)}
            className="h-full w-full bg-transparent px-2.5 text-[12px] text-content outline-none placeholder:text-content/25 disabled:opacity-50"
          />
        </label>
        <button
          type="button"
          disabled={working}
          onClick={onCancel}
          className="flex h-6 shrink-0 items-center rounded-[4.5px] bg-content/[0.05] px-2.5 text-[11px] text-content/45 transition-transform duration-150 hover:bg-content/10 hover:text-content active:scale-[0.97] disabled:opacity-40"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={working || !editor.label.trim()}
          className="ml-1 flex h-6 shrink-0 items-center gap-1.5 rounded-[4.5px] bg-content px-2.5 text-[11px] font-medium text-background-base transition-transform duration-150 hover:bg-content/85 active:scale-[0.97] disabled:cursor-default disabled:opacity-40"
        >
          {working ? <Loader className="size-3 animate-spin" /> : null}
          {adding
            ? working
              ? "Waiting for browser…"
              : "Sign in and add"
            : "Save"}
        </button>
      </div>
    </form>
  );
}

function RuntimeTextInput({
  value,
  placeholder,
  onChange,
  ariaLabel,
  className = "w-56",
}: {
  value: string;
  placeholder?: string;
  onChange: (value: string) => void;
  ariaLabel: string;
  className?: string;
}) {
  return (
    <label
      className={`flex h-7 max-w-full shrink-0 items-center rounded-md border border-content/10 px-2 focus-within:border-content/20 ${className}`}
    >
      <input
        type="text"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        aria-label={ariaLabel}
        autoComplete="off"
        spellCheck={false}
        className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
      />
    </label>
  );
}

/** The binary name each resolver in src-tauri/src/harness.rs looks for on
 * PATH, shown as the Binary path placeholder when no override is set. */
const DEFAULT_HARNESS_BINARY: Record<HarnessId, string> = {
  claude: "claude",
  codex: "codex",
  cursor: "cursor-agent",
  grok: "grok",
  opencode: "opencode",
  pi: "pi-coding-agent",
  omp: "omp",
  fx: "fx",
  hermes: "hermes",
  antigravity: "agy_acp_server.par",
};

/** The icon the project rail shows: custom logo, else the project mascot. */
function ProjectScopeIcon({ path }: { path: string }) {
  const logos = useTabGroupLogos();
  const [colors] = useState(loadTabGroupColors);
  const [customColors] = useState(loadTabGroupCustomColors);
  const [mascots] = useState(loadTabGroupMascots);
  const key = projectKey(path);
  const name = projectName(path);
  const logoPath = resolveTabGroupLogo(key, logos);
  if (logoPath) {
    return (
      <ProjectLogoIcon
        path={logoPath}
        className="size-4 rounded-sm"
        imageClassName="size-4"
      />
    );
  }
  return (
    <ProjectMascot
      project={name}
      color={resolveTabGroupColor(key, colors, customColors, name)}
      name={resolveTabGroupMascot(key, mascots)}
      className="size-3.5"
    />
  );
}

function ProviderRow({
  harness,
  selectedModel,
  isDefault,
  inPicker,
  pickerLocked = false,
  onDefault,
  onModelChange,
  onPickerVisible,
}: {
  harness: HarnessId;
  selectedModel: string;
  isDefault: boolean;
  inPicker: boolean;
  /** Globally hidden providers cannot be turned on per project. */
  pickerLocked?: boolean;
  onDefault: (harness: HarnessId, model: string) => void;
  onModelChange: (harness: HarnessId, model: string) => void;
  onPickerVisible: (visible: boolean) => void;
}) {
  const models = modelsFor(harness);
  const available = isHarnessAvailable(harness);
  const current =
    models.length > 0 ? resolveModel(harness, selectedModel) : null;
  const [runtimeOpen, setRuntimeOpen] = useState(false);

  useEffect(() => {
    if (!available || models.length > 0) return;
    void refreshHarnessCatalogs([harness]);
  }, [available, harness, models.length]);

  return (
    <>
      <Row
        label={
          <span className="flex items-center gap-2">
            <HarnessIcon harness={harness} className="size-4 shrink-0" />
            {HARNESS_TITLE[harness]}
            {isDefault ? (
              <span className="rounded-full bg-content/10 px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-content/60">
                Default
              </span>
            ) : null}
          </span>
        }
        description={
          available
            ? `${models.length} ${models.length === 1 ? "model" : "models"} available.`
            : harnessUnavailableHint(harness)
        }
      >
        {current ? (
          <Select
            label={`${HARNESS_TITLE[harness]} model`}
            value={current.id}
            onChange={(next) => onModelChange(harness, next)}
            options={models.map((item) => ({
              value: item.id,
              label: item.name,
            }))}
          />
        ) : null}
        <SecondaryButton
          onClick={() => current && onDefault(harness, current.id)}
          disabled={isDefault || !current}
        >
          {isDefault ? "Default" : "Use by default"}
        </SecondaryButton>
        {available ? (
          <div className="flex items-center gap-2">
            <span className="text-[12px] text-content/50">
              {pickerLocked ? "Hidden globally" : "Show in picker"}
            </span>
            <Toggle
              label={`Show ${HARNESS_TITLE[harness]} in the model picker`}
              on={inPicker}
              onChange={onPickerVisible}
              disabled={pickerLocked}
            />
          </div>
        ) : null}
        <button
          type="button"
          aria-expanded={runtimeOpen}
          aria-label={`${HARNESS_TITLE[harness]} runtime settings`}
          onClick={() => setRuntimeOpen((prev) => !prev)}
          className={`flex h-7 shrink-0 items-center gap-1.5 rounded-md border px-2 text-[12px] transition-colors ${
            runtimeOpen
              ? "border-content/20 text-content"
              : "border-content/10 text-content/50 hover:border-content/20 hover:text-content"
          }`}
        >
          <SlidersHorizontal className="size-3.5" strokeWidth={1.75} />
          Runtime
          <ChevronDown
            className={`size-3 transition-transform ${runtimeOpen ? "rotate-180" : ""}`}
            strokeWidth={2}
          />
        </button>
      </Row>
      {runtimeOpen ? <ProviderRuntimePanel harness={harness} /> : null}
    </>
  );
}

/** Binary path, launch arguments, and env vars apply the same way to every
 * harness (see `harness_spawn` in src-tauri, which is generic over these).
 * Claude additionally gets a CLAUDE_CONFIG_DIR convenience, which doesn't
 * generalize to the other CLIs. */
function ProviderRuntimePanel({ harness }: { harness: HarnessId }) {
  const [runtime, setRuntime] = useState<HarnessRuntimeSettings>(() =>
    loadHarnessRuntime(harness),
  );
  const [configDir, setConfigDir] = useState<string>(loadClaudeConfigDir);

  const update = (patch: Partial<HarnessRuntimeSettings>) => {
    setRuntime((prev) => {
      const next = { ...prev, ...patch };
      saveHarnessRuntime(harness, next);
      return next;
    });
  };

  const onConfigDir = (value: string) => {
    saveClaudeConfigDir(value);
    setConfigDir(value);
  };

  const addEnvVar = () => {
    update({ env: [...runtime.env, { key: "", value: "" }] });
  };

  const updateEnvVar = (
    index: number,
    patch: Partial<HarnessRuntimeEnvVar>,
  ) => {
    update({
      env: runtime.env.map((item, i) =>
        i === index ? { ...item, ...patch } : item,
      ),
    });
  };

  const removeEnvVar = (index: number) => {
    update({ env: runtime.env.filter((_, i) => i !== index) });
  };

  return (
    <div className="flex flex-col border-b border-content/5 bg-content/[0.03] last:border-b-0">
      <Row
        label="Binary path"
        description={`Path to the ${HARNESS_TITLE[harness]} binary used by this instance. Leave blank to use the CLI found on your PATH.`}
      >
        <RuntimeTextInput
          value={runtime.binaryPath}
          placeholder={DEFAULT_HARNESS_BINARY[harness]}
          onChange={(value) => update({ binaryPath: value })}
          ariaLabel={`${HARNESS_TITLE[harness]} binary path`}
        />
      </Row>
      {harness === "claude" ? (
        <Row
          label="CLAUDE_CONFIG_DIR path"
          description="Custom Claude home and config directory. Overrides any provider account profile, and takes precedence over a CLAUDE_CONFIG_DIR set below in Environment variables. Leave blank to use ~/.claude."
        >
          <RuntimeTextInput
            value={configDir}
            placeholder="~/.claude"
            onChange={onConfigDir}
            ariaLabel="CLAUDE_CONFIG_DIR path"
          />
        </Row>
      ) : null}
      <Row
        label="Launch arguments"
        description="Additional CLI arguments passed on session start."
      >
        <RuntimeTextInput
          value={runtime.launchArgs}
          placeholder="e.g. --chrome"
          onChange={(value) => update({ launchArgs: value })}
          ariaLabel={`${HARNESS_TITLE[harness]} launch arguments`}
        />
      </Row>
      <Row
        label="Environment variables"
        description="API keys, base URLs, and other per-instance CLI settings."
      >
        <SecondaryButton onClick={addEnvVar}>
          <Plus className="size-3.5" strokeWidth={1.75} />
          Add variable
        </SecondaryButton>
      </Row>
      {runtime.env.length > 0 ? (
        <div className="flex flex-col gap-2 px-4 pb-3.5">
          {runtime.env.map((entry, index) => (
            <div key={index} className="flex items-center gap-2">
              <RuntimeTextInput
                value={entry.key}
                placeholder="KEY"
                onChange={(value) => updateEnvVar(index, { key: value })}
                ariaLabel={`Environment variable ${index + 1} key`}
                className="w-40"
              />
              <RuntimeTextInput
                value={entry.value}
                placeholder="value"
                onChange={(value) => updateEnvVar(index, { value })}
                ariaLabel={`Environment variable ${index + 1} value`}
                className="flex-1"
              />
              <button
                type="button"
                aria-label="Remove environment variable"
                title="Remove variable"
                onClick={() => removeEnvVar(index)}
                className="grid size-7 shrink-0 place-items-center rounded-md text-content/35 transition-transform duration-150 hover:bg-red-400/10 hover:text-red-400 active:scale-[0.96]"
              >
                <Trash2 className="size-3.5" strokeWidth={1.75} />
              </button>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function useArchivedProjects(): ArchivedProject[] {
  const [items, setItems] = useState(loadArchivedProjects);
  useEffect(
    () => subscribeArchivedProjects(() => setItems(loadArchivedProjects())),
    [],
  );
  return items;
}

function archivedProjectLabel(path: string): string {
  return resolveTabGroupLabel(
    projectKey(path),
    loadTabGroupLabels(),
    projectName(path),
  );
}

function ArchivePage({
  cwd,
  sessions,
  onOpenSession,
  onArchiveSession,
  onDeleteSession,
  onRestoreProject,
  onDeleteProject,
}: {
  cwd: string;
  sessions: SessionSummary[];
  onOpenSession: (sessionId: string) => void;
  onArchiveSession: (sessionId: string, archived: boolean) => void;
  onDeleteSession: (sessionId: string) => void;
  onRestoreProject?: (path: string) => void;
  onDeleteProject?: (path: string) => void;
}) {
  const [filters, setFilters] = useState(loadSessionSidebarFilters);
  const [deleting, setDeleting] = useState<ArchivedProject | null>(null);
  const archivedProjects = useArchivedProjects();
  const archived = useMemo(
    () =>
      sessions
        .filter((session) => session.archived)
        .sort((a, b) => b.updatedAt - a.updatedAt),
    [sessions],
  );

  const onShowArchived = (showArchived: boolean) => {
    const next = { ...filters, showArchived };
    saveSessionSidebarFilters(next);
    setFilters(next);
  };

  return (
    <>
      <Group
        title="Archived projects"
        description="Archive a project from the rail to keep its chats without listing it in the sidebar."
      >
        {archivedProjects.length === 0 ? (
          <p className="px-4 py-3.5 text-[12px] text-content/45">
            No archived projects.
          </p>
        ) : (
          archivedProjects.map((project) => (
            <div
              key={project.path}
              className="flex items-center gap-3 border-b border-content/5 px-4 py-2.5 last:border-b-0"
            >
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px]">
                  {archivedProjectLabel(project.path)}
                </div>
                <div className="truncate text-[11px] text-content/40">
                  {prettyCwd(project.path)}
                </div>
              </div>
              {onRestoreProject ? (
                <SecondaryButton onClick={() => onRestoreProject(project.path)}>
                  Restore
                </SecondaryButton>
              ) : null}
              {onDeleteProject ? (
                <SecondaryButton danger onClick={() => setDeleting(project)}>
                  Delete
                </SecondaryButton>
              ) : null}
            </div>
          ))
        )}
      </Group>

      <Group
        title={
          looksLikeProject(cwd)
            ? `Archived in ${projectName(cwd)}`
            : "Archived conversations"
        }
      >
        <Row
          id="show-archived"
          label="Show archived in the sidebar"
          description="Keep archived conversations listed alongside the active ones."
        >
          <Toggle
            label="Show archived in the sidebar"
            on={filters.showArchived}
            onChange={onShowArchived}
          />
        </Row>
        {!looksLikeProject(cwd) ? (
          <p className="px-4 py-3.5 text-[12px] text-content/45">
            Open a project to see its archived conversations.
          </p>
        ) : archived.length === 0 ? (
          <p className="px-4 py-3.5 text-[12px] text-content/45">
            No archived conversations in this project.
          </p>
        ) : (
          archived.map((session) => (
            <div
              key={session.id}
              className="flex items-center gap-3 border-b border-content/5 px-4 py-2.5 last:border-b-0"
            >
              <HarnessIcon
                harness={session.harness}
                className="size-3.5 shrink-0"
              />
              <button
                type="button"
                onClick={() => onOpenSession(session.id)}
                className="min-w-0 flex-1 truncate text-left text-[13px] hover:text-content"
              >
                {sessionDisplayTitle(session.title, session.harness)}
              </button>
              <span className="shrink-0 text-[11px] text-content/35 tabular-nums">
                {formatDate(session.updatedAt)}
              </span>
              <SecondaryButton
                onClick={() => onArchiveSession(session.id, false)}
              >
                Unarchive
              </SecondaryButton>
              <SecondaryButton
                danger
                onClick={() => onDeleteSession(session.id)}
              >
                Delete
              </SecondaryButton>
            </div>
          ))
        )}
      </Group>

      {deleting ? (
        <RemoveProjectDialog
          name={archivedProjectLabel(deleting.path)}
          path={deleting.path}
          onCancel={() => setDeleting(null)}
          onConfirm={() => {
            onDeleteProject?.(deleting.path);
            setDeleting(null);
          }}
        />
      ) : null}
    </>
  );
}

function formatDate(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "";
  try {
    return new Intl.DateTimeFormat(undefined, {
      month: "short",
      day: "numeric",
    }).format(new Date(value));
  } catch {
    return "";
  }
}

function PageHeader({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <header className="pb-4">
      <h1 className="text-[20px] font-semibold leading-tight text-content">
        {title}
      </h1>
      {description ? (
        <p className="mt-1.5 max-w-xl text-[13px] leading-relaxed text-content/45">
          {description}
        </p>
      ) : null}
    </header>
  );
}

/**
 * A titled card of related settings. Everything on a page lives in one, so a
 * page reads as a handful of topics instead of one long list of switches.
 */
function Group({
  id,
  title,
  description,
  action,
  children,
}: {
  /** Matches a `SETTINGS_INDEX` id when the whole card is the search target. */
  id?: string;
  title: ReactNode;
  description?: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  const revealed = useContext(RevealedSetting);
  const flash = id != null && revealed === id;

  return (
    <section
      id={id ? settingDomId(id) : undefined}
      data-setting-id={id}
      className="pt-8 first:pt-0"
    >
      <div className="flex items-end gap-4 pb-2.5">
        <div className="min-w-0 flex-1">
          <h2 className="text-[13px] font-semibold text-content">{title}</h2>
          {description ? (
            <p className="mt-1 text-[12px] leading-relaxed text-content/45">
              {description}
            </p>
          ) : null}
        </div>
        {action ? <div className="shrink-0 pb-0.5">{action}</div> : null}
      </div>
      <div
        className={`overflow-hidden rounded-xl border bg-content/3 transition-colors ${
          flash ? "border-accent/60" : "border-content/10"
        }`}
      >
        {children}
      </div>
    </section>
  );
}

function Row({
  id,
  label,
  description,
  children,
}: {
  /** Matches a `SETTINGS_INDEX` id so search can scroll here. */
  id?: string;
  label: ReactNode;
  description?: string;
  children?: ReactNode;
}) {
  const revealed = useContext(RevealedSetting);
  const flash = id != null && revealed === id;

  return (
    <div
      id={id ? settingDomId(id) : undefined}
      data-setting-id={id}
      className={`settings-row flex items-start gap-6 border-b border-content/5 px-4 py-3.5 transition-colors last:border-b-0 ${
        flash ? "bg-accent/10" : ""
      }`}
    >
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium text-content">{label}</div>
        {description ? (
          <p className="mt-1 text-[12px] leading-relaxed text-content/45">
            {description}
          </p>
        ) : null}
      </div>
      <div className="settings-row-control flex min-w-0 max-w-[60%] shrink-0 flex-wrap items-center justify-end gap-2">
        {children}
      </div>
    </div>
  );
}

function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
  optionIdPrefix,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  optionIdPrefix?: string;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="inline-grid max-w-full shrink-0 gap-0.5 rounded-md border border-content/10 p-0.5 text-[12px]"
      style={{
        gridTemplateColumns: `repeat(${options.length}, minmax(0, 1fr))`,
      }}
    >
      {options.map((option) => (
        <button
          id={
            optionIdPrefix
              ? `${optionIdPrefix}-${option.value.toLowerCase()}`
              : undefined
          }
          key={option.value}
          type="button"
          role="radio"
          aria-checked={value === option.value}
          onClick={() => onChange(option.value)}
          className={`min-w-0 rounded-[5px] px-2.5 py-1 ${
            value === option.value
              ? "bg-selection text-content"
              : "text-content/50 hover:text-content"
          }`}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function Slider({
  label,
  value,
  display,
  min,
  max,
  step = 1,
  onChange,
  disabled = false,
}: {
  label: string;
  value: number;
  display: string;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  disabled?: boolean;
}) {
  return (
    <div
      className={`flex w-56 max-w-full items-center gap-3 ${disabled ? "opacity-40" : ""}`}
    >
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={value}
        aria-label={label}
        disabled={disabled}
        className="sidebar-opacity-slider min-w-0 flex-1 disabled:cursor-not-allowed"
        onChange={(event) => onChange(Number(event.target.value))}
      />
      <span className="w-10 shrink-0 text-right text-[12px] text-content tabular-nums">
        {display}
      </span>
    </div>
  );
}

const ACCENT_COLOR_PRESETS = [
  "#4da3f5",
  "#8b5cf6",
  "#ec4899",
  "#ef4444",
  "#f59e0b",
  "#10b981",
] as const;

function AccentColorPicker({
  value,
  onChange,
}: {
  value: string | null;
  onChange: (value: string | null) => void;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const colorIndex = value
    ? ACCENT_COLOR_PRESETS.indexOf(
        value as (typeof ACCENT_COLOR_PRESETS)[number],
      )
    : -1;
  const presetIndex = value == null ? 0 : colorIndex >= 0 ? colorIndex + 1 : -1;

  return (
    <div ref={root} className="w-48">
      <ColorSwatchRow
        colors={["var(--color-content)", ...ACCENT_COLOR_PRESETS]}
        labels={["Default", "Blue", "Violet", "Pink", "Red", "Orange", "Green"]}
        colorIndex={presetIndex >= 0 ? presetIndex : undefined}
        customColor={presetIndex < 0 ? (value ?? undefined) : undefined}
        customPickerOpen={open}
        onPickIndex={(index) => {
          setOpen(false);
          onChange(
            index === 0
              ? ACCENT_COLOR_DEFAULT
              : (ACCENT_COLOR_PRESETS[index - 1] ?? ACCENT_COLOR_PRESETS[0]),
          );
        }}
        onToggleCustom={() => setOpen((current) => !current)}
      />
      {open ? (
        <Popover
          anchor={root}
          side="bottom"
          align="end"
          width={248}
          onDismiss={() => setOpen(false)}
          className="px-2 pb-2"
        >
          <ColorPickerPopover
            value={value ?? ACCENT_COLOR_PRESETS[0]}
            onChange={onChange}
          />
        </Popover>
      ) : null}
    </div>
  );
}

/** macOS keeps the decision after the first prompt; only System Settings can flip it. Windows toasts are governed by Settings > Notifications. */
function NotificationsBlocked() {
  return (
    <span className="flex items-center gap-2 text-[12px] text-content/45">
      Permission needed
      {IS_MAC || IS_WIN ? (
        <button
          type="button"
          onClick={() => {
            void openNotificationSettings().catch(() => {});
          }}
          className="rounded-md border border-content/10 px-2 py-1 text-content/70 hover:bg-content/10 hover:text-content"
        >
          Open System Settings
        </button>
      ) : null}
    </span>
  );
}

function Toggle({
  label,
  on,
  onChange,
  disabled = false,
}: {
  label: string;
  on: boolean;
  onChange: (on: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-label={label}
      aria-checked={on}
      disabled={disabled}
      onClick={() => {
        onChange(!on);
        playCue("switch");
      }}
      className={`relative h-5 w-9 shrink-0 rounded-full transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
        on ? "bg-accent" : "bg-content/20"
      }`}
    >
      <span
        className={`absolute top-0.5 size-4 rounded-full bg-white transition-[left] ${
          on ? "left-4.5" : "left-0.5"
        }`}
      />
    </button>
  );
}

/** Theme-aware dropdown for a Settings row: a trigger button opening a Popover listbox. Used instead of a native select, whose option popup is OS-rendered and unreadable in dark mode on Windows/Linux. */
function Select({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: { value: string; label: string; icon?: ReactNode }[];
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(() =>
    Math.max(
      0,
      options.findIndex((option) => option.value === value),
    ),
  );
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const activeOption = useRef<HTMLButtonElement>(null);
  const listId = useId();
  const selected = options.find((option) => option.value === value);
  const activeId =
    options[active] != null ? `${listId}-opt-${active}` : undefined;

  useEffect(() => {
    if (!open) return;
    setActive(
      Math.max(
        0,
        options.findIndex((option) => option.value === value),
      ),
    );
  }, [open, value, options]);

  useEffect(() => {
    if (!open) return;
    activeOption.current?.scrollIntoView({ block: "nearest" });
  }, [active, open]);

  const pick = (next: string) => {
    onChange(next);
    setOpen(false);
    trigger.current?.focus();
  };

  const onMenuKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => Math.min(options.length - 1, i + 1));
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => Math.max(0, i - 1));
      return;
    }
    if (e.key === "Home") {
      e.preventDefault();
      setActive(0);
      return;
    }
    if (e.key === "End") {
      e.preventDefault();
      setActive(options.length - 1);
      return;
    }
    if (e.key === "Tab") {
      const option = options[active];
      if (option && option.value !== value) onChange(option.value);
      setOpen(false);
      trigger.current?.focus();
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const option = options[active];
      if (option) pick(option.value);
    }
  };

  return (
    <div ref={root} className="relative max-w-52">
      <button
        type="button"
        ref={trigger}
        aria-label={`${label}: ${selected?.label ?? value}`}
        aria-expanded={open}
        aria-haspopup="listbox"
        onClick={() => setOpen((prev) => !prev)}
        className="flex w-full items-center justify-between gap-2 rounded-md border border-content/10 bg-content/5 px-2 py-1 text-left text-[12px] text-content outline-none hover:border-content/20"
      >
        <span className="flex min-w-0 flex-1 items-center gap-1.5">
          {selected?.icon ? (
            <span className="grid size-4 shrink-0 place-items-center">
              {selected.icon}
            </span>
          ) : null}
          <span className="min-w-0 truncate">
            {selected ? selected.label : value}
          </span>
        </span>
        <ChevronDown
          className={`size-3.5 shrink-0 text-content/50 transition-transform ${open ? "rotate-180" : ""}`}
          strokeWidth={1.75}
        />
      </button>
      {open ? (
        <Popover
          anchor={root}
          side="bottom"
          align="end"
          width={280}
          maxHeight={320}
          autoFocus
          onDismiss={(reason) => {
            setOpen(false);
            if (reason === "escape") trigger.current?.focus();
          }}
          role="listbox"
          aria-label={label}
          aria-activedescendant={activeId}
          tabIndex={-1}
          onKeyDown={onMenuKey}
          className="overflow-y-auto overscroll-contain p-1"
        >
          {options.map((option, index) => {
            const isSelected = option.value === value;
            const highlighted = index === active;
            return (
              <button
                key={option.value}
                ref={highlighted ? activeOption : undefined}
                type="button"
                id={`${listId}-opt-${index}`}
                role="option"
                tabIndex={-1}
                aria-selected={isSelected}
                onMouseDown={(e) => e.preventDefault()}
                onMouseEnter={() => setActive(index)}
                onClick={() => pick(option.value)}
                className={`flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-[12px] ${
                  highlighted || isSelected
                    ? "bg-selection text-content"
                    : "text-content hover:bg-content/5"
                }`}
              >
                {option.icon ? (
                  <span className="grid size-4 shrink-0 place-items-center">
                    {option.icon}
                  </span>
                ) : null}
                <span className="min-w-0 flex-1 truncate">{option.label}</span>
                {isSelected ? (
                  <Check className="size-3.5 shrink-0" strokeWidth={2.25} />
                ) : null}
              </button>
            );
          })}
        </Popover>
      ) : null}
    </div>
  );
}
