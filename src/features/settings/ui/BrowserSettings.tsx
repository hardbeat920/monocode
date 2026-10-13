import { ask } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useState } from "react";
import { copyText } from "../../../platform/tauri/clipboard";
import { pickFolders } from "../../../platform/tauri/fs";
import { Copy, RefreshCw, RotateCcw, X } from "../../../shared/ui/icons";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import {
  BROWSER_CHANNELS,
  browserTabHost,
  browserTabStatus,
  clearBrowserAudit,
  closeAllBrowserTabs,
  closeBrowserTab,
  connectBrowser,
  connectionView,
  DEFAULT_BROWSER_SETTINGS,
  disconnectBrowser,
  inspectPages,
  listBrowserAudit,
  listBrowserMcpStatus,
  registerBrowserMcp,
  releaseBrowserTab,
  revokeBrowserSession,
  saveBrowserSettings,
  setBrowserToggle,
  transferBrowserTab,
  useBrowserState,
  type BrowserAuditEntry,
  type BrowserChoice,
  type BrowserLimits,
  type BrowserMcpRegistration,
  type BrowserOutcome,
  type BrowserSettings as Settings,
  type BrowserState,
  type BrowserTab,
  type ConnectionView,
} from "../model/browser";
import { MCP_PROVIDER_LABELS } from "../model/mcp";
import { Group, Row, Select, settingDomId, Toggle } from "./SettingsControls";

/** Resolves a session id to the name the sidebar shows. */
export type BrowserSessionName = (sessionId: string) => string;

