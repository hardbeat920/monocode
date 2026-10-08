import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { LAYER } from "../../../shared/lib/layers";
import { SearchableSelect } from "../../../shared/ui/SearchableSelect";
import { Lock } from "../../../shared/ui/icons";
import { pickFolder } from "../../../platform/tauri/fs";
import { remoteRequest, useRemoteMachines } from "../../connections/model/connections";
import type { HostDirectory } from "../../connections/model/protocol";
import { FolderRow } from "../../connections/ui/AddRemoteProjectDialog";
import { InboxProviderMark } from "../../inbox/ui/InboxProviderMark";
import {
  cachedRepos,
  localCloneLocation,
  localDefaultCloneParent,
  rememberCloneParent,
  rememberedCloneParent,
  remoteCloneLocation,
  useGitHostAvailability,
  type CloneTarget,
} from "../model/gitHosts";
import { gitHostUi, parseRepoInput } from "../model/providers";
import type { CheckoutPlan, GitHostId, GitHostRepo } from "../model/types";

const errorText = (reason: unknown) => String(reason).replace(/^Error: /, "");
const MAX_LISTED_REPOS = 60;

/** Starts a project from a hosted repository: opens a folder that already
 * tracks it, or clones it, on this computer or a connected machine. */
export function CloneRepoDialog({
  provider,
  target,
  onCancel,
  onOpen,
}: {
  provider: GitHostId;
  target: CloneTarget;
  onCancel: () => void;
  /** Receives the project's rail key: its path, or a remote project key. */
  onOpen: (key: string) => void;
}) {
  const ui = gitHostUi(provider);
  const remote = target === "remote";
  const { machines } = useRemoteMachines(remote);
  const availability = useGitHostAvailability();
  // Machines stay listed once seen signed in, so a check that fails while
  // the dialog is open does not pull the form out from under the user.
  const eligibleIds = useRef(new Set<string>());
  for (const id of availability.remote[provider] ?? []) eligibleIds.current.add(id);
  const eligible = machines.filter((machine) => eligibleIds.current.has(machine.id));
  const [machineId, setMachineId] = useState<string>();
  const machine = remote ? (eligible.find((entry) => entry.id === machineId) ?? eligible[0]) : undefined;
  const locationKey = remote ? machine?.environmentId : "local";
  const location = useMemo(
    () => (!remote ? localCloneLocation : machine ? remoteCloneLocation(machine) : undefined),
    [remote, machine?.id, machine?.environmentId],
  );

  const [repos, setRepos] = useState<GitHostRepo[]>();
  const [reposError, setReposError] = useState("");
  const [query, setQuery] = useState("");
  const [parent, setParent] = useState("");
  const [browsing, setBrowsing] = useState<HostDirectory>();
  const [plan, setPlan] = useState<CheckoutPlan>();
  const [planError, setPlanError] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const alive = useRef(true);
  const checkout = useRef<AbortController>(undefined);
  const browseVersion = useRef(0);
  // Set on every mount: development StrictMode mounts, unmounts and mounts
  // again, and responses after the first cleanup must still be shown.
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      checkout.current?.abort();
    };
  }, []);

  const cancel = () => {
    alive.current = false;
    checkout.current?.abort();
    onCancel();
  };
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      cancel();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onCancel]);

  // Repositories and the default folder follow the chosen location.
  useEffect(() => {
    if (!location || !locationKey) return;
    let current = true;
    browseVersion.current++;
    setRepos(undefined);
    setReposError("");
    setBrowsing(undefined);
    cachedRepos(locationKey, location, provider).then(
      (value) => current && setRepos(value),
      (reason) => current && setReposError(errorText(reason)),
    );
    const remembered = rememberedCloneParent(locationKey);
    setParent(remembered ?? "");
    if (!remembered) {
      const fallback = machine
        ? remoteRequest<HostDirectory>(machine.id, "projects.browse", {}).then((home) => home.path)
        : localDefaultCloneParent();
      fallback.then((path) => current && setParent((value) => value || path), () => undefined);
    }
    return () => {
      current = false;
    };
  }, [location, locationKey, provider]);

  const slug = parseRepoInput(ui, query);
  const needle = query.trim().toLowerCase();
  const matches = useMemo(
    () =>
      (needle && !slug
        ? (repos ?? []).filter(
            (repo) =>
              repo.slug.toLowerCase().includes(needle) ||
              repo.description?.toLowerCase().includes(needle),
          )
        : (repos ?? [])
      ).slice(0, MAX_LISTED_REPOS),
    [repos, needle, slug],
  );
  // Enter with a partial name takes the first repository whose name matches;
  // a match only in a description is listed but never picked implicitly.
  const chosen =
    slug ?? (needle ? matches.find((repo) => repo.slug.toLowerCase().includes(needle))?.slug : undefined);

  // Shows where the repository will land before anything is cloned.
  useEffect(() => {
    setPlan(undefined);
    setPlanError("");
    if (!location || !chosen || !parent.trim()) return;
    let current = true;
    const timer = setTimeout(() => {
      location.plan(provider, chosen, parent.trim()).then(
        (value) => current && setPlan(value),
        (reason) => current && setPlanError(errorText(reason)),
      );
    }, 250);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [location, chosen, parent, provider]);

  const browse = async (path?: string) => {
    if (!machine) return;
    const version = ++browseVersion.current;
    try {
      const directory = await remoteRequest<HostDirectory>(machine.id, "projects.browse", { path });
      if (!alive.current || version !== browseVersion.current) return;
      setBrowsing(directory);
      setParent(directory.path);
    } catch (reason) {
      if (alive.current && version === browseVersion.current) setError(errorText(reason));
    }
  };

  const chooseLocalParent = async () => {
    const path = await pickFolder("Clone into");
    if (path && alive.current) setParent(path);
  };

  const submit = async () => {
    if (!location || !locationKey || !chosen || !parent.trim() || busy) return;
    const controller = new AbortController();
    checkout.current = controller;
    setBusy(true);
    setError("");
    try {
      const folder = parent.trim();
      const key = await location.checkout(provider, chosen, folder, controller.signal);
      // Cancelled: a local clone still lands on disk, but nothing opens.
      if (!alive.current || controller.signal.aborted) return;
      rememberCloneParent(locationKey, folder);
      onOpen(key);
    } catch (reason) {
      if (alive.current && !controller.signal.aborted) setError(errorText(reason));
    } finally {
      if (alive.current) setBusy(false);
    }
  };

  const title = remote ? `Clone from ${ui.label} on a machine` : `Clone from ${ui.label}`;
  const input =
    "h-8 min-w-0 rounded-md border border-content/10 bg-content/3 px-2.5 text-[12px] text-content outline-none focus:border-content/25";
  const secondary =
    "shrink-0 rounded-md px-3 py-1.5 text-[12px] text-content/70 hover:bg-content/8 hover:text-content";

  return createPortal(
    <div className="fixed inset-0" style={{ zIndex: LAYER.dialog }}>
      <div className="absolute inset-0 z-0 bg-black/30" onMouseDown={cancel} />
      <form
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
        className="absolute z-[1] left-1/2 top-[14%] flex max-h-[74vh] w-[min(520px,calc(100vw-24px))] -translate-x-1/2 flex-col gap-3 rounded-lg border border-content/10 bg-background-base dark:bg-content/5 p-4 shadow-xl backdrop-blur-xl"
      >
        <div className="flex flex-col gap-1">
          <h2 className="flex items-center gap-2 text-[13px] font-medium leading-tight text-content">
            <InboxProviderMark provider={provider} className="size-3.5 shrink-0" />
            {title}
          </h2>
          <p className="text-[12px] leading-snug text-content/55">
            {remote
              ? `Opens the repository on that machine, cloning it with its ${ui.label} sign-in when it is not there yet.`
              : `Opens the repository if it is already checked out in the folder below, or clones it with your ${ui.label} sign-in.`}
          </p>
        </div>

        {remote && !machine ? (
          <p className="text-[12px] leading-snug text-content/55">
            No connected machine is signed in to {ui.label}. Run <code>gh auth login</code> on the machine, then try
            again.
          </p>
        ) : (
          <>
            {remote && machine ? (
              eligible.length > 1 ? (
                <SearchableSelect
                  label="Machine"
                  value={machine.id}
                  options={eligible.map((entry) => ({
                    value: entry.id,
                    label: entry.name,
                    keywords: entry.ssh?.target ?? entry.endpoint,
                  }))}
                  onChange={setMachineId}
                  searchable={false}
                />
              ) : (
                <p className="text-[12px] text-content/55">
                  On <span className="text-content/80">{machine.name}</span>
                </p>
              )
            ) : null}

            <input
              aria-label="Repository"
              autoFocus
              className={`${input} shrink-0 font-mono`}
              placeholder={ui.placeholder}
              value={query}
              spellCheck={false}
              autoCorrect="off"
              autoCapitalize="off"
              autoComplete="off"
              onChange={(event) => setQuery(event.target.value)}
            />
            <div
              aria-label="Repositories"
              role="listbox"
              className="min-h-24 flex-1 overflow-y-auto overscroll-contain rounded-md border border-content/10"
            >
              <div className="p-1">
                {matches.map((repo) => (
                  <RepoRow
                    key={repo.slug}
                    repo={repo}
                    selected={repo.slug === chosen}
                    onChoose={() => setQuery(repo.slug)}
                  />
                ))}
                {!repos && !reposError ? (
                  <p className="px-2 py-1.5 text-[12px] text-content/45">Loading repositories…</p>
                ) : null}
                {reposError ? (
                  <p className="px-2 py-1.5 text-[12px] text-red-400/90">{reposError}</p>
                ) : null}
                {repos && !matches.length ? (
                  <p className="px-2 py-1.5 text-[12px] text-content/45">
                    {slug ? `Will use ${slug}` : "No matching repositories. Type owner/name to use any repository."}
                  </p>
                ) : null}
              </div>
            </div>

            <div className="flex flex-col gap-1">
              <span className="text-[11px] text-content/50">Clone into</span>
              <div className="flex gap-2">
                <input
                  aria-label="Folder to clone into"
                  className={`${input} flex-1 font-mono`}
                  value={parent}
                  spellCheck={false}
                  autoCorrect="off"
                  autoCapitalize="off"
                  autoComplete="off"
                  onChange={(event) => setParent(event.target.value)}
                />
                <button
                  type="button"
                  className={secondary}
                  onClick={() => (remote ? void browse(parent.trim() || undefined) : void chooseLocalParent())}
                >
                  {remote ? "Browse" : "Choose…"}
                </button>
              </div>
            </div>
            {remote && browsing ? (
              <div
                aria-label="Folders"
                className="max-h-40 shrink-0 overflow-y-auto overscroll-contain rounded-md border border-content/10 p-1"
              >
                {browsing.parent ? <FolderRow name=".." onOpen={() => void browse(browsing.parent!)} /> : null}
                {browsing.entries.map((entry) => (
                  <FolderRow key={entry.path} name={entry.name} onOpen={() => void browse(entry.path)} />
                ))}
              </div>
            ) : null}

            <p aria-live="polite" className="min-h-4 truncate font-mono text-[11px] text-content/55">
              {plan
                ? `${plan.reuse ? "Opens existing" : "Clones into"} ${plan.path}`
                : planError}
            </p>

            {error ? (
              <p role="alert" className="whitespace-pre-wrap break-words text-[12px] leading-snug text-red-400/90">
                {error}
              </p>
            ) : null}
          </>
        )}

        <div className="flex justify-end gap-2">
          <button type="button" onClick={cancel} className={secondary}>
            Cancel
          </button>
          {!remote || machine ? (
            <button
              type="submit"
              disabled={busy || !chosen || !parent.trim() || !!planError}
              className="rounded-md bg-selection px-3 py-1.5 text-[12px] font-medium hover:bg-selection-hover disabled:opacity-40"
            >
              {busy ? (plan?.reuse ? "Opening…" : "Cloning…") : plan?.reuse ? "Open" : "Clone"}
            </button>
          ) : null}
        </div>
      </form>
    </div>,
    document.body,
  );
}

function RepoRow({
  repo,
  selected,
  onChoose,
}: {
  repo: GitHostRepo;
  selected: boolean;
  onChoose: () => void;
}) {
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      onClick={onChoose}
      className="flex w-full flex-col rounded-md px-2 py-1.5 text-left hover:bg-content/8 aria-selected:bg-content/8"
    >
      <span className="flex min-w-0 items-center gap-1.5 text-[12px] text-content/80">
        <span className="min-w-0 truncate">{repo.slug}</span>
        {repo.private ? <Lock className="size-3 shrink-0 text-content/40" strokeWidth={1.75} /> : null}
      </span>
      {repo.description ? (
        <span className="truncate text-[11px] text-content/45">{repo.description}</span>
      ) : null}
    </button>
  );
}
