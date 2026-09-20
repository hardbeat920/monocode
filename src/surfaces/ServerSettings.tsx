import { useEffect, useState } from "react";

import { SecondaryButton } from "../chrome/SecondaryButton";
import {
  Check,
  CircleAlert,
  Copy,
  Loader,
  Pencil,
  Plus,
  RefreshCw,
  Terminal,
  Trash2,
} from "../chrome/icons";
import {
  blankConnectionProfile,
  removeConnection,
  REMOTE_AGENT_INSTALLS,
  saveConnection,
  testConnection,
  type ConnectionProfile,
  type ConnectionTestResult,
} from "../lib/connections";
import { refreshConnections, useConnections } from "../lib/remote";

/**
 * Settings › Servers — manage the SSH servers (and server+container targets)
 * that remote projects connect to.
 */

type Draft = {
  id: string;
  name: string;
  host: string;
  user: string;
  port: string;
  container: string;
  authNote: string;
  /** Carried through on edits so `connections_save` keeps the stored value. */
  createdAt: number;
};

function draftFrom(profile: ConnectionProfile): Draft {
  return {
    id: profile.id,
    name: profile.name,
    host: profile.host,
    user: profile.user ?? "",
    port: profile.port != null ? String(profile.port) : "",
    container: profile.container ?? "",
    authNote: profile.authNote ?? "",
    createdAt: profile.createdAt,
  };
}

function draftToProfile(draft: Draft): ConnectionProfile {
  const port = Number.parseInt(draft.port, 10);
  return {
    id: draft.id,
    name: draft.name.trim(),
    host: draft.host.trim(),
    user: draft.user.trim() || null,
    port: Number.isFinite(port) && port > 0 && port < 65536 ? port : null,
    container: draft.container.trim() || null,
    authNote: draft.authNote.trim() || null,
    createdAt: draft.createdAt,
  };
}

function profileDetail(profile: ConnectionProfile): string {
  const dest = profile.user ? `${profile.user}@${profile.host}` : profile.host;
  const port = profile.port ? `:${profile.port}` : "";
  return profile.container
    ? `${dest}${port} · container ${profile.container}`
    : `${dest}${port}`;
}

export function ServersPage() {
  const profiles = useConnections();
  const [editing, setEditing] = useState<Draft | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);

  useEffect(() => {
    void refreshConnections();
  }, []);

  return (
    <>
      <section className="pt-8 first:pt-0" data-setting-id="servers">
        <div className="flex items-end gap-4 pb-2.5">
          <div className="min-w-0 flex-1">
            <h2 className="text-[13px] font-semibold text-content">Servers</h2>
            <p className="mt-1 max-w-xl text-[12px] leading-relaxed text-content/45">
              SSH servers and containers MonoCode can develop on. Agent
              sessions need key or ssh-agent login; password servers still
              work for terminals.
            </p>
          </div>
          {editing ? null : (
            <SecondaryButton
              onClick={() => setEditing(draftFrom(blankConnectionProfile()))}
            >
              <Plus className="h-3.5 w-3.5" aria-hidden />
              Add server
            </SecondaryButton>
          )}
        </div>
        <div className="overflow-hidden rounded-xl border border-content/10 bg-content/3">
          {editing ? (
            <ConnectionForm
              key={editing.id || "new"}
              draft={editing}
              onCancel={() => setEditing(null)}
              onSaved={() => setEditing(null)}
            />
          ) : profiles.length === 0 ? (
            <EmptyServers onAdd={() => setEditing(draftFrom(blankConnectionProfile()))} />
          ) : (
            profiles.map((profile, index) => (
              <ConnectionRow
                key={profile.id}
                profile={profile}
                first={index === 0}
                confirmRemove={removing === profile.id}
                onEdit={() => setEditing(draftFrom(profile))}
                onRequestRemove={() => setRemoving(profile.id)}
                onCancelRemove={() => setRemoving(null)}
                onConfirmRemove={() => {
                  void removeConnection(profile.id)
                    .then(() => refreshConnections())
                    .finally(() => setRemoving(null));
                }}
              />
            ))
          )}
        </div>
      </section>

      <section className="pt-8">
        <div className="pb-2.5">
          <h2 className="text-[13px] font-semibold text-content">
            Install agents on a server
          </h2>
          <p className="mt-1 max-w-xl text-[12px] leading-relaxed text-content/45">
            Remote sessions run the agent CLI on the server itself. Open a
            terminal on the remote project, install, then log in once —
            credentials stay on the server.
          </p>
        </div>
        <div className="overflow-hidden rounded-xl border border-content/10 bg-content/3">
          {REMOTE_AGENT_INSTALLS.map((install, index) => (
            <InstallRow key={install.agent} install={install} first={index === 0} />
          ))}
        </div>
      </section>
    </>
  );
}

