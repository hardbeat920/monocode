import {
  closeSync,
  openSync,
  readdirSync,
  readFileSync,
  readSync,
  realpathSync,
  statSync,
} from "node:fs";
import { dirname, isAbsolute, join, relative } from "node:path";

// Mirrors src-tauri/src/skills.rs, so a project on a connected machine lists
// the skills installed there, as a local project lists this computer's.

export type HostSkill = {
  name: string;
  description: string;
  path: string;
  scope: "project" | "user";
  source: string;
};

const MAX_SKILLS = 300;
const MAX_FRONTMATTER_BYTES = 16 * 1024;

const NATIVE_ROOTS: [string, string][] = [
  [".claude/skills", "claude"],
  [".cursor/skills", "cursor"],
  [".codex/skills", "codex"],
  [".opencode/skills", "opencode"],
  [".pi/skills", "pi"],
  [".omp/skills", "omp"],
  [".fx/skills", "fx"],
  [".grok/skills", "grok"],
  [".hermes/skills", "hermes"],
];

/**
 * Skills visible for a project: `.agents/skills` first, then native harness
 * folders, then Claude plugins. The same name from an earlier root wins.
 * Disabled paths are skipped before that, so an enabled skill of the same
 * name from a later root still appears.
 */
export function listHostSkills(
  project: string,
  home: string | undefined,
  disabledPaths?: readonly string[] | null,
  managedRoot = managedSettingsRoot(),
): HostSkill[] {
  const disabled = disabledFilter(disabledPaths);
  const byName = new Map<string, HostSkill>();
  const seenRoots = new Set<string>();
  const addRoot = (
    root: string,
    scope: HostSkill["scope"],
    source: string,
    namespace?: string,
  ) => {
    if (byName.size >= MAX_SKILLS) return;
    if (!namespace) {
      const key = canonical(root) ?? root;
      if (seenRoots.has(key)) return;
      seenRoots.add(key);
    }
    for (const skill of scanRoot(root, scope, source)) {
      if (disabled?.(skill.path)) continue;
      if (byName.size >= MAX_SKILLS) break;
      const name = namespace ? `${namespace}:${skill.name}` : skill.name;
      if (!byName.has(name)) byName.set(name, { ...skill, name });
    }
  };

  addRoot(join(project, ".agents/skills"), "project", "agents");
  if (home) addRoot(join(home, ".agents/skills"), "user", "agents");
  for (const [dir, source] of NATIVE_ROOTS) {
    addRoot(join(project, dir), "project", source);
    if (home) addRoot(join(home, dir), "user", source);
  }
  if (home) {
    addRoot(join(home, ".pi/agent/skills"), "user", "pi");
    addRoot(join(home, ".omp/agent/skills"), "user", "omp");
    // Later providers come after every established root, so a skill of the
    // same name never shadows one of those.
    const antigravity = join(home, ".gemini/antigravity/skills");
    if (isDirectory(antigravity)) addRoot(antigravity, "user", "antigravity");
    addRoot(join(project, ".factory/skills"), "project", "droid");
    addRoot(join(home, ".factory/skills"), "user", "droid");
    for (const plugin of claudePluginSkillRoots(home, project, managedRoot))
      addRoot(plugin.root, plugin.scope, "claude", plugin.namespace);
  }
  return [...byName.values()].sort((a, b) =>
    a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
  );
}

function disabledFilter(paths?: readonly string[] | null) {
  if (!paths?.length) return undefined;
  const normalized = new Set(paths.map(normalizePath));
  const resolved = new Set(
    paths.flatMap((path) => {
      const real = canonical(path);
      return real ? [real] : [];
    }),
  );
  return (path: string) => {
    if (normalized.has(normalizePath(path))) return true;
    const real = resolved.size ? canonical(path) : undefined;
    return !!real && resolved.has(real);
  };
}

function normalizePath(path: string): string {
  if (process.platform !== "win32") return path;
  return path
    .replace(/\\/g, "/")
    .replace(/^\/\/\?\//, "")
    .replace(/\/{2,}/g, "/")
    .toLowerCase();
}

function canonical(path: string): string | undefined {
  try {
    return realpathSync(path);
  } catch {
    return undefined;
  }
}

function isDirectory(path: string): boolean {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
}

function isFile(path: string): boolean {
  try {
    return statSync(path).isFile();
  } catch {
    return false;
  }
}

function readJson(path: string): unknown {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return undefined;
  }
}

