import { useState } from "react";
import type {
  ModManifest,
  UserConfigField,
  UserConfigValue,
  UserConfigValues,
} from "../api/types";
import { useModHost } from "../runtime/host";
import { pickFolders } from "../../../platform/tauri/fs";
import { Group, Row } from "../../../shared/ui/SettingsRow";
import { Toggle } from "../../../shared/ui/Toggle";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { SearchableSelect } from "../../../shared/ui/SearchableSelect";
import { Modal } from "../../../shared/ui/Modal";
import { Badge } from "../../../shared/ui/Badge";
import { FolderOpen, RefreshCw } from "../../../shared/ui/icons";

type ModEntry = ReturnType<ReturnType<typeof useModHost>["mods"]>[number];

/** Programs that run any code they are given, so allowing one is full access. */
const SHELLS = new Set([
  "sh", "bash", "zsh", "fish", "dash", "ksh", "csh", "tcsh", "rm", "sudo",
  "env", "xargs", "find", "osascript", "node", "deno", "bun", "npx", "npm",
  "python", "python3", "perl", "ruby", "php", "lua",
  // `-c alias.x=!cmd`, `alias set --shell` and friends run anything.
  "git", "gh",
]);

/** What a mod may touch, in the words the consent sheet uses. */
export function permissionLines(
  manifest: ModManifest,
): { text: string; danger?: boolean }[] {
  const permissions = manifest.permissions ?? {};
  const lines: { text: string; danger?: boolean }[] = [];
  const process = permissions.process ?? [];
  if (process.some((name) => SHELLS.has(name))) {
    lines.push({ text: "Full shell access: it can run any command as you", danger: true });
  } else if (process.length) {
    lines.push({ text: `Run programs, with any arguments: ${process.join(", ")}` });
  }
  if (permissions.fs?.read?.length) {
    lines.push({ text: `Read files: ${permissions.fs.read.join(", ")}` });
  }
  if (permissions.http?.length) {
    lines.push({ text: `Connect to: ${permissions.http.join(", ")}` });
  }
  if (permissions.env?.length) {
    lines.push({ text: `Read environment variables: ${permissions.env.join(", ")}` });
  }
  return lines;
}

const SOURCE_LABELS: Record<ModEntry["source"], string> = {
  project: "Project",
  user: "User",
  unpacked: "Unpacked",
};

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function ModsSettings() {
  const host = useModHost();
  const mods = host.mods();
  const [consent, setConsent] = useState<ModManifest | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = (action: Promise<void>) => {
    setError(null);
    void action.catch((failure: unknown) => setError(message(failure)));
  };
  const loadUnpacked = () =>
    run(
      pickFolders("Load unpacked mod").then((paths) =>
        paths[0] ? host.loadUnpacked(paths[0]) : undefined,
      ),
    );

  return (
    <>
      {error ? (
        <p role="alert" className="pb-4 text-[12px] leading-5 text-red-400">
          {error}
        </p>
      ) : null}
      <Group
        title="Installed mods"
        description="Mods come from .monocode/mods in a project, ~/.monocode/mods, or a folder you load. Each one runs isolated and asks before it gets any access."
        action={
          <SecondaryButton onClick={loadUnpacked}>
            <FolderOpen className="size-3.5" strokeWidth={1.75} />
            Load unpacked…
          </SecondaryButton>
        }
      >
        {mods.length ? (
          mods.map((mod) => (
            <ModRow
              key={mod.manifest.name}
              mod={mod}
              onEnable={() => setConsent(mod.manifest)}
              onDisable={() => run(host.setEnabled(mod.manifest.name, false))}
              onReload={() => run(host.reload(mod.manifest.name))}
            />
          ))
        ) : (
          <p className="px-4 py-3.5 text-[12px] text-content/45">
            No mods yet.
          </p>
        )}
      </Group>
      {mods
        .filter((mod) => mod.enabled && mod.manifest.userConfig)
        .map((mod) => (
          <ModConfigGroup
            key={mod.manifest.name}
            mod={mod}
            onSave={(values) => run(host.setConfig(mod.manifest.name, values))}
          />
        ))}
      {consent ? (
        <ConsentSheet
          manifest={consent}
          onClose={() => setConsent(null)}
          onAllow={() => {
            setConsent(null);
            run(host.setEnabled(consent.name, true));
          }}
        />
      ) : null}
    </>
  );
}