const CUSTOM_PROFILE = "custom";
const OUTCOMES: BrowserOutcome[] = ["done", "blocked", "failed", "unknown"];

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function clockTime(ms: number): string {
  return new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Settings → Browser (spec 13.1): one page for the toggle, link and audit. */
export function BrowserSettings({
  sessionName,
  cwd,
}: {
  sessionName: BrowserSessionName;
  /** Project the provider CLIs are asked about when registering. */
  cwd: string;
}) {
  const state = useBrowserState();
  const [error, setError] = useState<string | null>(null);

  const run = useCallback((action: () => Promise<unknown>) => {
    setError(null);
    void action().catch((err: unknown) => setError(errorText(err)));
  }, []);

  if (!state) {
    return (
      <p className="text-[12px] text-content/45">
        Browser access is available in the desktop app.
      </p>
    );
  }

  const save = (patch: Partial<Settings>) =>
    run(() => saveBrowserSettings({ ...state.settings, ...patch }));

  return (
    <>
      {error ? (
        <p
          role="alert"
          className="mb-4 rounded-lg border border-red-400/30 bg-red-400/5 px-3 py-2 text-[12px] text-red-400"
        >
          {error}
        </p>
      ) : null}
      <Group
        title="Browser access"
        description="Agents open their own tabs in your Chrome or Edge, next to yours and signed in as you. They never see or touch your own tabs."
      >
        <Row
          id="browser-toggle"
          label="Let agents use my browser"
          description="Off after every restart. Turning it off stops every browser action, takes /browser away from every thread and leaves the agents' tabs open. A thread also needs /browser before it can use the browser."
        >
          <Toggle
            label="Let agents use my browser"
            on={state.toggle_on}
            onChange={(on) => run(() => setBrowserToggle(on))}
          />
        </Row>
        <BrowserChoiceRow
          choice={state.settings.browser}
          onChange={(browser) => save({ browser })}
        />
        <Row
          id="browser-separate-window"
          label="Agent tabs in a separate window"
          description="Off: agent tabs open in the background of your current window, with ◆ MonoCode in front of their title. On: every agent tab opens in a window of its own."
        >
          <Toggle
            label="Agent tabs in a separate window"
            on={state.settings.separate_window}
            onChange={(separate_window) => save({ separate_window })}
          />
        </Row>
      </Group>
      <ConnectionGroup state={state} run={run} />
      <SessionsGroup state={state} sessionName={sessionName} run={run} />
      <AuditGroup state={state} sessionName={sessionName} />
      <AdvancedGroup
        settings={state.settings}
        cwd={cwd}
        save={save}
        run={run}
      />
    </>
  );
}

function choiceValue(choice: BrowserChoice): string {
  if (choice.kind === "channel") return choice.channel;
  if (choice.kind === "user_data_dir") return CUSTOM_PROFILE;
  return "auto";
}

function BrowserChoiceRow({
  choice,
  onChange,
}: {
  choice: BrowserChoice;
  onChange: (choice: BrowserChoice) => void;
}) {
  const pickProfile = async () => {
    const [path] = await pickFolders("Choose the browser's user data folder");
    if (path) onChange({ kind: "user_data_dir", path });
  };
  return (
    <Row
      id="browser-choice"
      label="Browser"
      description={
        choice.kind === "user_data_dir"
          ? `Profile folder: ${choice.path}`
          : "Automatic tries Chrome, then Edge, then their Beta, Dev and Canary builds. Pick a profile folder for Snap, Flatpak or portable installs."
      }
    >
      <Select
        label="Browser"
        value={choiceValue(choice)}
        options={[
          { value: "auto", label: "Automatic" },
          ...BROWSER_CHANNELS.map((entry) => ({
            value: entry.channel,
            label: entry.label,
          })),
          { value: CUSTOM_PROFILE, label: "Profile folder…" },
        ]}
        onChange={(value) => {
          if (value === "auto") onChange({ kind: "auto" });
          else if (value === CUSTOM_PROFILE) void pickProfile();
          else onChange({ kind: "channel", channel: value });
        }}
      />
    </Row>
  );
}

const TONE_DOT: Record<ConnectionView["tone"], string> = {
  neutral: "bg-content/30",
  pending: "bg-amber-400 animate-pulse",
  good: "bg-emerald-400",
  warning: "bg-amber-400",
  error: "bg-red-400",
};

function ConnectionGroup({
  state,
  run,
}: {
  state: BrowserState;
  run: (action: () => Promise<unknown>) => void;
}) {
  const view = connectionView(state);
  const { connection } = state;
  const action =
    view.action === "disconnect" ? (
      <SecondaryButton onClick={() => run(disconnectBrowser)}>
        Disconnect
      </SecondaryButton>
    ) : view.action ? (
      <SecondaryButton onClick={() => run(connectBrowser)}>
        <RefreshCw className="size-3.5" strokeWidth={1.75} />
        {view.action === "retry" ? "Try again" : "Connect"}
      </SecondaryButton>
    ) : null;
  return (
    <Group
      id="browser-connection"
      title="Connection"
      description="MonoCode never starts your browser. It finds the one you already run and asks it for permission on every new connection."
    >
      <Row
        label={
          <span className="flex items-center gap-2">
            <span
              aria-hidden
              className={`size-2 shrink-0 rounded-full ${TONE_DOT[view.tone]}`}
            />
            {view.message}
          </span>
        }
        description={connectionDetail(state)}
      >
        {state.toggle_on ? action : null}
      </Row>
      {state.toggle_on && view.guide ? <ConsentGuide state={state} /> : null}
    </Group>
  );

  function connectionDetail(current: BrowserState): string | undefined {
    const parts: string[] = [];
    if (connection.browser) {
      parts.push(`Profile ${connection.browser.user_data_dir}`);
      if (current.connection.state === "connected")
        parts.push(`since ${clockTime(connection.since_ms)}`);
    }
    if (connection.last_error) parts.push(connection.last_error);
    else if (connection.detail && connection.state !== "connected")
      parts.push(connection.detail);
    return parts.length ? parts.join(" · ") : undefined;
  }
}

/** Guided consent (spec 13.3): the steps for the browser that was chosen. */
function ConsentGuide({ state }: { state: BrowserState }) {
  const pages = inspectPages(state);
  const [copied, setCopied] = useState<string | null>(null);
  return (
    <div className="border-t border-content/5 px-4 py-3.5 text-[12px] leading-relaxed text-content/65">
      <ol className="list-decimal space-y-2 pl-4">
        <li>
          Open{" "}
          {pages.map((page, index) => (
            <span key={page.url}>
              {index > 0 ? " or " : null}
              <button
                type="button"
                title={`Copy ${page.url}`}
                onClick={() => {
                  void copyText(page.url).then(() => setCopied(page.url));
                }}
                className="inline-flex items-center gap-1 rounded bg-content/8 px-1.5 py-0.5 font-mono text-[11px] text-content hover:bg-content/15"
              >
                {page.url}
                <Copy className="size-3" strokeWidth={1.75} />
              </button>
            </span>
          ))}{" "}
          in the browser and turn on remote debugging.
          {copied ? (
            <span className="ml-1 text-content/45">Copied.</span>
          ) : null}
        </li>
        <li>
          Click Connect here. The browser asks whether to allow the connection;
          choose Allow.
        </li>
        <li>
          While MonoCode is connected the browser shows that it is being
          controlled by automated software. The browser draws that bar and
          MonoCode can't hide it; it goes away when you disconnect.
        </li>
      </ol>
      <p className="mt-3 text-content/45">
        Agents can open, read and use their own tabs, with your sign-ins. They
        can't see your tabs, cookies or downloads, and every agent tab carries ◆
        MonoCode in its title.
      </p>
    </div>
  );
}

function SessionsGroup({
  state,
  sessionName,
  run,
}: {
  state: BrowserState;
  sessionName: BrowserSessionName;
  run: (action: () => Promise<unknown>) => void;
}) {
  const rows = state.sessions.filter(
    (session) =>
      session.granted ||
      state.tabs.some((t) => t.session_id === session.session_id),
  );
  const granted = state.sessions.filter((session) => session.granted);
  const owners = new Set(state.tabs.map((tab) => tab.session_id));
  const orphans = [...owners].filter(
    (id) => !rows.some((row) => row.session_id === id),
  );
  const liveTabs = state.tabs.filter((tab) => tab.state !== "revoked");
  return (
    <Group
      id="browser-sessions"
      title="Threads and tabs"
      description="Threads you gave /browser and the tabs their agents opened. Revoking keeps the tabs open; taking a tab back leaves it to you."
      action={
        liveTabs.length ? (
          <SecondaryButton
            danger
            onClick={() => {
              void ask(
                `Close ${liveTabs.length === 1 ? "the agent tab" : `all ${liveTabs.length} agent tabs`} in your browser?`,
                { title: "Close agent tabs", kind: "warning" },
              ).then((ok) => {
                if (ok) run(closeAllBrowserTabs);
              });
            }}
          >
            Close all agent tabs
          </SecondaryButton>
        ) : undefined
      }
    >
      {rows.length === 0 && orphans.length === 0 ? (
        <p className="px-4 py-3.5 text-[12px] text-content/45">
          No thread has browser access. Type /browser in a thread to give it
          access.
        </p>
      ) : null}
      {[...rows.map((row) => row.session_id), ...orphans].map((sessionId) => {
        const access = rows.find((row) => row.session_id === sessionId);
        const tabs = state.tabs.filter((tab) => tab.session_id === sessionId);
        const others = granted.filter((s) => s.session_id !== sessionId);
        return (
          <div
            key={sessionId}
            className="border-b border-content/5 px-4 py-3.5 last:border-b-0"
          >
            <div className="flex items-start gap-4">
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px] font-medium text-content">
                  {sessionName(sessionId)}
                </div>
                <p className="mt-0.5 text-[12px] text-content/45">
                  {access?.granted
                    ? `Access since ${
                        access.granted_since_ms
                          ? clockTime(access.granted_since_ms)
                          : "now"
                      }`
                    : "No access"}
                  {" · "}
                  {tabs.length === 1 ? "1 tab" : `${tabs.length} tabs`}
                  {access?.turn_active ? " · working" : ""}
                </p>
              </div>
              {access?.granted ? (
                <SecondaryButton
                  onClick={() => run(() => revokeBrowserSession(sessionId))}
                >
                  Revoke
                </SecondaryButton>
              ) : null}
            </div>
            {tabs.length ? (
              <ul className="mt-2 space-y-1">
                {tabs.map((tab) => (
                  <BrowserTabRow
                    key={tab.tab}
                    tab={tab}
                    others={others.map((s) => ({
                      value: s.session_id,
                      label: sessionName(s.session_id),
                    }))}
                    run={run}
                  />
                ))}
              </ul>
            ) : null}
          </div>
        );
      })}
    </Group>
  );
}