function claudePluginSkillRoots(
  home: string,
  project: string,
  managedRoot: string | undefined,
): { root: string; scope: HostSkill["scope"]; namespace: string }[] {
  const registry = readJson(join(home, ".claude/plugins/installed_plugins.json"));
  const plugins =
    registry && typeof registry === "object"
      ? (registry as { plugins?: unknown }).plugins
      : undefined;
  if (!plugins || typeof plugins !== "object" || Array.isArray(plugins))
    return [];
  const roots: { root: string; scope: HostSkill["scope"]; namespace: string }[] =
    [];
  for (const [pluginId, installed] of Object.entries(plugins)) {
    if (!claudePluginEnabled(home, project, pluginId, managedRoot)) continue;
    const at = pluginId.lastIndexOf("@");
    const namespace = at >= 0 ? pluginId.slice(0, at) : pluginId;
    if (!isValidSkillName(namespace)) continue;
    const entries = Array.isArray(installed)
      ? installed
      : installed && typeof installed === "object"
        ? [installed]
        : [];
    for (const entry of entries as Record<string, unknown>[]) {
      if (typeof entry?.installPath !== "string") continue;
      let scope: HostSkill["scope"];
      if (entry.scope === "project" || entry.scope === "local") {
        if (typeof entry.projectPath !== "string") continue;
        if (!pathIsWithin(project, resolveHomePath(entry.projectPath, home)))
          continue;
        scope = "project";
      } else if (entry.scope === "user" || entry.scope === undefined) {
        scope = "user";
      } else continue;
      roots.push({
        root: join(resolveHomePath(entry.installPath, home), "skills"),
        scope,
        namespace,
      });
    }
  }
  // Stable, so plugins keep their registry order within each scope.
  return roots.sort(
    (a, b) => (a.scope === "project" ? 0 : 1) - (b.scope === "project" ? 0 : 1),
  );
}

function resolveHomePath(raw: string, home: string): string {
  if (raw === "~") return home;
  if (raw.startsWith("~/")) return join(home, raw.slice(2));
  return isAbsolute(raw) ? raw : join(home, raw);
}

function claudePluginEnabled(
  home: string,
  project: string,
  pluginId: string,
  managedRoot: string | undefined,
): boolean {
  const managed = managedRoot
    ? managedPluginSetting(managedRoot, pluginId)
    : undefined;
  if (managed !== undefined) return managed;
  const projectRoot = claudeSettingsProjectRoot(project);
  for (const settings of [
    join(projectRoot, ".claude/settings.local.json"),
    join(projectRoot, ".claude/settings.json"),
    join(home, ".claude/settings.json"),
  ]) {
    const enabled = pluginSetting(settings, pluginId);
    if (enabled !== undefined) return enabled;
  }
  return true;
}

function claudeSettingsProjectRoot(project: string): string {
  for (let candidate = project; ; candidate = dirname(candidate)) {
    if (
      isFile(join(candidate, ".claude/settings.local.json")) ||
      isFile(join(candidate, ".claude/settings.json"))
    )
      return candidate;
    if (dirname(candidate) === candidate) return project;
  }
}

function pluginSetting(path: string, pluginId: string): boolean | undefined {
  const value = readJson(path) as
    | { enabledPlugins?: Record<string, unknown> }
    | undefined;
  const enabled = value?.enabledPlugins?.[pluginId];
  return typeof enabled === "boolean" ? enabled : undefined;
}

function managedPluginSetting(
  root: string,
  pluginId: string,
): boolean | undefined {
  let value = pluginSetting(join(root, "managed-settings.json"), pluginId);
  let files: string[] = [];
  try {
    files = readdirSync(join(root, "managed-settings.d"))
      .filter((name) => name.endsWith(".json") && !name.startsWith("."))
      .sort();
  } catch {
    /* no drop-in folder */
  }
  for (const file of files) {
    const enabled = pluginSetting(join(root, "managed-settings.d", file), pluginId);
    if (enabled !== undefined) value = enabled;
  }
  return value;
}

