import { Plus } from "./icons";
import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { looksLikeProject } from "../lib/recents";
import {
  isValidSkillName,
  slugSkillName,
  type Skill,
} from "../lib/skills";
import { useLockOverscroll } from "../hooks/useLockOverscroll";

type Props = {
  skills: Skill[];
  query: string;
  active: number;
  creating: boolean;
  cwd: string;
  error?: string | null;
  busy?: boolean;
  onActive: (index: number) => void;
  onPick: (skill: Skill) => void;
  onStartCreate: () => void;
  onCancelCreate: () => void;
  onCreate: (name: string, scope: "project" | "user") => void;
};

export function SkillPicker({
  skills,
  query,
  active,
  creating,
  cwd,
  error,
  busy,
  onActive,
  onPick,
  onStartCreate,
  onCancelCreate,
  onCreate,
}: Props) {
  return (
    <div
      data-skill-picker
      className="overflow-hidden rounded-lg border border-content/10 bg-content/5 backdrop-blur-xl"
    >
      {creating ? (
        <CreateSkillForm
          query={query}
          cwd={cwd}
          error={error}
          busy={busy}
          onCancel={onCancelCreate}
          onCreate={onCreate}
        />
      ) : (
        <>
          <SkillList
            skills={skills}
            query={query}
            active={active}
            onActive={onActive}
            onPick={onPick}
          />
          <button
            type="button"
            onMouseDown={(e) => e.preventDefault()}
            onClick={onStartCreate}
            className="flex w-full items-center gap-2 border-t border-content/10 px-2.5 py-2 text-left text-[12px] text-content/70 hover:bg-content/10 hover:text-content"
          >
            <Plus className="size-3.5 shrink-0" strokeWidth={1.75} />
            New skill
          </button>
        </>
      )}
    </div>
  );
}

function SkillList({
  skills,
  query,
  active,
  onActive,
  onPick,
}: {
  skills: Skill[];
  query: string;
  active: number;
  onActive: (index: number) => void;
  onPick: (skill: Skill) => void;
}) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const activeRef = useRef<HTMLButtonElement>(null);
  const pointer = useRef({ x: Number.NaN, y: Number.NaN, allow: false });
  const fromPointer = useRef(false);

  useEffect(() => {
    pointer.current.allow = false;
  }, [skills]);

  useEffect(() => {
    if (fromPointer.current) {
      fromPointer.current = false;
      return;
    }
    pointer.current.allow = false;
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const onListMouseMove = (e: ReactMouseEvent<HTMLDivElement>) => {
    if (e.clientX === pointer.current.x && e.clientY === pointer.current.y) {
      return;
    }
    pointer.current = { x: e.clientX, y: e.clientY, allow: true };
  };

  const onRowEnter = (index: number) => {
    if (!pointer.current.allow) return;
    fromPointer.current = true;
    onActive(index);
  };

  if (skills.length === 0) {
    return (
      <p className="px-3 py-2.5 text-[12px] text-content/50">
        {query.trim() ? "No matching skills" : "No skills yet"}
      </p>
    );
  }

  return (
    <div
      ref={lockOverscroll}
      role="listbox"
      aria-label="Skills"
      onMouseMove={onListMouseMove}
      className="max-h-[min(240px,40vh)] overflow-y-auto overscroll-none px-1 py-1"
    >
      {skills.map((skill, index) => {
        const highlighted = index === active;
        return (
          <button
            key={`${skill.kind}:${skill.source}:${skill.invocation}`}
            ref={highlighted ? activeRef : undefined}
            type="button"
            role="option"
            aria-selected={highlighted}
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => onRowEnter(index)}
            onClick={() => onPick(skill)}
            className={`flex w-full flex-col gap-0.5 rounded-md px-2 py-1.5 text-left ${
              highlighted ? "bg-skill/15 text-content" : "text-content"
            }`}
          >
            <span className="flex min-w-0 items-baseline gap-2">
              <span
                className={`truncate font-mono text-[13px] ${
                  highlighted ? "font-medium text-skill" : ""
                }`}
              >
                /{skill.invocation}
              </span>
              <span className="shrink-0 text-[10px] uppercase tracking-wide text-content/40">
                {scopeLabel(skill)}
              </span>
            </span>
            {skill.description ? (
              <span className="line-clamp-2 text-[11px] leading-4 text-content/50">
                {skill.description}
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}

function CreateSkillForm({
  query,
  cwd,
  error,
  busy,
  onCancel,
  onCreate,
}: {
  query: string;
  cwd: string;
  error?: string | null;
  busy?: boolean;
  onCancel: () => void;
  onCreate: (name: string, scope: "project" | "user") => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const project = looksLikeProject(cwd);
  const [name, setName] = useState(() => slugSkillName(query));
  const [scope, setScope] = useState<"project" | "user">(
    project ? "project" : "user",
  );

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  const slug = slugSkillName(name);
  const valid = isValidSkillName(slug);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!valid || busy) return;
    onCreate(slug, project ? scope : "user");
  };

  return (
    <form onSubmit={submit} className="px-2.5 py-2">
      <p className="mb-2 text-[11px] text-content/50">
        Writes a starter SKILL.md you can edit.
      </p>
      <input
        ref={input}
        value={name}
        spellCheck={false}
        placeholder="skill-name"
        aria-label="Skill name"
        disabled={busy}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key !== "Escape") return;
          e.preventDefault();
          onCancel();
        }}
        className="mb-2 w-full rounded-md bg-content/10 px-2 py-1.5 font-mono text-[13px] text-content outline-none placeholder:text-content/40"
      />
      <div className="mb-2 flex gap-1">
        <ScopeButton
          label="Project"
          hint=".agents/skills"
          selected={scope === "project"}
          disabled={!project || busy}
          onClick={() => setScope("project")}
        />
        <ScopeButton
          label="Personal"
          hint="~/.agents/skills"
          selected={scope === "user"}
          disabled={busy}
          onClick={() => setScope("user")}
        />
      </div>
      {error ? (
        <p className="mb-2 text-[12px] text-content/70">{error}</p>
      ) : !name.trim() || valid ? null : (
        <p className="mb-2 text-[12px] text-content/50">
          Use lowercase letters, numbers, and hyphens.
        </p>
      )}
      <div className="flex items-center justify-end gap-1">
        <button
          type="button"
          disabled={busy}
          onClick={onCancel}
          className="rounded-md px-2 py-1 text-[12px] text-content/50 hover:bg-content/10 hover:text-content"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={!valid || busy}
          className="rounded-md bg-content/20 px-2 py-1 text-[12px] text-content disabled:opacity-40"
        >
          {busy ? "Creating…" : "Create"}
        </button>
      </div>
    </form>
  );
}

function ScopeButton({
  label,
  hint,
  selected,
  disabled,
  onClick,
}: {
  label: string;
  hint: string;
  selected: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={`flex min-w-0 flex-1 flex-col rounded-md px-2 py-1.5 text-left ${
        selected ? "bg-content/20 text-content" : "bg-content/10 text-content/70"
      } disabled:opacity-40`}
    >
      <span className="text-[12px]">{label}</span>
      <span className="truncate font-mono text-[10px] text-content/40">
        {hint}
      </span>
    </button>
  );
}

function scopeLabel(skill: Skill): string {
  if (skill.kind === "native") return skill.source;
  if (skill.kind === "builtin") return "monocode";
  if (skill.scope === "user") return "personal";
  if (skill.source !== "agents" && skill.source !== "monocode") return skill.source;
  return "project";
}