/** One agent tab with the actions Settings and the thread indicator share. */
export function BrowserTabRow({
  tab,
  others = [],
  run,
}: {
  tab: BrowserTab;
  /** Threads with access the tab can move to. */
  others?: { value: string; label: string }[];
  run: (action: () => Promise<unknown>) => void;
}) {
  const released = tab.state === "revoked";
  return (
    <li className="flex items-center gap-2 rounded-lg bg-content/3 px-2.5 py-1.5 text-[12px]">
      <div className="min-w-0 flex-1">
        <div className="truncate text-content">{tab.title || "Untitled"}</div>
        <div className="truncate text-content/45">
          {browserTabHost(tab.url)} · {browserTabStatus(tab)}
        </div>
      </div>
      {!released && others.length ? (
        <Select
          label="Move tab to"
          value=""
          options={[{ value: "", label: "Move to…" }, ...others]}
          onChange={(to) => {
            if (to) run(() => transferBrowserTab(tab.tab, tab.session_id, to));
          }}
        />
      ) : null}
      {!released ? (
        <SecondaryButton
          title="The agent loses this tab; it stays open for you"
          onClick={() => run(() => releaseBrowserTab(tab.tab))}
        >
          Take back
        </SecondaryButton>
      ) : null}
      <button
        type="button"
        title="Close tab"
        aria-label={`Close ${tab.title || "tab"}`}
        onClick={() => run(() => closeBrowserTab(tab.tab))}
        className="grid size-7 shrink-0 place-items-center rounded-md text-content/40 hover:bg-content/10 hover:text-content"
      >
        <X className="size-3.5" strokeWidth={1.75} />
      </button>
    </li>
  );
}

