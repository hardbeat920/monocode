import { useEffect, useMemo, useRef, useState } from "react";
import { Modal } from "./Modal";
import { Folder, Loader, RefreshCw } from "./icons";
import {
  blankConnectionProfile, browseRemoteDirectories, listRemoteContainers,
  saveConnection, sameServer, sshConfigHosts,
  type ConnectionProfile, type RemoteContainer, type RemoteDirectoryListing,
} from "../lib/connections";
import { parseRemotePath, refreshConnections, remoteProjectUri, useConnections } from "../lib/remote";
import { loadRecents } from "../lib/recents";

const field = "h-9 w-full rounded-md border border-content/15 bg-background-base px-2.5 text-[13px] outline-none focus:border-accent disabled:opacity-50";
const button = "flex h-8 items-center justify-center gap-1.5 rounded-md border border-content/10 px-2.5 text-[12px] text-content/70 hover:bg-content/10 disabled:opacity-40";
const parent = (path: string) => path.replace(/\/+$/, "").replace(/\/[^/]*$/, "") || "/";
const join = (path: string, name: string) => `${path.replace(/\/+$/, "")}/${name}`;
const CONTAINER_USAGE_KEY = "monocode.remoteContainerUsage";

function containerUsageKey(profile: ConnectionProfile, container: string): string {
  return `${profile.host}\u0000${profile.user ?? ""}\u0000${profile.port ?? ""}\u0000${container}`;
}