function EmptyServers({ onAdd }: { onAdd: () => void }) {
  return (
    <div className="flex flex-col items-center gap-3 px-4 py-10 text-center">
      <Terminal className="h-5 w-5 text-content/30" aria-hidden />
      <p className="max-w-sm text-[12px] leading-relaxed text-content/45">
        No servers yet. Add one to develop on a remote machine, or point it at
        a Docker container running there.
      </p>
      <SecondaryButton onClick={onAdd}>
        <Plus className="h-3.5 w-3.5" aria-hidden />
        Add server
      </SecondaryButton>
    </div>
  );
}

function ConnectionRow({
  profile,
  first,
  confirmRemove,
  onEdit,
  onRequestRemove,
  onCancelRemove,
  onConfirmRemove,
}: {
  profile: ConnectionProfile;
  first: boolean;
  confirmRemove: boolean;
  onEdit: () => void;
  onRequestRemove: () => void;
  onCancelRemove: () => void;
  onConfirmRemove: () => void;
}) {
  const [result, setResult] = useState<ConnectionTestResult | null>(null);
  const [testing, setTesting] = useState(false);

  const onTest = () => {
    setTesting(true);
    setResult(null);
    void testConnection(profile)
      .then(setResult)
      .catch((error) => setResult({ ok: false, latencyMs: 0, error: String(error), home: null, uname: null }))
      .finally(() => setTesting(false));
  };

  return (
    <div
      className={`flex items-start gap-6 px-4 py-3.5 ${
        first ? "" : "border-t border-content/5"
      }`}
    >
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-[13px] font-medium text-content">
          <span className="truncate">{profile.name}</span>
          {profile.container ? (
            <span className="shrink-0 rounded border border-content/10 px-1.5 py-px text-[10px] uppercase tracking-wide text-content/45">
              container
            </span>
          ) : null}
        </div>
        <p className="mt-1 font-mono text-[12px] text-content/45">
          {profileDetail(profile)}
        </p>
        {profile.authNote ? (
          <p className="mt-1 text-[12px] text-content/40">{profile.authNote}</p>
        ) : null}
        {result ? <TestResult result={result} /> : null}
      </div>
      <div className="flex min-w-0 shrink-0 flex-wrap items-center justify-end gap-2">
        <SecondaryButton onClick={onTest} disabled={testing}>
          {testing ? (
            <Loader className="h-3.5 w-3.5 animate-spin" aria-hidden />
          ) : (
            <RefreshCw className="h-3.5 w-3.5" aria-hidden />
          )}
          Test
        </SecondaryButton>
        <SecondaryButton onClick={onEdit}>
          <Pencil className="h-3.5 w-3.5" aria-hidden />
          Edit
        </SecondaryButton>
        {confirmRemove ? (
          <>
            <SecondaryButton onClick={onCancelRemove}>Cancel</SecondaryButton>
            <SecondaryButton danger onClick={onConfirmRemove}>
              <Trash2 className="h-3.5 w-3.5" aria-hidden />
              Remove
            </SecondaryButton>
          </>
        ) : (
          <SecondaryButton danger onClick={onRequestRemove}>
            <Trash2 className="h-3.5 w-3.5" aria-hidden />
          </SecondaryButton>
        )}
      </div>
    </div>
  );
}

function TestResult({ result }: { result: ConnectionTestResult }) {
  if (!result.ok) {
    return (
      <p className="mt-2 flex items-start gap-1.5 text-[12px] leading-relaxed text-red-400">
        <CircleAlert className="mt-px h-3.5 w-3.5 shrink-0" aria-hidden />
        <span className="min-w-0 break-words">{result.error ?? "Connection failed"}</span>
      </p>
    );
  }
  return (
    <p className="mt-2 flex items-center gap-1.5 text-[12px] text-content/45">
      <Check className="h-3.5 w-3.5 shrink-0 text-green-500" aria-hidden />
      Connected{result.latencyMs ? ` in ${result.latencyMs} ms` : ""}
      {result.uname ? ` · ${result.uname}` : ""}
    </p>
  );
}

