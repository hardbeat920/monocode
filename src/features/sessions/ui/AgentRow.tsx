import {
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import {
  Check,
  ChevronRight,
  CircleAlert,
  CircleHelp,
  CircleX,
  Copy,
  ExternalLink,
  Square,
  Trash2,
  type IconComponent,
} from "../../../shared/ui/icons";
import { Shimmer } from "../../../shared/ui/Shimmer";
import { copyMessage } from "../../../platform/tauri/clipboard";
import type { SessionUpdateStatus } from "../../agent-app/model/sessionLinks";
import {
  defaultModelId,
  findModel,
  getModelSnapshot,
  modelsFor,
  nativeModelId,
  resolveModel,
  subscribeModels,
} from "../model/models";
import { HARNESSES, type HarnessId } from "../model/session";
import { HarnessIcon } from "./HarnessIcon";

/** `iconOnly` tints just the icon, leaving the word in the meta colour. */
export const AGENT_STATUS: Record<
  SessionUpdateStatus,
  { word: string; icon: IconComponent; tone: string; iconOnly?: boolean }
> = {
  settled: {
    word: "replied",
    icon: Check,
    tone: "text-emerald-400",
    iconOnly: true,
  },
  stopped: { word: "stopped by you", icon: Square, tone: "text-content/50" },
  failed: { word: "failed", icon: CircleX, tone: "text-red-400" },
  interrupted: {
    word: "interrupted",
    icon: CircleAlert,
    tone: "text-amber-400",
  },
  removed: { word: "deleted", icon: Trash2, tone: "text-content/50" },
  approval: {
    word: "needs your approval",
    icon: CircleAlert,
    tone: "text-amber-400",
  },
  question: {
    word: "has a question",
    icon: CircleHelp,
    tone: "text-amber-400",
  },
};

export function StatusWord({ status }: { status: SessionUpdateStatus }) {
  const { word, icon: Icon, tone, iconOnly } = AGENT_STATUS[status];
  return (
    <span
      data-agent-status={status}
      className={`inline-flex items-center gap-1 whitespace-nowrap ${iconOnly ? "" : tone}`}
    >
      <Icon
        className={`size-3 shrink-0 ${iconOnly ? tone : ""}`}
        strokeWidth={2}
      />
      {word}
    </span>
  );
}

function isHarness(value: string | undefined): value is HarnessId {
  return !!value && (HARNESSES as string[]).includes(value);
}

/**
 * The catalog's name for a model id, or the id itself. `resolveModel` answers
 * an id it cannot place with the harness default, and naming that model would
 * misreport which one the agent runs.
 */
function readableModel(harness: string | undefined, id: string): string {
  if (!isHarness(harness)) return findModel(id)?.name ?? id;
  const model = resolveModel(harness, id);
  if (!model.id) return id;
  const placed =
    model.id === id ||
    nativeModelId(model) === nativeModelId(id) ||
    (model.id !== defaultModelId(harness) &&
      model !== modelsFor(harness)[0]);
  return placed ? model.name : id;
}

/** A readable model name that follows the catalog as it loads. An empty id has no name. */
export function useReadableModel(
  harness: string | undefined,
  id: string | undefined,
): string | undefined {
  useSyncExternalStore(subscribeModels, getModelSnapshot);
  return id ? readableModel(harness, id) : undefined;
}

/**
 * One line per agent the operator talks to, shaped like a delegated run's row:
 * the harness, the agent's name, then its model and status in muted text. The
 * body, when there is one, opens underneath on the activity rail.
 */
export function AgentRow({
  harness,
  name,
  model,
  status,
  active = false,
  label,
  subject,
  children,
  below,
}: {
  harness?: string;
  name: string;
  /** Readable model name. */
  model?: string;
  status?: ReactNode;
  /** The agent is working: its name shimmers. */
  active?: boolean;
  /** What the row says to a screen reader, such as "luna-hi, replied". */
  label: string;
  /** What opening the row shows, such as "task" or "reply". */
  subject: string;
  /** Absent for a row that has nothing to open. */
  children?: ReactNode;
  /** An indented second line under the row. */
  below?: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const glyph = isHarness(harness) ? (
    <HarnessIcon
      harness={harness}
      className={`size-3.5 shrink-0 ${active ? "text-content/60" : "text-content/45"}`}
    />
  ) : (
    <span className="size-3.5 shrink-0" />
  );
  const line = (
    <span className="flex min-w-0 flex-1 items-center gap-2">
      {active ? (
        <Shimmer className="min-w-0 truncate text-sm" duration={1.6}>
          {name}
        </Shimmer>
      ) : (
        <span className="min-w-0 truncate text-sm text-content/75 transition-colors duration-200 group-hover:text-content">
          {name}
        </span>
      )}
      {model || status ? (
        <span className="flex min-w-0 max-w-[55%] shrink-0 items-center gap-2 text-[12px] text-content/40">
          {model ? (
            <span className="truncate" title={`Model: ${model}`}>
              {model}
            </span>
          ) : null}
          {status ? (
            <span className="inline-flex shrink-0 items-center gap-1 whitespace-nowrap">
              {status}
            </span>
          ) : null}
        </span>
      ) : null}
    </span>
  );
  return (
    <div data-agent-row className="flex min-w-0 flex-col px-4 py-0.5 font-sans">
      {children ? (
        <>
          <button
            type="button"
            aria-expanded={open}
            aria-label={`${label}. ${open ? "Hide" : "Show"} ${subject}`}
            onClick={() => setOpen(!open)}
            className={`group -mx-1.5 flex min-w-0 items-center gap-2 rounded-md px-1.5 py-1 text-left transition-colors duration-200 hover:bg-content/8 ${
              open ? "bg-content/8" : ""
            }`}
          >
            {glyph}
            {line}
            <ChevronRight
              className={`size-3.5 shrink-0 text-content/35 transition-transform duration-200 group-hover:text-content/60 ${
                open ? "rotate-90" : ""
              }`}
              strokeWidth={1.75}
            />
          </button>
          <div className="zen-phase-body" data-open={open}>
            {open ? (
              <div className="flex min-w-0 flex-col pt-1 pb-2">
                <div className="zen-phase-step min-w-0 py-0.5">{children}</div>
              </div>
            ) : null}
          </div>
        </>
      ) : (
        <div
          aria-label={label}
          className="-mx-1.5 flex min-w-0 items-center gap-2 px-1.5 py-1"
        >
          {glyph}
          {line}
          <span className="size-3.5 shrink-0" />
        </div>
      )}
      {below ? (
        <div
          data-agent-row-detail
          className="pb-1 pl-[22px] text-[12.5px] leading-normal"
        >
          {below}
        </div>
      ) : null}
    </div>
  );
}

/** Raw identifiers live only here, behind an opened row. */
export function IdsLine({
  parts,
  id,
}: {
  parts: (string | undefined | false)[];
  /** A long session id, shortened in the middle; the full id is its title. */
  id?: string;
}) {
  const text = [
    ...parts,
    id && id.length > 28 ? `${id.slice(0, 12)}…${id.slice(-12)}` : id,
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <p
      title={id}
      className="mt-1.5 font-mono text-[11px] leading-normal text-content/35 [overflow-wrap:anywhere]"
    >
      {text}
    </p>
  );
}

const ACTION =
  "inline-flex h-7 items-center gap-1.5 rounded-md px-2 text-xs transition-colors";

export function RowActions({ children }: { children: ReactNode }) {
  return <div className="mt-1.5 -ml-2 flex flex-wrap gap-1">{children}</div>;
}

/** Opens a session in its tab. `warn` asks the reader to act there. */
export function OpenSessionAction({
  label,
  onOpen,
  warn = false,
}: {
  label: string;
  onOpen: () => void;
  warn?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onOpen}
      className={`${ACTION} ${
        warn
          ? "bg-amber-400/12 text-amber-400 hover:bg-amber-400/20"
          : "text-content/55 hover:bg-content/10 hover:text-content"
      }`}
    >
      {label}
      <ExternalLink className="size-3 shrink-0" strokeWidth={1.75} />
    </button>
  );
}

/** A truncated reply is only its tail, so it copies as the excerpt it is, ellipsis and all. */
export function CopyReplyAction({
  text,
  truncated = false,
}: {
  text: string;
  truncated?: boolean;
}) {
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (timer.current != null) window.clearTimeout(timer.current);
    },
    [],
  );

  return (
    <>
      <button
        type="button"
        className={`${ACTION} text-content/55 hover:bg-content/10 hover:text-content`}
        onClick={() => {
          setError(null);
          void copyMessage(truncated ? `…${text}` : text).then(
            () => {
              setCopied(true);
              if (timer.current != null) window.clearTimeout(timer.current);
              timer.current = window.setTimeout(() => setCopied(false), 2000);
            },
            (reason: unknown) =>
              setError(reason instanceof Error ? reason.message : String(reason)),
          );
        }}
      >
        {copied ? (
          <Check className="size-3 shrink-0" strokeWidth={1.75} />
        ) : (
          <Copy className="size-3 shrink-0" strokeWidth={1.75} />
        )}
        {copied ? "Copied" : truncated ? "Copy excerpt" : "Copy reply"}
      </button>
      {error ? (
        <span role="alert" className="self-center text-xs text-content/70">
          Copy failed. {error}
        </span>
      ) : null}
    </>
  );
}