function readContainerUsage(): Record<string, number> {
  try {
    const raw = localStorage.getItem(CONTAINER_USAGE_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : null;
    if (!parsed || typeof parsed !== "object") return {};
    return Object.fromEntries(
      Object.entries(parsed).filter(([, value]) => typeof value === "number" && Number.isFinite(value)),
    );
  } catch {
    return {};
  }
}

function rememberContainerUse(profile: ConnectionProfile): void {
  if (!profile.container) return;
  try {
    const usage = readContainerUsage();
    const key = containerUsageKey(profile, profile.container);
    usage[key] = (usage[key] ?? 0) + 1;
    localStorage.setItem(CONTAINER_USAGE_KEY, JSON.stringify(usage));
  } catch {
    // private mode / unavailable storage
  }
}

function preferredContainer(
  server: ConnectionProfile,
  profiles: ConnectionProfile[],
  containers: RemoteContainer[] = [],
): string {
  const usage = readContainerUsage();
  const counts = new Map<string, number>();
  const recency = new Map<string, number>();
  const add = (container: string, amount: number) => {
    if (container) counts.set(container, (counts.get(container) ?? 0) + amount);
  };

  for (const profile of profiles) {
    if (sameServer(profile, server) && profile.container) {
      add(profile.container, 1);
      add(profile.container, usage[containerUsageKey(server, profile.container)] ?? 0);
    }
  }
  for (const [index, item] of loadRecents().entries()) {
    const parsed = parseRemotePath(item.path);
    const profile = parsed && profiles.find((candidate) => candidate.id === parsed.connectionId);
    if (profile && sameServer(profile, server) && profile.container) {
      add(profile.container, 1);
      if (!recency.has(profile.container)) recency.set(profile.container, index);
    }
  }

  const names = [...new Set([...counts.keys(), ...containers.map((container) => container.name)])];
  names.sort((a, b) => {
    const count = (counts.get(b) ?? 0) - (counts.get(a) ?? 0);
    if (count !== 0) return count;
    return (recency.get(a) ?? Number.MAX_SAFE_INTEGER) - (recency.get(b) ?? Number.MAX_SAFE_INTEGER);
  });
  return names[0] ?? "";
}

export function ConnectDialog({ onConnect, onClose }: {
  onConnect: (uri: string) => void; onClose: () => void;
}) {
  const profiles = useConnections();
  const [aliases, setAliases] = useState<string[]>([]);
  const [serversLoading, setServersLoading] = useState(true);
  const [discoveryError, setDiscoveryError] = useState("");
  const [serverKey, setServerKey] = useState("");
  const [manualHost, setManualHost] = useState("");
  const [manualUser, setManualUser] = useState("");
  const [manualPort, setManualPort] = useState("");
  const [container, setContainer] = useState("");
  const [containers, setContainers] = useState<RemoteContainer[]>([]);
  const [containersLoading, setContainersLoading] = useState(false);
  const [containerError, setContainerError] = useState("");
  const [reloadContainers, setReloadContainers] = useState(0);
  const [location, setLocation] = useState("/");
  const [pathInput, setPathInput] = useState("/");
  const [listing, setListing] = useState<RemoteDirectoryListing | null>(null);
  const [listingLoading, setListingLoading] = useState(false);
  const [directoryError, setDirectoryError] = useState("");
  const [reloadDirectory, setReloadDirectory] = useState(0);
  const [folderFilter, setFolderFilter] = useState("");
  const [showHidden, setShowHidden] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState("");
  const generation = useRef(0);
  const defaultedServer = useRef("");

  useEffect(() => {
    let active = true;
    void Promise.allSettled([refreshConnections(), sshConfigHosts()]).then(([saved, config]) => {
      if (!active) return;
      if (config.status === "fulfilled") setAliases(config.value);
      const errors = [saved, config].flatMap(result => result.status === "rejected" ? [String(result.reason)] : []);
      setDiscoveryError(errors.join(" · "));
      setServersLoading(false);
    });
    return () => { active = false; generation.current++; };
  }, []);

  const servers = useMemo(() => {
    const rows: { key: string; label: string; profile: ConnectionProfile }[] = [];
    for (const profile of profiles) {
      if (!rows.some(row => sameServer(row.profile, profile))) {
        const destination = `${profile.user ? `${profile.user}@` : ""}${profile.host}${profile.port ? `:${profile.port}` : ""}`;
        rows.push({ key: profile.id, label: destination, profile });
      }
    }
    for (const alias of aliases) {
      // Keep the alias rather than resolving HostName: ProxyJump, keys and
      // future config edits continue to be interpreted by system SSH.
      if (!rows.some(row => row.profile.host === alias)) {
        rows.push({ key: `ssh:${alias}`, label: `${alias} · SSH config`, profile: { ...blankConnectionProfile(), host: alias } });
      }
    }
    return rows;
  }, [profiles, aliases]);

  useEffect(() => {
    if (!serverKey && !serversLoading) {
      const first = servers[0];
      setServerKey(first?.key ?? "manual");
      const nextContainer = first ? preferredContainer(first.profile, profiles) : "";
      setContainer(nextContainer);
      defaultedServer.current = nextContainer ? first?.key ?? "" : "";
    }
  }, [profiles, servers, serverKey, serversLoading]);

  const server = useMemo(() => serverKey === "manual"
    ? { ...blankConnectionProfile(), host: manualHost.trim(), user: manualUser.trim() || null, port: manualPort ? Number(manualPort) : null }
    : servers.find(row => row.key === serverKey)?.profile ?? null,
  [serverKey, servers, manualHost, manualUser, manualPort]);
  const target = useMemo(() => server ? { ...server, container: container || null } : null, [server, container]);
  const validServer = !!server?.host && (!server.port || (Number.isInteger(server.port) && server.port > 0 && server.port < 65536));

  useEffect(() => {
    let active = true;
    setContainers([]); setContainerError("");
    if (!server || !validServer) { setContainersLoading(false); return; }
    setContainersLoading(true);
    const timer = setTimeout(() => {
      void listRemoteContainers(server).then(rows => {
        if (active) setContainers(rows);
      }, error => { if (active) setContainerError(String(error)); })
        .finally(() => { if (active) setContainersLoading(false); });
    }, serverKey === "manual" ? 500 : 0);
    return () => { active = false; clearTimeout(timer); };
  }, [server, validServer, reloadContainers, serverKey]);

  // Docker discovery is asynchronous. Once it arrives, fill an otherwise
  // empty environment with the most-used container for this server.
  useEffect(() => {
    if (!server || !validServer || containers.length === 0 || defaultedServer.current === serverKey) return;
    const nextContainer = preferredContainer(server, profiles, containers);
    if (!nextContainer) return;
    setContainer(nextContainer);
    defaultedServer.current = serverKey;
  }, [containers, profiles, server, serverKey, validServer]);

  useEffect(() => {
    let active = true;
    setListing(null); setDirectoryError(""); setSaveError("");
    if (!target || !validServer) { setListingLoading(false); return; }
    setListingLoading(true);
    const timer = setTimeout(() => {
      void browseRemoteDirectories(target, location).then(result => {
        if (!active) return;
        setListing(result); setPathInput(result.path);
      }, error => { if (active) setDirectoryError(String(error)); })
        .finally(() => { if (active) setListingLoading(false); });
    }, serverKey === "manual" ? 500 : 0);
    return () => { active = false; clearTimeout(timer); };
  }, [target, validServer, location, reloadDirectory, serverKey]);

  const resetDirectory = () => {
    generation.current++; setListing(null); setLocation("/"); setPathInput("/"); setFolderFilter(""); setSaveError("");
  };
  const navigate = (path: string) => {
    generation.current++; setListing(null); setLocation(path); setPathInput(path); setFolderFilter(""); setReloadDirectory(n => n + 1);
  };
  const recent = target ? loadRecents().flatMap(item => {
    const parsed = parseRemotePath(item.path);
    const profile = profiles.find(p => p.id === parsed?.connectionId);
    return parsed && profile && sameServer(profile, target) && profile.container === target.container ? [parsed.path] : [];
  }).slice(0, 4) : [];
  const directories = listing?.directories.filter(name => (showHidden || !name.startsWith(".")) && name.toLowerCase().includes(folderFilter.toLowerCase())) ?? [];

  const open = async () => {
    if (!target || !listing || saving || listingLoading) return;
    const epoch = generation.current;
    const chosenPath = listing.path;
    setSaving(true); setSaveError("");
    try {
      let profile = profiles.find(p => sameServer(p, target) && p.container === target.container);
      if (!profile) {
        profile = await saveConnection({ ...target, id: "", name: "", createdAt: 0 });
        await refreshConnections();
      }
      if (generation.current === epoch) {
        rememberContainerUse(target);
        onConnect(remoteProjectUri(profile.id, chosenPath));
      }
    } catch (error) { if (generation.current === epoch) setSaveError(String(error)); }
    finally { setSaving(false); }
  };

  return <Modal title="Open remote folder" size="md" onClose={onClose} className="max-h-[80vh]">
    <div className="flex flex-col gap-4 p-4">
      <fieldset disabled={saving} className="flex min-w-0 flex-col gap-4 disabled:opacity-60">
        <label className="flex flex-col gap-1.5 text-[12px] text-content/70">
          <span>1 · Server</span>
          <select aria-label="Server" className={field} value={serverKey} disabled={serversLoading} onChange={e => {
            resetDirectory();
            const row = servers.find(row => row.key === e.target.value);
            const nextContainer = row ? preferredContainer(row.profile, profiles) : "";
            setServerKey(e.target.value);
            setContainer(nextContainer);
            defaultedServer.current = nextContainer ? e.target.value : "";
          }}>
            <option value="" disabled>{serversLoading ? "Reading servers…" : "Choose a server"}</option>
            {servers.map(row => <option key={row.key} value={row.key}>{row.label}</option>)}
            <option value="manual">Other server…</option>
          </select>
        </label>
        {discoveryError && <p role="alert" className="text-[12px] text-red-400">{discoveryError}</p>}
        {serverKey === "manual" && <div className="grid grid-cols-2 gap-2">
          <input aria-label="Host or SSH alias" placeholder="Host or SSH alias" className={`${field} col-span-2`} value={manualHost} onChange={e => { resetDirectory(); setManualHost(e.target.value); setContainer(""); defaultedServer.current = ""; }} />
          <input aria-label="SSH user" placeholder="User (from SSH config)" className={field} value={manualUser} onChange={e => { resetDirectory(); setManualUser(e.target.value); setContainer(""); defaultedServer.current = ""; }} />
          <input aria-label="SSH port" placeholder="Port (from SSH config)" className={field} inputMode="numeric" value={manualPort} onChange={e => { resetDirectory(); setManualPort(e.target.value.replace(/\D/g, "")); setContainer(""); defaultedServer.current = ""; }} />
        </div>}
        <div className="flex flex-col gap-1.5 text-[12px] text-content/70">
          <label htmlFor="remote-environment">2 · Environment</label>
          <div className="flex gap-2">
            <select id="remote-environment" aria-label="Environment" className={`${field} min-w-0 flex-1`} value={container} disabled={!validServer} onChange={e => { resetDirectory(); setContainer(e.target.value); }}>
              <option value="">Server directly</option>
              {container && !containers.some(row => row.name === container) && <option value={container}>{container} · {containersLoading ? "Checking…" : "Saved container"}</option>}
              {containers.map(row => <option key={row.id} value={row.name}>{row.name} · {row.image} · {row.status}</option>)}
            </select>
            <button type="button" aria-label="Refresh containers" className={button} disabled={containersLoading || !validServer} onClick={() => setReloadContainers(n => n + 1)}>
              {containersLoading ? <Loader className="size-4 animate-spin" /> : <RefreshCw className="size-4" />}
            </button>
          </div>
          {containersLoading ? <p role="status">Loading running containers…</p> : containerError
            ? <p role="alert" className="text-red-400">Could not list containers. You can still browse the server. {containerError}</p>
            : validServer && <p className="text-content/45">{containers.length ? `${containers.length} running containers` : "No running containers. Browse the server directly."}</p>}
        </div>
        <section className="flex flex-col gap-2" aria-label="Project directory">
          <span className="text-[12px] text-content/70">3 · Project folder{container ? ` in ${container}` : ""}</span>
          {recent.length > 0 && <div className="flex flex-wrap items-center gap-1 text-[11px] text-content/50">
            <span>Recent</span>{recent.map(path => <button key={path} type="button" className="max-w-full truncate rounded px-2 py-1 text-accent hover:bg-content/5" title={path} onClick={() => navigate(path)}>{path}</button>)}
          </div>}
          <form className="flex gap-2" onSubmit={e => { e.preventDefault(); navigate(pathInput.trim()); }}>
            <input aria-label="Remote project path" className={`${field} min-w-0 flex-1 font-mono`} placeholder="Home directory or /path/to/project" value={pathInput} disabled={!validServer} spellCheck={false} onChange={e => setPathInput(e.target.value)} />
            <button className={button} disabled={!validServer} type="submit">Go</button>
          </form>
          <div className="overflow-hidden rounded-lg border border-content/10">
            <div className="flex items-center gap-2 border-b border-content/10 px-2 py-1.5">
              <button className={button} type="button" disabled={!validServer} onClick={() => navigate("")}>Home</button>
              <button className={button} type="button" disabled={!listing || listing.path === "/"} onClick={() => listing && navigate(parent(listing.path))}>Up</button>
              <input aria-label="Filter folders" placeholder="Filter folders…" className="h-8 min-w-0 flex-1 bg-transparent px-1 text-[12px] outline-none" value={folderFilter} onChange={e => setFolderFilter(e.target.value)} />
              <label className="flex gap-1 text-[11px] text-content/50"><input type="checkbox" checked={showHidden} onChange={e => setShowHidden(e.target.checked)} />Hidden</label>
            </div>
            <div className="h-44 overflow-y-auto" aria-busy={listingLoading}>
              {listingLoading ? <p role="status" className="flex items-center justify-center gap-2 p-6 text-[12px] text-content/50"><Loader className="size-4 animate-spin" />Loading folders…</p>
                : directoryError ? <div className="p-3"><p role="alert" className="break-words text-[12px] text-red-400">{directoryError}</p><button className={`${button} mt-2`} onClick={() => setReloadDirectory(n => n + 1)}>Retry</button></div>
                : !listing ? <p className="p-4 text-[12px] text-content/45">Choose a server to browse folders.</p>
                : directories.length === 0 ? <p className="p-4 text-[12px] text-content/45">{folderFilter ? "No matching folders." : "No folders here. You can open this folder."}</p>
                : directories.map(name => <button type="button" key={name} aria-label={`Browse ${name}`} className="flex w-full items-center gap-2 px-3 py-2 text-left text-[13px] text-content/80 hover:bg-content/8 focus-visible:bg-content/10 focus-visible:outline-none" onClick={() => navigate(join(listing!.path, name))}><Folder className="size-4 shrink-0 text-content/45" /><span className="truncate">{name}</span><span className="ml-auto text-content/30">›</span></button>)}
            </div>
          </div>
        </section>
      </fieldset>
      {saveError && <p role="alert" className="text-[12px] text-red-400">{saveError}</p>}
      <div className="flex items-center justify-end gap-2">
        <button type="button" className={button} onClick={onClose}>Cancel</button>
        <button type="button" className="flex h-9 items-center gap-2 rounded-md bg-accent px-4 text-[12px] font-medium text-white disabled:opacity-40" disabled={saving || listingLoading || !listing || pathInput !== listing.path} onClick={() => void open()}>
          {saving && <Loader className="size-4 animate-spin" />}{saving ? "Connecting…" : "Open folder"}
        </button>
      </div>
    </div>
  </Modal>;
}