function AuditGroup({
  state,
  sessionName,
}: {
  state: BrowserState;
  sessionName: BrowserSessionName;
}) {
  const [session, setSession] = useState("");
  const [outcome, setOutcome] = useState<"" | BrowserOutcome>("");
  const [entries, setEntries] = useState<BrowserAuditEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);
  // A finished action changes the tabs' busy flags, so the list follows them.
  const activity = state.tabs
    .map((tab) => `${tab.tab}:${tab.last_action_at_ms}:${tab.busy}`)
    .join();

  useEffect(() => {
    let cancelled = false;
    const timer = window.setTimeout(() => {
      listBrowserAudit(session || undefined)
        .then((next) => {
          if (!cancelled) {
            setEntries(next);
            setError(null);
          }
        })
        .catch((err: unknown) => {
          if (!cancelled) setError(errorText(err));
        });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [session, nonce, activity]);

  const sessionOptions = useMemo(() => {
    const ids = new Set(state.sessions.map((s) => s.session_id));
    for (const entry of entries) ids.add(entry.session_id);
    return [...ids].map((id) => ({ value: id, label: sessionName(id) }));
  }, [state.sessions, entries, sessionName]);
  const shown = outcome
    ? entries.filter((entry) => entry.outcome === outcome)
    : entries;

  return (
    <Group
      id="browser-audit"
      title="Activity"
      description="The last 50 browser actions, kept for 30 days. Typed text and page content are never stored, and addresses are kept without their query."
      action={
        <div className="flex items-center gap-1.5">
          <SecondaryButton onClick={() => setNonce((n) => n + 1)}>
            <RefreshCw className="size-3.5" strokeWidth={1.75} />
            Refresh
          </SecondaryButton>
          <SecondaryButton
            danger
            disabled={entries.length === 0}
            onClick={() => {
              void ask("Delete the whole browser activity log?", {
                title: "Clear activity",
                kind: "warning",
              }).then((ok) => {
                if (!ok) return;
                clearBrowserAudit()
                  .then(() => setNonce((n) => n + 1))
                  .catch((err: unknown) => setError(errorText(err)));
              });
            }}
          >
            Clear
          </SecondaryButton>
        </div>
      }
    >
      <div className="flex flex-wrap items-center gap-2 border-b border-content/5 px-4 py-2.5">
        <Select
          label="Thread"
          value={session}
          options={[{ value: "", label: "All threads" }, ...sessionOptions]}
          onChange={setSession}
        />
        <Select
          label="Outcome"
          value={outcome}
          options={[
            { value: "", label: "All outcomes" },
            ...OUTCOMES.map((value) => ({ value, label: value })),
          ]}
          onChange={(value) => setOutcome(value as "" | BrowserOutcome)}
        />
      </div>
      {error ? (
        <p className="px-4 py-3 text-[12px] text-red-400">{error}</p>
      ) : shown.length === 0 ? (
        <p className="px-4 py-3.5 text-[12px] text-content/45">
          No browser actions yet.
        </p>
      ) : (
        <ul className="max-h-80 overflow-y-auto overscroll-contain">
          {shown.map((entry) => (
            <li
              key={entry.id}
              className="flex items-baseline gap-3 border-b border-content/5 px-4 py-1.5 text-[12px] last:border-b-0"
            >
              <span className="w-12 shrink-0 text-content/45 tabular-nums">
                {clockTime(entry.ts_ms)}
              </span>
              <span className="w-24 shrink-0 truncate font-mono text-[11px] text-content">
                {entry.action}
              </span>
              <span className="min-w-0 flex-1 truncate text-content/55">
                {entry.url ? browserTabHost(entry.url) : ""}
                {session ? "" : ` · ${sessionName(entry.session_id)}`}
              </span>
              <span
                className={`shrink-0 ${
                  entry.outcome === "done"
                    ? "text-content/55"
                    : entry.outcome === "blocked"
                      ? "text-amber-500"
                      : "text-red-400"
                }`}
              >
                {entry.reason
                  ? `${entry.outcome}: ${entry.reason}`
                  : entry.outcome}
              </span>
              <span className="w-14 shrink-0 text-right text-content/40 tabular-nums">
                {entry.duration_ms} ms
              </span>
            </li>
          ))}
        </ul>
      )}
    </Group>
  );
}

const LIMIT_FIELDS: {
  key: keyof BrowserLimits;
  label: string;
  max: number;
}[] = [
  { key: "tabs_per_session", label: "Tabs per thread", max: 64 },
  { key: "tabs_total", label: "Tabs in total", max: 128 },
  { key: "concurrent_actions", label: "Actions at once", max: 16 },
];

function AdvancedGroup({
  settings,
  cwd,
  save,
  run,
}: {
  settings: Settings;
  cwd: string;
  save: (patch: Partial<Settings>) => void;
  run: (action: () => Promise<unknown>) => void;
}) {
  const addRoots = async () => {
    const picked = await pickFolders("Folders agents may upload from");
    const roots = [...settings.upload_roots];
    for (const path of picked) if (!roots.includes(path)) roots.push(path);
    if (roots.length !== settings.upload_roots.length)
      save({ upload_roots: roots });
  };
  return (
    <Group
      title="Advanced"
      action={
        <SecondaryButton
          onClick={() =>
            run(() => saveBrowserSettings({ ...DEFAULT_BROWSER_SETTINGS }))
          }
        >
          <RotateCcw className="size-3.5" strokeWidth={1.75} />
          Restore defaults
        </SecondaryButton>
      }
    >
      <Row
        id="browser-upload-roots"
        label="Upload folders"
        description="Agents can upload files from the thread's project folder and from these folders."
      >
        <SecondaryButton onClick={() => void addRoots()}>
          Add folder…
        </SecondaryButton>
      </Row>
      {settings.upload_roots.length ? (
        <ul className="border-b border-content/5 px-4 pb-3 text-[12px]">
          {settings.upload_roots.map((root) => (
            <li
              key={root}
              className="flex items-center gap-2 py-0.5 text-content/65"
            >
              <span className="min-w-0 flex-1 truncate font-mono text-[11px]">
                {root}
              </span>
              <button
                type="button"
                aria-label={`Remove ${root}`}
                onClick={() =>
                  save({
                    upload_roots: settings.upload_roots.filter(
                      (entry) => entry !== root,
                    ),
                  })
                }
                className="grid size-6 place-items-center rounded-md text-content/40 hover:bg-content/10 hover:text-content"
              >
                <X className="size-3" strokeWidth={1.75} />
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      <Row
        id="browser-dotfiles"
        label="Allow hidden files in uploads"
        description="Files and folders whose name starts with a dot. SSH, cloud and Docker credentials stay blocked either way."
      >
        <Toggle
          label="Allow hidden files in uploads"
          on={settings.allow_dotfiles}
          onChange={(allow_dotfiles) => save({ allow_dotfiles })}
        />
      </Row>
      <McpRegistrationRow cwd={cwd} />
      <Row
        id="browser-limits"
        label="Limits"
        description="Over a tab limit the agent is asked to close a tab it no longer needs."
      >
        {LIMIT_FIELDS.map((field) => (
          <LimitInput
            key={field.key}
            label={field.label}
            value={settings.limits[field.key]}
            max={field.max}
            onChange={(value) =>
              save({ limits: { ...settings.limits, [field.key]: value } })
            }
          />
        ))}
      </Row>
    </Group>
  );
}

export function clampLimit(raw: string, max: number): number | null {
  const value = Number.parseInt(raw, 10);
  if (!Number.isFinite(value)) return null;
  return Math.min(max, Math.max(1, value));
}

function LimitInput({
  label,
  value,
  max,
  onChange,
}: {
  label: string;
  value: number;
  max: number;
  onChange: (value: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => {
    const next = clampLimit(draft, max);
    if (next == null || next === value) setDraft(String(value));
    else onChange(next);
  };
  return (
    <label className="flex items-center gap-1.5 text-[12px] text-content/55">
      {label}
      <input
        type="number"
        min={1}
        max={max}
        value={draft}
        aria-label={label}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") commit();
        }}
        className="w-14 rounded-md border border-content/10 bg-content/5 px-1.5 py-1 text-right text-content tabular-nums outline-none focus-visible:border-accent"
      />
    </label>
  );
}

function registrationLabel(entry: BrowserMcpRegistration): string {
  if (entry.unsupportedReason) return "Not supported";
  if (!entry.registered) return "Not added";
  return entry.current ? "Added" : "Points to an old copy of MonoCode";
}

/**
 * Where the browser tools are registered (spec 13.1 "status MCP
 * registracije"). MonoCode adds them on the first /browser of a provider;
 * this row shows the result and repairs an entry a moved app left behind.
 */
function McpRegistrationRow({ cwd }: { cwd: string }) {
  const [entries, setEntries] = useState<BrowserMcpRegistration[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(() => {
    listBrowserMcpStatus()
      .then((next) => {
        setEntries(next);
        setError(null);
      })
      .catch((err: unknown) => setError(errorText(err)));
  }, []);
  useEffect(load, [load]);

  const register = (provider: string) => {
    setBusy(provider);
    registerBrowserMcp(provider, cwd)
      .then(load)
      .catch((err: unknown) => setError(errorText(err)))
      .finally(() => setBusy(null));
  };

  return (
    <div
      id={settingDomId("browser-mcp")}
      data-setting-id="browser-mcp"
      className="border-b border-content/5 px-4 py-3.5"
    >
      <div className="text-[13px] font-medium text-content">
        Browser tools in providers
      </div>
      <p className="mt-1 text-[12px] leading-relaxed text-content/45">
        MonoCode adds its browser tools to a provider the first time one of its
        threads gets /browser. Agents already running see them after a restart.
        Other providers get the same actions through a command line.
      </p>
      {error ? <p className="mt-2 text-[12px] text-red-400">{error}</p> : null}
      {entries ? (
        <ul className="mt-2 space-y-1">
          {entries
            .filter((entry) => entry.supported || entry.unsupportedReason)
            .map((entry) => (
              <li
                key={entry.provider}
                className="flex flex-wrap items-center gap-x-2 text-[12px]"
              >
                <span className="w-28 shrink-0 text-content">
                  {MCP_PROVIDER_LABELS[
                    entry.provider as keyof typeof MCP_PROVIDER_LABELS
                  ] ?? entry.provider}
                </span>
                <span
                  className="min-w-0 flex-1 truncate text-content/45"
                  title={entry.configPath}
                >
                  {registrationLabel(entry)}
                </span>
                {entry.unsupportedReason ||
                (entry.registered && entry.current) ? null : (
                  <SecondaryButton
                    disabled={busy != null}
                    onClick={() => register(entry.provider)}
                  >
                    {busy === entry.provider
                      ? "Adding…"
                      : entry.registered
                        ? "Repair"
                        : "Add"}
                  </SecondaryButton>
                )}
                {entry.unsupportedReason ? (
                  <p className="basis-full pt-0.5 pl-30 leading-relaxed text-content/45">
                    {entry.unsupportedReason}
                  </p>
                ) : null}
              </li>
            ))}
        </ul>
      ) : null}
    </div>
  );
}
