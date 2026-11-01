import Storage from "expo-sqlite/kv-store";

/**
 * Where the user last was. Only a project is remembered now; a Mono is never
 * the home screen. An older phone stored `kind: "mono"`, which still reads
 * so the app can fall back from it (see homeProjectId).
 */
export type Place = { kind: "project" | "mono"; id: string };

// Per desktop, since each has its own projects and monos.
const storageKey = (desktop: string) => `monocode.lastPlace.${desktop}`;
/** The last place written, so revisiting it writes nothing. */
const written = new Map<string, string>();

export function parsePlace(raw: string | null | undefined): Place | null {
  try {
    const place = raw ? (JSON.parse(raw) as Partial<Place>) : null;
    return place &&
      (place.kind === "project" || place.kind === "mono") &&
      typeof place.id === "string"
      ? { kind: place.kind, id: place.id }
      : null;
  } catch {
    return null;
  }
}

export function readLastPlace(desktop: string | undefined): Place | null {
  if (!desktop) return null;
  try {
    return parsePlace(Storage.getItemSync(storageKey(desktop)));
  } catch {
    return null;
  }
}

/**
 * The project the app opens on: the last one visited while it is still on the
 * phone. From an old Mono place, that Mono's first project on the phone;
 * otherwise the first project. Never a Mono; undefined with no projects.
 */
export function homeProjectId(
  projects: readonly { id: string }[],
  monos: readonly { id: string; projectIds: string[] }[],
  last: Place | null,
): string | undefined {
  const has = (id: string) => projects.some((project) => project.id === id);
  if (last?.kind === "project" && has(last.id)) return last.id;
  if (last?.kind === "mono") {
    const mono = monos.find((entry) => entry.id === last.id);
    const own = mono?.projectIds.find(has);
    if (own) return own;
  }
  return projects[0]?.id;
}

export function rememberPlace(
  desktop: string | undefined,
  place: { kind: "project"; id: string },
) {
  if (!desktop) return;
  const value = JSON.stringify(place);
  if (written.get(desktop) === value) return;
  written.set(desktop, value);
  void Storage.setItem(storageKey(desktop), value);
}
