import {
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
  type ReactNode,
} from "react";
import { isImeComposition } from "../../../shared/lib/keyboard";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { Modal } from "../../../shared/ui/Modal";
import { Popover } from "../../../shared/ui/Popover";
import { Settings, Sparkles, StickyNote } from "../../../shared/ui/icons";
import type { HarnessId, RuntimeMode } from "../../sessions/model/session";
import { AccessPicker } from "../../sessions/ui/AccessPicker";
import { ModelPicker, ModelSettingRows } from "../../sessions/ui/ModelPicker";
import { PixelMascot } from "../../projects/ui/PixelMascot";
import {
  defaultMonoName,
  findMono,
  saveMonoMascot,
  saveMonoName,
  subscribeMonos,
  updateMono,
  type MonoLook,
  type MonoState,
} from "../model/mono";
import { ConfirmReset } from "./ConfirmReset";
import { MemoryPage, SoulPage, useMonoFiles } from "./MonoFilePages";
import { MonoProjects } from "./MonoProjects";
import { ColorPicker, MascotPicker, SwitchRow } from "./monoPanelParts";

export type MonoSettingsSection = "general" | "soul" | "memory";

const SECTIONS: {
  id: MonoSettingsSection;
  label: string;
  icon: typeof Settings;
}[] = [
  { id: "general", label: "General", icon: Settings },
  { id: "soul", label: "Soul", icon: Sparkles },
  { id: "memory", label: "Memory", icon: StickyNote },
];

type Props = {
  monoId: string;
  agent: MonoLook;
  /** Its soul and memory reload when a turn ends. */
  status: MonoState["status"];
  /** Its conversation's folder, which the model picker reads settings from. */
  cwd: string;
  harness: HarnessId;
  model: string;
  modelSettings: Record<string, string>;
  runtimeMode: RuntimeMode;
  busy?: boolean;
  initialSection?: MonoSettingsSection;
  onModelChange: (harness: HarnessId, model: string) => void;
  onModelSettingsChange: (settings: Record<string, string>) => void;
  onRuntimeModeChange: (mode: RuntimeMode) => void;
  onReset?: () => Promise<void>;
  onClose: () => void;
};

/**
 * Who the Mono is and how it works, in a dialog of its own: its name, face,
 * model and projects under General, then its soul and memory.
 */
export function MonoSettingsDialog({
  monoId,
  agent,
  status,
  initialSection = "general",
  onClose,
  ...general
}: Props) {
  const [section, setSection] = useState(initialSection);
  const files = useMonoFiles(monoId, status);
  return (
    <Modal
      title={`${agent.name} settings`}
      size="lg"
      minimalHeader
      fitViewport
      onClose={onClose}
      // Centre the close button in the section header's 40px row.
      className="h-[min(680px,calc(100dvh-32px))] [&>.modal-panel>header]:top-1.5"
    >
      <div
        className="flex h-full min-h-0 font-sans"
        // Section headers leave room for the dialog's close button.
        style={{ "--mono-window-controls-width": "2rem" } as CSSProperties}
        data-mono-settings
      >
        <nav
          aria-label="Settings sections"
          className="flex w-48 shrink-0 flex-col gap-px border-r border-stroke p-2"
        >
          <p className="px-2.5 pb-2 pt-1.5 text-[11px] text-content/45">
            Settings
          </p>
          {SECTIONS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              aria-current={section === id ? "page" : undefined}
              onClick={() => setSection(id)}
              className={`flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px] ${
                section === id
                  ? "bg-content/8 text-content"
                  : "text-content/65 hover:bg-content/5 hover:text-content"
              }`}
            >
              <Icon className="size-3.5 shrink-0" strokeWidth={1.75} />
              {label}
            </button>
          ))}
        </nav>
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          {section === "general" ? (
            <GeneralSection monoId={monoId} agent={agent} {...general} />
          ) : section === "soul" ? (
            <SoulPage monoId={monoId} agent={agent} files={files} />
          ) : (
            <MemoryPage monoId={monoId} files={files} />
          )}
        </div>
      </div>
    </Modal>
  );
}

