import { useEffect, useRef, useState, type ReactNode } from "react";
import { ChevronRight } from "../../../shared/ui/icons";
import { PixelMascot } from "../../projects/ui/PixelMascot";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import {
  defaultMonoName,
  findMono,
  saveMonoMascot,
  saveMonoName,
  subscribeMonos,
  updateMono,
  type MonoLook,
} from "../model/mono";
import { ConfirmReset } from "./ConfirmReset";
import { ColorPicker, MascotPicker, PageHeader } from "./monoPanelParts";

export type SettingsPage = "habits" | "soul" | "memory";

/**
 * Who the Mono is: its face and name up top, then what it does, who it is
 * and what it remembers, each a page of its own.
 */
export function MonoSettingsPage({
  monoId,
  agent,
  onOpen,
  onBack,
  onReset,
  counts,
  children,
}: {
  monoId: string;
  agent: MonoLook;
  /** How many habits and facts it has, beside their rows once loaded. */
  counts?: { habits?: number; memory?: number };
  onOpen: (page: SettingsPage) => void;
  onBack?: () => void;
  onReset?: () => Promise<void>;
  /** Model and project controls, alongside the profile on the front panel. */
  children?: ReactNode;
}) {
  const lock = useLockOverscroll<HTMLDivElement>();
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-mono-settings>
      {onBack ? <PageHeader title="Settings" onBack={onBack} /> : null}
      <div
        ref={lock}
        className="flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-none"
      >
        <div className="flex flex-col items-center px-3 pb-4 pt-6">
          <PixelMascot
            name={agent.mascot}
            color={agent.color}
            className="size-14 shrink-0"
          />
          <NameField key={monoId} monoId={monoId} fallback={agent.mascot} />
        </div>

        <div className="flex flex-col gap-3 px-3 pb-4">
          <MascotPicker
            current={agent.mascot}
            color={agent.color}
            onPick={(mascot) => saveMonoMascot(monoId, mascot)}
          />
          <ColorPicker
            current={agent.color}
            onPick={(color) =>
              updateMono(monoId, (mono) => ({ ...mono, color }))
            }
          />
        </div>

        {children}

        <nav className="flex flex-col gap-px border-t border-stroke p-2">
          <NavRow
            label="Soul"
            description="Defines who this bot is and the rules it follows. Always included in its context."
            onClick={() => onOpen("soul")}
          />
          <NavRow
            label="Habits"
            description="Recurring tasks this bot runs on its own."
            count={counts?.habits}
            onClick={() => onOpen("habits")}
          />
          <NavRow
            label="Memory"
            description="Facts and preferences this bot remembers."
            count={counts?.memory}
            onClick={() => onOpen("memory")}
          />
        </nav>
        {onReset ? (
          <div className="mt-auto p-2">
            <ConfirmReset
              label="Reset conversation"
              title={`Reset ${agent.name}'s conversation?`}
              body="All messages in this Mono's conversation will be deleted and any active reply will be stopped. This can't be undone."
              kept="Its soul, memory and habits will be kept."
              failure="Could not reset the conversation."
              onConfirm={onReset}
            >
              {(open, ref) => (
                <button
                  ref={ref}
                  type="button"
                  onClick={open}
                  className="flex w-full flex-col rounded-lg px-3 py-2 text-left hover:bg-content/5"
                >
                  <span className="text-[13px] leading-5 text-red-400">
                    Reset conversation
                  </span>
                  <span className="text-[12px] leading-5 text-content/40">
                    Clear all messages and start fresh
                  </span>
                </button>
              )}
            </ConfirmReset>
          </div>
        ) : null}
      </div>
    </div>
  );
}

/** A row that opens a page: its name, a description, and a chevron. */
function NavRow({
  label,
  description,
  count,
  onClick,
}: {
  label: string;
  /** An explanation of the page, clamped to one line under the label. */
  description: string;
  /** How many entries the page holds, left of the chevron. */
  count?: number;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-content/5"
    >
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="text-[13px] leading-5 text-content/90">{label}</span>
        <span className="line-clamp-1 text-[12px] leading-5 text-content/40">
          {description}
        </span>
      </span>
      {count !== undefined ? (
        <span className="shrink-0 text-[12px] leading-5 tabular-nums text-content/40">
          {count}
        </span>
      ) : null}
      <ChevronRight
        className="size-3.5 shrink-0 text-content/30"
        strokeWidth={1.75}
      />
    </button>
  );
}

/** The Mono's name, edited in place; empty goes back to its mascot's name. */
function NameField({ monoId, fallback }: { monoId: string; fallback: string }) {
  const saved = () => findMono(monoId)?.name ?? "";
  const [draft, setDraft] = useState(saved);
  const editing = useRef(false);
  // A reset from Settings clears the name while this panel is open.
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
        if (event.key === "Enter") event.currentTarget.blur();
        if (event.key === "Escape") {
          setDraft(saved());
          event.currentTarget.blur();
        }
      }}
      className="mt-3 h-8 w-full rounded-md bg-transparent px-2 text-center text-[15px] font-medium text-content outline-none placeholder:text-content/60 hover:bg-content/5 focus:bg-content/5"
    />
  );
}
