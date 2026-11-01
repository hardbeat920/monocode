import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useProjectBranchesState } from "../hooks/useProjectBranches";
import { LAYER } from "../../../shared/lib/layers";
import { createWorktree, type Worktree } from "../model/worktrees";
import { prettyCwd } from "../../../shared/lib/paths";
import { Modal } from "../../../shared/ui/Modal";
import { SearchableSelect } from "../../../shared/ui/SearchableSelect";
import { Loader } from "../../../shared/ui/icons";
import { translate } from "../../../shared/i18n/locale";
import { useLanguage } from "../../../shared/i18n/useTranslation";

export function CreateWorktreeDialog({
  cwd,
  baseCwd,
  defaultRoot,
  onCreated,
  onCancel,
}: {
  cwd: string;
  baseCwd: string;
  defaultRoot?: string;
  onCreated: (tree: Worktree) => void | Promise<void>;
  onCancel: () => void;
}) {
  useLanguage();
  const { branches } = useProjectBranchesState(baseCwd, true);
  const [name, setName] = useState("");
  const [base, setBase] = useState("HEAD");
  const [existing, setExisting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (existing) return;
    const frame = requestAnimationFrame(() => input.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [existing]);
  const localBranches = useMemo(
    () =>
      (branches?.branches ?? [])
        .filter((branch) => !branch.remote)
        .map((branch) => ({ value: branch.name, label: branch.name })),
    [branches],
  );
  const baseOptions = useMemo(
    () => [
      {
        value: "HEAD",
        label: branches?.current
          ? translate("Current commit ({branch})", { branch: branches.current })
          : translate("Current commit"),
        keywords: "HEAD current commit",
      },
      ...(branches?.branches ?? []).map((branch) => {
        const ref = branch.remote
          ? `${branch.remote}/${branch.name}`
          : branch.name;
        return { value: ref, label: ref };
      }),
    ],
    [branches],
  );
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy || !name.trim()) return;
    setBusy(true);
    setError(undefined);
    try {
      const tree = await createWorktree(baseCwd, name.trim(), base, existing);
      await onCreated(tree);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };
  const field =
    "h-9 rounded-md border border-content/10 bg-background-base px-2.5 text-[13px] outline-none focus:border-content/25 disabled:opacity-50";
  return (
    <Modal
      title={translate("Create worktree")}
      size="sm"
      onClose={() => {
        if (!busy) onCancel();
      }}
    >
      <form
        className="flex flex-col gap-4 p-4"
        onSubmit={(e) => void submit(e)}
      >
        <p className="text-[12px] text-content/55">
          {translate(
            "An independent working copy of {path}. Existing uncommitted changes stay in their current working copy.",
            { path: prettyCwd(cwd) },
          )}
        </p>
        <div className="flex flex-col gap-1.5 text-[12px] text-content/70">
          <span>{translate("Branch")}</span>
          <SearchableSelect
            label={translate("Branch type")}
            disabled={busy}
            value={existing ? "existing" : "new"}
            options={[
              {
                value: "new",
                label: translate("Create a new branch"),
                keywords: "new create branch",
              },
              {
                value: "existing",
                label: translate("Use an existing local branch"),
                keywords: "existing local branch",
              },
            ]}
            onChange={(value) => {
              setExisting(value === "existing");
              setName("");
            }}
            searchPlaceholder={translate("Search options…")}
            layer={LAYER.dialogPopover}
          />
        </div>
        <div className="flex flex-col gap-1.5 text-[12px] text-content/70">
          <span>
            {existing
              ? translate("Existing branch")
              : translate("New branch name")}
          </span>
          {existing ? (
            <SearchableSelect
              label={translate("Existing branch")}
              value={name}
              disabled={busy}
              options={localBranches}
              onChange={setName}
              placeholder={translate("Choose a branch…")}
              searchPlaceholder={translate("Search local branches…")}
              emptyLabel={translate("No matching local branches")}
              layer={LAYER.dialogPopover}
            />
          ) : (
            <input
              ref={input}
              aria-label={translate("New branch name")}
              className={field}
              value={name}
              disabled={busy}
              placeholder="feature/my-task"
              autoComplete="off"
              spellCheck={false}
              onChange={(e) => setName(e.target.value)}
            />
          )}
        </div>
        {!existing && (
          <div className="flex flex-col gap-1.5 text-[12px] text-content/70">
            <span>{translate("Start from")}</span>
            <SearchableSelect
              label={translate("Start from")}
              value={base}
              disabled={busy}
              options={baseOptions}
              onChange={setBase}
              searchPlaceholder={translate("Search branches and refs…")}
              layer={LAYER.dialogPopover}
            />
          </div>
        )}
        {defaultRoot && (
          <p className="break-all text-[11px] text-content/40">
            {translate("Created in {path}", { path: prettyCwd(defaultRoot) })}
          </p>
        )}
        {error && (
          <p role="alert" className="text-[12px] text-red-400">
            {error}
          </p>
        )}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={onCancel}
            className="rounded-md px-3 py-1.5 text-[12px] hover:bg-content/8 active:scale-[0.97]"
          >
            {translate("Cancel")}
          </button>
          <button
            type="submit"
            disabled={busy || !name.trim()}
            className="inline-flex items-center gap-1.5 rounded-md bg-content px-3 py-1.5 text-[12px] font-medium text-background-base disabled:opacity-40 active:scale-[0.97]"
          >
            {busy && <Loader className="size-3.5 animate-spin" />}
            {translate("Create worktree")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