function GeneralSection({
  monoId,
  agent,
  cwd,
  harness,
  model,
  modelSettings,
  runtimeMode,
  busy = false,
  onModelChange,
  onModelSettingsChange,
  onRuntimeModeChange,
  onReset,
}: Omit<Props, "status" | "initialSection" | "onClose">) {
  const lock = useLockOverscroll<HTMLDivElement>();
  const shown = useSyncExternalStore(
    subscribeMonos,
    () => findMono(monoId)?.showStartedSessionsInSidebar === true,
  );
  const folders = useSyncExternalStore(
    subscribeMonos,
    () => findMono(monoId)?.useSidebarFolders === true,
  );
  return (
    <div
      ref={lock}
      className="min-h-0 flex-1 overflow-y-auto overscroll-none px-8 pb-8 pt-6"
    >
      <h3 className="text-[14px] font-medium text-content">General</h3>

      <div className="flex flex-col items-center pb-6 pt-4">
        <LookButton monoId={monoId} agent={agent} />
        <NameField key={monoId} monoId={monoId} fallback={agent.mascot} />
      </div>

      <GroupHeading title="Model" />
      <SettingGroup>
        <SettingRow
          label="Model"
          description="What it thinks and replies with."
        >
          <ModelPicker
            harness={harness}
            model={model}
            values={modelSettings}
            project={cwd}
            hideSettings
            side="bottom"
            variant="plain"
            onChange={onModelChange}
            onSettingsChange={onModelSettingsChange}
          />
        </SettingRow>
        <ModelSettingRows
          harness={harness}
          model={model}
          values={modelSettings}
          side="bottom"
          onSettingsChange={onModelSettingsChange}
          row={({ label, control }) => (
            <SettingRow label={label}>{control}</SettingRow>
          )}
        />
        <SettingRow
          label="Permissions"
          description="What it may do without asking."
        >
          <AccessPicker
            value={runtimeMode}
            onChange={onRuntimeModeChange}
            busy={busy}
            side="bottom"
            variant="plain"
          />
        </SettingRow>
      </SettingGroup>

      <GroupHeading
        title="Projects"
        description="It can see every session, note and folder in these."
      />
      <SettingGroup>
        <MonoProjects monoId={monoId} projects={agent.projects} />
      </SettingGroup>

      <GroupHeading title="Sessions it starts" />
      <SettingGroup>
        <SwitchRow
          label="Show sessions in sidebar"
          description="Sessions it starts appear in your project sidebar. Hidden ones stay saved and open from its overview."
          on={shown}
          onChange={(on) =>
            updateMono(monoId, (mono) => ({
              ...mono,
              showStartedSessionsInSidebar: on,
            }))
          }
        />
        {/* Hidden sessions have no sidebar folder to go in. */}
        <SwitchRow
          label="Group in a folder"
          description={`Sessions it starts go in a “${agent.name}” folder in your project sidebar.`}
          on={shown && folders}
          disabled={!shown}
          onChange={(on) =>
            updateMono(monoId, ({ useSidebarFolders: _, ...mono }) =>
              on ? { ...mono, useSidebarFolders: true } : mono,
            )
          }
        />
      </SettingGroup>

      {onReset ? (
        <>
          <GroupHeading title="Conversation" />
          <ConfirmReset
            label="Reset conversation"
            title={`Reset ${agent.name}'s conversation?`}
            body="All messages in this Mono's conversation will be deleted and any active reply will be stopped. This can't be undone."
            kept="Its soul, memory and habits will be kept."
            failure="Could not reset the conversation."
            onConfirm={onReset}
          >
            {(open, ref) => (
              <SettingGroup>
                <SettingRow
                  label="Reset conversation"
                  description="Clear all messages and start fresh. Its soul, memory and habits stay."
                >
                  <button
                    ref={ref}
                    type="button"
                    onClick={open}
                    className="rounded-md border border-stroke px-3 py-1.5 text-[12px] text-red-400 hover:bg-red-500/10"
                  >
                    Reset
                  </button>
                </SettingRow>
              </SettingGroup>
            )}
          </ConfirmReset>
        </>
      ) : null}
    </div>
  );
}

