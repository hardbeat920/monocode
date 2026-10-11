import { useEffect, useState } from "react";
import { listSkills, type DiscoveredSkill } from "../../../platform/tauri/fs";
import { isRemoteProjectPath } from "../../projects/model/recents";
import { SkillDocumentPreview } from "../../skills/ui/SkillDocumentPreview";
import type { MonoFiles } from "../model/monoFiles";
import {
  assignMonoSkill,
  createMonoSkill,
  readMonoSkill,
  removeMonoSkill,
  updateMonoSkill,
  type MonoSkill,
} from "../model/monoSkills";
import { Empty, PageHeader } from "./monoPanelParts";

type View =
  { kind: "list" | "assign" | "new" } | { kind: "skill"; skill: MonoSkill };
const buttonClass =
  "rounded-md bg-content/10 px-2 py-1 text-[12px] enabled:hover:bg-content/[0.14] disabled:opacity-40";
const fieldClass =
  "w-full rounded-md border border-stroke bg-transparent px-2 py-1.5 text-[13px] text-content outline-none focus:border-content/30";
const errorText = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

/** Skills belong to a Mono across its projects, conversations and providers. */
export function MonoSkillsPage({
  monoId,
  cwd,
  projects,
  files,
  onBack,
}: {
  monoId: string;
  cwd: string;
  projects: readonly string[];
  files: MonoFiles | undefined;
  onBack: () => void;
}) {
  const [view, setView] = useState<View>({ kind: "list" });
  const [error, setError] = useState<string>();
  const [working, setWorking] = useState(false);
  const skills = files?.skills ?? [];
  const back = () => {
    setError(undefined);
    setView({ kind: "list" });
  };
  const remove = async (skill: MonoSkill) => {
    if (
      skill.owned &&
      !window.confirm(`Delete “${skill.name}”? This cannot be undone.`)
    )
      return;
    setWorking(true);
    setError(undefined);
    try {
      await removeMonoSkill(monoId, skill);
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setWorking(false);
    }
  };

  if (view.kind === "new")
    return <SkillEditor key="new" monoId={monoId} onBack={back} />;
  if (view.kind === "skill")
    return (
      <SkillEditor
        key={view.skill.path}
        monoId={monoId}
        skill={view.skill}
        onBack={back}
      />
    );
  if (view.kind === "assign")
    return (
      <AssignSkills
        monoId={monoId}
        cwd={cwd}
        projects={projects}
        assigned={skills}
        onBack={back}
      />
    );
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-mono-skills>
      <PageHeader title="Skills" onBack={onBack}>
        <button
          type="button"
          className={buttonClass}
          disabled={!files || working}
          onClick={() => setView({ kind: "assign" })}
        >
          Assign
        </button>
        <button
          type="button"
          className={buttonClass}
          disabled={!files || working}
          onClick={() => setView({ kind: "new" })}
        >
          New skill
        </button>
      </PageHeader>
      {error ? (
        <p role="alert" className="px-4 py-2 text-[12px] text-red-400">
          {error}
        </p>
      ) : null}
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none p-2">
        {!files ? (
          <Empty>Loading…</Empty>
        ) : skills.length === 0 ? (
          <Empty>Assign an existing skill or create one for this Mono.</Empty>
        ) : (
          <ul className="flex flex-col gap-1">
            {skills.map((skill) => (
              <li
                key={skill.name}
                className="flex items-start gap-2 rounded-lg px-2 py-2 hover:bg-content/5"
              >
                <button
                  type="button"
                  className="min-w-0 flex-1 text-left"
                  disabled={working}
                  onClick={() => setView({ kind: "skill", skill })}
                >
                  <span className="block text-[13px] text-content/90">
                    {skill.name}
                  </span>
                  <span className="block text-[12px] text-content/45">
                    {skill.description}
                  </span>
                  <span className="block pt-1 text-[11px] text-content/35">
                    {skill.owned
                      ? "Created for this Mono"
                      : "Assigned · shared"}
                    {skill.available ? "" : " · Unavailable"}
                  </span>
                </button>
                <button
                  type="button"
                  className="px-1 py-0.5 text-[12px] text-content/45 hover:text-red-400 disabled:opacity-40"
                  disabled={working}
                  aria-label={`${skill.owned ? "Delete" : "Unassign"} skill ${skill.name}`}
                  onClick={() => void remove(skill)}
                >
                  {skill.owned ? "Delete" : "Unassign"}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

function AssignSkills({
  monoId,
  cwd,
  projects,
  assigned,
  onBack,
}: {
  monoId: string;
  cwd: string;
  projects: readonly string[];
  assigned: readonly MonoSkill[];
  onBack: () => void;
}) {
  const [catalog, setCatalog] = useState<DiscoveredSkill[]>();
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string>();
  const [working, setWorking] = useState(false);
  const rootsKey = JSON.stringify([
    ...new Set(
      [cwd, ...projects].filter((path) => !!path && !isRemoteProjectPath(path)),
    ),
  ]);
  useEffect(() => {
    let live = true;
    const roots = JSON.parse(rootsKey) as string[];
    void Promise.all(roots.map((root) => listSkills(root)))
      .then((lists) => {
        if (!live) return;
        setCatalog([
          ...new Map(lists.flat().map((skill) => [skill.path, skill])).values(),
        ]);
      })
      .catch((reason) => {
        if (live) setError(errorText(reason));
      });
    return () => {
      live = false;
    };
  }, [rootsKey]);
  const assign = async (skill: DiscoveredSkill) => {
    setWorking(true);
    setError(undefined);
    try {
      await assignMonoSkill(monoId, skill);
      onBack();
    } catch (reason) {
      setError(errorText(reason));
      setWorking(false);
    }
  };
  const filtered = catalog?.filter(
    (skill) =>
      !assigned.some((item) => item.name === skill.name) &&
      `${skill.name} ${skill.description} ${skill.path}`
        .toLowerCase()
        .includes(query.toLowerCase()),
  );
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Assign skill" onBack={onBack} />
      <div className="px-4 pt-3">
        <input
          aria-label="Find skill to assign"
          placeholder="Find a skill"
          className={fieldClass}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
      </div>
      <p className="px-4 py-2 text-[12px] text-content/45">
        Choose from your personal skills and this Mono’s projects. Changes to an
        assigned skill are shared with every Mono using it.
      </p>
      {error ? (
        <p role="alert" className="px-4 py-2 text-[12px] text-red-400">
          {error}
        </p>
      ) : null}
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none p-2">
        {!catalog && !error ? (
          <Empty>Loading…</Empty>
        ) : filtered?.length === 0 ? (
          <Empty>No matching skills available to assign.</Empty>
        ) : (
          filtered?.map((skill) => (
            <button
              key={skill.path}
              type="button"
              disabled={working}
              aria-label={`Assign skill ${skill.name} from ${skill.path}`}
              onClick={() => void assign(skill)}
              className="block w-full rounded-lg px-3 py-2 text-left hover:bg-content/5 disabled:opacity-40"
            >
              <span className="block text-[13px] text-content/90">
                {skill.name}
              </span>
              <span className="block text-[12px] text-content/45">
                {skill.description}
              </span>
              <span className="block break-all pt-1 text-[11px] text-content/30">
                {skill.path}
              </span>
            </button>
          ))
        )}
      </div>
    </div>
  );
}

function SkillEditor({
  monoId,
  skill,
  onBack,
}: {
  monoId: string;
  skill?: MonoSkill;
  onBack: () => void;
}) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [instructions, setInstructions] = useState("");
  const [loaded, setLoaded] = useState<{ text: string; hash: string }>();
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (!skill) return;
    let live = true;
    void readMonoSkill(monoId, skill.name)
      .then((file) => {
        if (!live) return;
        setLoaded(file);
        setInstructions(file.text);
      })
      .catch((reason) => {
        if (live) setError(errorText(reason));
      });
    return () => {
      live = false;
    };
  }, [monoId, skill]);
  const editable = !skill || skill.owned;
  const ready =
    !saving &&
    (skill
      ? !!loaded && !!instructions.trim()
      : !!name.trim() && !!description.trim() && !!instructions.trim());
  const save = async () => {
    if (!ready) return;
    setSaving(true);
    setError(undefined);
    try {
      if (skill && loaded)
        await updateMonoSkill(monoId, skill.name, instructions, loaded.hash);
      else await createMonoSkill(monoId, { name, description, instructions });
      onBack();
    } catch (reason) {
      setError(errorText(reason));
      setSaving(false);
    }
  };
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader title={skill?.name ?? "New skill"} onBack={onBack}>
        {editable ? (
          <button
            type="button"
            className={buttonClass}
            disabled={!ready}
            onClick={() => void save()}
          >
            {skill ? "Save" : "Create"}
          </button>
        ) : null}
      </PageHeader>
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none px-4 py-3">
        {!skill ? (
          <div className="flex flex-col gap-3">
            <label className="text-[12px] text-content/55">
              Name
              <input
                aria-label="Skill name"
                placeholder="review-pr"
                maxLength={64}
                className={`${fieldClass} mt-1`}
                value={name}
                disabled={saving}
                onChange={(event) => setName(event.target.value)}
              />
            </label>
            <label className="text-[12px] text-content/55">
              When to use it
              <textarea
                aria-label="Skill description"
                placeholder="Review pull requests against our team’s standards."
                maxLength={1024}
                rows={3}
                className={`${fieldClass} mt-1 resize-y`}
                value={description}
                disabled={saving}
                onChange={(event) => setDescription(event.target.value)}
              />
            </label>
          </div>
        ) : (
          <p className="mb-3 break-all text-[11px] text-content/35">
            {skill.path}
          </p>
        )}
        {!skill || loaded ? (
          editable ? (
            <label className="mt-3 block text-[12px] text-content/55">
              {skill ? "SKILL.md" : "Instructions"}
              <textarea
                aria-label="Skill instructions"
                placeholder="Describe the steps this Mono should follow."
                className={`${fieldClass} mt-1 min-h-64 resize-y font-mono text-[12px]`}
                value={instructions}
                disabled={saving}
                onChange={(event) => setInstructions(event.target.value)}
              />
            </label>
          ) : (
            <SkillDocumentPreview text={loaded!.text} />
          )
        ) : !error ? (
          <Empty>Loading skill…</Empty>
        ) : null}
        {error ? (
          <p role="alert" className="pt-3 text-[12px] text-red-400">
            {error}
          </p>
        ) : null}
      </div>
    </div>
  );
}