function managedSettingsRoot(): string | undefined {
  if (process.platform === "darwin")
    return "/Library/Application Support/ClaudeCode";
  if (process.platform === "linux") return "/etc/claude-code";
  if (process.platform === "win32") return "C:\\Program Files\\ClaudeCode";
  return undefined;
}

function pathIsWithin(path: string, root: string): boolean {
  const child = canonical(path);
  const parent = canonical(root);
  if (!child || !parent) return false;
  const rest = relative(parent, child);
  return rest === "" || (!rest.startsWith("..") && !isAbsolute(rest));
}

function scanRoot(
  root: string,
  scope: HostSkill["scope"],
  source: string,
): HostSkill[] {
  let folders: string[];
  try {
    folders = readdirSync(root);
  } catch {
    return [];
  }
  const out: HostSkill[] = [];
  for (const folder of folders) {
    if (folder.startsWith(".") || folder === "skills-cursor") continue;
    const dir = join(root, folder);
    if (!isDirectory(dir)) continue;
    const file = [join(dir, "SKILL.md"), join(dir, "skill.md")].find(isFile);
    if (!file) continue;
    const text = readPrefix(file, MAX_FRONTMATTER_BYTES);
    if (text === undefined) continue;
    const fallback = slugName(folder);
    if (!fallback) continue;
    const { name, description } = parseFrontmatter(text, fallback);
    if (!name) continue;
    out.push({ name, description, path: file, scope, source });
  }
  return out;
}

function readPrefix(path: string, max: number): string | undefined {
  let fd: number | undefined;
  try {
    fd = openSync(path, "r");
    const buffer = Buffer.alloc(max);
    const length = readSync(fd, buffer, 0, max, 0);
    // Invalid UTF-8 skips the file, as the desktop does.
    return new TextDecoder("utf-8", { fatal: true }).decode(
      buffer.subarray(0, length),
    );
  } catch {
    return undefined;
  } finally {
    if (fd !== undefined) closeSync(fd);
  }
}

export function parseFrontmatter(
  text: string,
  fallback: string,
): { name: string; description: string } {
  const trimmed = text.replace(/^\uFEFF+/, "");
  if (!trimmed.startsWith("---")) return { name: fallback, description: "" };
  let rest = trimmed.slice(3);
  if (rest.startsWith("\r")) rest = rest.slice(1);
  if (rest.startsWith("\n")) rest = rest.slice(1);
  const end = rest.indexOf("\n---");
  const yaml = end >= 0 ? rest.slice(0, end) : rest;

  let name: string | undefined;
  let description = "";
  let inDescription = false;
  let folded = false;
  for (const raw of yaml.split(/\r?\n/)) {
    if (inDescription) {
      if (raw.startsWith(" ") || raw.startsWith("\t")) {
        const piece = raw.trim();
        if (!piece) continue;
        if (description) description += folded ? " " : "\n";
        description += piece;
        continue;
      }
      inDescription = false;
    }
    const line = raw.trimEnd().trimStart();
    if (line.startsWith("name:")) {
      name = unquote(line.slice("name:".length));
    } else if (line.startsWith("description:")) {
      const value = line.slice("description:".length).trim();
      if (value.startsWith(">") || value.startsWith("|")) {
        inDescription = true;
        folded = value.startsWith(">");
        description = "";
      } else {
        description = unquote(value);
      }
    }
  }
  return {
    name: name && isValidSkillName(name) ? name : fallback,
    description: description.trim(),
  };
}

function unquote(value: string): string {
  const text = value.trim();
  if (
    text.length >= 2 &&
    ((text.startsWith('"') && text.endsWith('"')) ||
      (text.startsWith("'") && text.endsWith("'")))
  )
    return text.slice(1, -1);
  return text;
}

function isValidSkillName(name: string): boolean {
  return name.length > 0 && name.length <= 64 && /^[a-z0-9]+(-[a-z0-9]+)*$/.test(name);
}

function slugName(raw: string): string {
  let out = raw
    .replace(/[^A-Za-z0-9]+/g, "-")
    .replace(/^-+/, "")
    .toLowerCase();
  out = out.replace(/-+$/, "");
  if (out.length > 64) out = out.slice(0, 64).replace(/-+$/, "");
  return out;
}