/** A titled group of settings, with what it covers under the title. */
function GroupHeading({
  title,
  description,
}: {
  title: string;
  description?: string;
}) {
  return (
    <div className="px-1 pb-2 pt-7">
      <h4 className="text-[12px] font-medium text-content/70">{title}</h4>
      {description ? (
        <p className="text-[12px] leading-5 text-content/40">{description}</p>
      ) : null}
    </div>
  );
}

/** Settings that belong together, in one card with a line between rows. */
function SettingGroup({ children }: { children: ReactNode }) {
  return (
    <div className="flex flex-col divide-y divide-stroke rounded-xl border border-stroke bg-content/[0.02]">
      {children}
    </div>
  );
}

/**
 * A setting's name and what it does on the left, its control on the right.
 * Controls get a column of their own, wide enough that a value is never cut.
 */
function SettingRow({
  label,
  description,
  children,
}: {
  label: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex min-h-12 items-center gap-4 px-3 py-2">
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="text-[13px] leading-5 text-content/90">{label}</span>
        {description ? (
          <span className="text-[12px] leading-5 text-content/40">
            {description}
          </span>
        ) : null}
      </div>
      <div className="flex w-56 shrink-0 items-center justify-end pr-1.5 text-[12px] text-content/85">
        {children}
      </div>
    </div>
  );
}

/** The Mono's face, which opens its mascots and colors in a popover. */
function LookButton({ monoId, agent }: { monoId: string; agent: MonoLook }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button
        ref={anchor}
        type="button"
        aria-label="Change mascot and color"
        aria-haspopup="dialog"
        aria-expanded={open}
        title="Change mascot and color"
        onClick={() => setOpen((value) => !value)}
        className={`grid size-20 shrink-0 place-items-center rounded-2xl border border-stroke hover:bg-content/8 ${
          open ? "bg-content/8" : "bg-content/4"
        }`}
      >
        <PixelMascot
          name={agent.mascot}
          color={agent.color}
          className="size-12 shrink-0"
        />
      </button>
      {open ? (
        <Popover
          anchor={anchor}
          side="bottom"
          align="center"
          width={272}
          constrainHeight={false}
          onDismiss={() => setOpen(false)}
          role="dialog"
          aria-label="Mascot and color"
          className="flex flex-col gap-3 p-3"
        >
          <MascotPicker
            columns={5}
            current={agent.mascot}
            color={agent.color}
            onPick={(mascot) => saveMonoMascot(monoId, mascot)}
          />
          <div className="h-px bg-content/10" />
          <ColorPicker
            inline
            columns={5}
            current={agent.color}
            onPick={(color) =>
              updateMono(monoId, (mono) => ({ ...mono, color }))
            }
          />
        </Popover>
      ) : null}
    </>
  );
}

/** The Mono's name, edited in place; empty goes back to its mascot's name. */
function NameField({ monoId, fallback }: { monoId: string; fallback: string }) {
  const saved = () => findMono(monoId)?.name ?? "";
  const [draft, setDraft] = useState(saved);
  const editing = useRef(false);
  // A reset clears the name while this dialog is open.
  useEffect(
    () =>
      subscribeMonos(() => {
        if (!editing.current) setDraft(saved());
      }),
    [monoId],
  );
  const placeholder = defaultMonoName(fallback);
  const save = () => {
    const name = draft.trim().slice(0, 40);
    if (name !== saved()) saveMonoName(monoId, name);
    setDraft(name);
  };
  return (
    <input
      aria-label="Name"
      value={draft}
      placeholder={placeholder}
      maxLength={40}
      onChange={(event) => setDraft(event.target.value)}
      onFocus={() => (editing.current = true)}
      onBlur={() => {
        editing.current = false;
        save();
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter" && !isImeComposition(event.nativeEvent)) {
          event.currentTarget.blur();
        }
        if (event.key === "Escape") {
          setDraft(saved());
          event.currentTarget.blur();
        }
      }}
      className="mt-3 h-8 w-full max-w-64 rounded-md bg-transparent px-2 text-center text-[15px] font-medium text-content outline-none placeholder:text-content/60 hover:bg-content/5 focus:bg-content/5"
    />
  );
}