function ModRow({
  mod,
  onEnable,
  onDisable,
  onReload,
}: {
  mod: ModEntry;
  onEnable: () => void;
  onDisable: () => void;
  onReload: () => void;
}) {
  const { manifest } = mod;
  return (
    <Row
      label={
        <span className="flex min-w-0 items-center gap-2">
          <span className="truncate">{manifest.name}</span>
          <span className="shrink-0 text-[12px] font-normal text-content/40">
            {manifest.version}
          </span>
          {mod.crashed ? (
            <Badge className="bg-red-500/12 text-red-400">Crashed</Badge>
          ) : mod.error ? (
            <Badge className="bg-red-500/12 text-red-400">Error</Badge>
          ) : null}
        </span>
      }
      description={
        <>
          {mod.error ? null : manifest.description}
          <span className="block truncate" title={mod.dir}>
            {SOURCE_LABELS[mod.source]} · {mod.dir}
          </span>
          {mod.shadows.length ? (
            <span className="block text-red-400">
              Hides another mod with this name: {mod.shadows.join(", ")}
            </span>
          ) : null}
        </>
      }
    >
      {mod.error ? (
        <p className="w-full text-left text-[12px] leading-5 text-red-400">
          {mod.error}
        </p>
      ) : null}
      {mod.enabled ? (
        <button
          type="button"
          title="Reload"
          aria-label={`Reload ${manifest.name}`}
          onClick={onReload}
          className="grid size-7 place-items-center rounded-md text-content/40 transition-transform duration-150 hover:bg-content/10 hover:text-content active:scale-[0.96]"
        >
          <RefreshCw className="size-3.5" strokeWidth={1.75} />
        </button>
      ) : null}
      <Toggle
        label={`Enable ${manifest.name}`}
        on={mod.enabled}
        onChange={(on) => (on ? onEnable() : onDisable())}
      />
    </Row>
  );
}

function ConsentSheet({
  manifest,
  onClose,
  onAllow,
}: {
  manifest: ModManifest;
  onClose: () => void;
  onAllow: () => void;
}) {
  const lines = permissionLines(manifest);
  return (
    <Modal title={`Enable ${manifest.name}?`} size="sm" onClose={onClose}>
      <div className="flex flex-col gap-3 px-4 pb-4 pt-3">
        <p className="text-[13px] leading-5 text-content/70">
          Every mod can see your sessions' tool calls and their output.{" "}
          {lines.length
            ? "This mod also asks for:"
            : "This mod asks for nothing more."}
        </p>
        {lines.length ? (
          <ul className="flex flex-col gap-1.5 text-[12px] leading-5">
            {lines.map((line) => (
              <li
                key={line.text}
                className={`break-words ${line.danger ? "text-red-400" : "text-content/70"}`}
              >
                {line.text}
              </li>
            ))}
          </ul>
        ) : null}
        <p className="text-[12px] leading-5 text-content/50">
          If its manifest changes, MonoCode asks again.
        </p>
        <div className="flex justify-end gap-2 pt-1">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-[12px] text-content/70 hover:bg-content/8"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onAllow}
            className="rounded-md bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:brightness-110"
          >
            Enable
          </button>
        </div>
      </div>
    </Modal>
  );
}

function ModConfigGroup({
  mod,
  onSave,
}: {
  mod: ModEntry;
  onSave: (values: UserConfigValues) => void;
}) {
  const [values, setValues] = useState(() => mod.config);
  const set = (key: string, value: UserConfigValue) => {
    const next = { ...values, [key]: value };
    setValues(next);
    onSave(next);
  };
  return (
    <Group title={`${mod.manifest.name} settings`}>
      {Object.entries(mod.manifest.userConfig ?? {}).map(([key, field]) => (
        <Row key={key} label={field.title} description={field.description}>
          <ConfigControl
            field={field}
            value={values[key]}
            onChange={(value) => set(key, value)}
          />
        </Row>
      ))}
    </Group>
  );
}

function ConfigControl({
  field,
  value,
  onChange,
}: {
  field: UserConfigField;
  value: UserConfigValue | undefined;
  onChange: (value: UserConfigValue) => void;
}) {
  if (field.type === "boolean") {
    return <Toggle label={field.title} on={value === true} onChange={onChange} />;
  }
  if (field.type === "string" && field.options?.length) {
    return (
      <SearchableSelect
        label={field.title}
        value={typeof value === "string" ? value : ""}
        options={field.options.map((option) => ({ value: option, label: option }))}
        searchable={false}
        onChange={onChange}
      />
    );
  }
  return (
    <ConfigInput
      field={field}
      value={value === undefined ? "" : String(value)}
      onCommit={(raw) => {
        if (field.type !== "number") return onChange(raw);
        const number = Number(raw);
        if (raw.trim() && Number.isFinite(number)) onChange(number);
      }}
    />
  );
}

/** Saves on blur or Enter, so typing does not reload the mod per keystroke. */
function ConfigInput({
  field,
  value,
  onCommit,
}: {
  field: UserConfigField;
  value: string;
  onCommit: (raw: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  const commit = () => {
    if (draft !== value) onCommit(draft);
  };
  return (
    <label className="flex h-7 w-52 max-w-full shrink-0 items-center rounded-md border border-content/10 px-2 focus-within:border-content/20">
      <input
        aria-label={field.title}
        inputMode={field.type === "number" ? "decimal" : undefined}
        value={draft}
        spellCheck={false}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") commit();
        }}
        className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
      />
    </label>
  );
}