function ConnectionForm({
  draft,
  onCancel,
  onSaved,
}: {
  draft: Draft;
  onCancel: () => void;
  onSaved: () => void;
}) {
  const [form, setForm] = useState<Draft>(draft);
  const [result, setResult] = useState<ConnectionTestResult | null>(null);
  const [busy, setBusy] = useState<"test" | "save" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const profile = draftToProfile(form);
  const port = Number.parseInt(form.port, 10);
  const portError =
    form.port.length > 0 && (!Number.isFinite(port) || port < 1 || port > 65535)
      ? "Port must be between 1 and 65535."
      : null;
  const canSubmit =
    form.host.trim().length > 0 && portError === null && busy === null;

  const set = (patch: Partial<Draft>) => setForm((prev) => ({ ...prev, ...patch }));

  const onTest = () => {
    setBusy("test");
    setResult(null);
    setError(null);
    void testConnection(profile)
      .then(setResult)
      .catch((caught) => setError(String(caught)))
      .finally(() => setBusy(null));
  };

  const onSave = () => {
    setBusy("save");
    setResult(null);
    setError(null);
    void saveConnection(profile)
      .then(() => refreshConnections())
      .then(onSaved)
      .catch((caught) => setError(String(caught)))
      .finally(() => setBusy(null));
  };

  return (
    <div className="px-4 py-4">
      <div className="grid gap-3 @min-[560px]/settings:grid-cols-2">
        <Field label="Name">
          <TextInput
            value={form.name}
            placeholder={profile.container ? `${form.host} · ${form.container}` : form.host || "My server"}
            onChange={(name) => set({ name })}
          />
        </Field>
        <Field label="Host or ssh alias">
          <TextInput
            value={form.host}
            placeholder="build.example.com"
            onChange={(host) => set({ host })}
          />
        </Field>
        <Field label="User">
          <TextInput
            value={form.user}
            placeholder="ssh config default"
            onChange={(user) => set({ user })}
          />
        </Field>
        <Field label="Port" error={portError}>
          <TextInput
            value={form.port}
            placeholder="22"
            inputMode="numeric"
            onChange={(port) => set({ port: port.replace(/[^0-9]/g, "") })}
          />
        </Field>
        <Field label="Docker container (optional)" hint="Runs everything inside this container on the host.">
          <TextInput
            value={form.container}
            placeholder="my-app"
            onChange={(container) => set({ container })}
          />
        </Field>
        <Field label="Auth note (optional)" hint="Shown as a reminder, e.g. “key in agent”.">
          <TextInput
            value={form.authNote}
            onChange={(authNote) => set({ authNote })}
          />
        </Field>
      </div>
      {result ? (
        <div className="mt-3">
          <TestResult result={result} />
        </div>
      ) : null}
      {error ? (
        <p className="mt-3 text-[12px] leading-relaxed text-red-400">{error}</p>
      ) : null}
      <div className="mt-4 flex items-center justify-end gap-2">
        <SecondaryButton onClick={onCancel} disabled={busy !== null}>
          Cancel
        </SecondaryButton>
        <SecondaryButton onClick={onTest} disabled={!canSubmit}>
          {busy === "test" ? (
            <Loader className="h-3.5 w-3.5 animate-spin" aria-hidden />
          ) : (
            <RefreshCw className="h-3.5 w-3.5" aria-hidden />
          )}
          Test
        </SecondaryButton>
        <SecondaryButton onClick={onSave} disabled={!canSubmit}>
          {busy === "save" ? (
            <Loader className="h-3.5 w-3.5 animate-spin" aria-hidden />
          ) : (
            <Check className="h-3.5 w-3.5" aria-hidden />
          )}
          Save
        </SecondaryButton>
      </div>
    </div>
  );
}

function Field({
  label,
  hint,
  error,
  children,
}: {
  label: string;
  hint?: string;
  error?: string | null;
  children: React.ReactNode;
}) {
  return (
    <label className="flex min-w-0 flex-col gap-1.5">
      <span className="text-[12px] font-medium text-content/70">{label}</span>
      {children}
      {error ? <span className="text-[11px] text-red-400">{error}</span> : null}
      {hint ? <span className="text-[11px] text-content/40">{hint}</span> : null}
    </label>
  );
}

function TextInput({
  value,
  placeholder,
  inputMode,
  onChange,
}: {
  value: string;
  placeholder?: string;
  inputMode?: "text" | "numeric";
  onChange: (value: string) => void;
}) {
  return (
    <input
      type="text"
      value={value}
      placeholder={placeholder}
      inputMode={inputMode}
      onChange={(event) => onChange(event.target.value)}
      className="min-w-0 rounded-md border border-content/10 bg-content/5 px-2.5 py-1.5 font-mono text-[12px] text-content placeholder:font-sans placeholder:text-content/30 focus-visible:outline-2 focus-visible:outline-accent"
    />
  );
}

function InstallRow({
  install,
  first,
}: {
  install: { agent: string; command: string };
  first: boolean;
}) {
  const [copied, setCopied] = useState(false);

  const onCopy = () => {
    void navigator.clipboard
      ?.writeText(install.command)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => {});
  };

  return (
    <div
      className={`flex items-center gap-4 px-4 py-3 ${
        first ? "" : "border-t border-content/5"
      }`}
    >
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium text-content">{install.agent}</div>
        <code className="mt-1 block truncate font-mono text-[12px] text-content/45">
          {install.command}
        </code>
      </div>
      <SecondaryButton onClick={onCopy}>
        {copied ? (
          <Check className="h-3.5 w-3.5" aria-hidden />
        ) : (
          <Copy className="h-3.5 w-3.5" aria-hidden />
        )}
        {copied ? "Copied" : "Copy"}
      </SecondaryButton>
    </div>
  );
}
