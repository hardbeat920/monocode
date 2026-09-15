import { play, setEnabled, setVolume, type SoundName } from "cuelume";
import type { LinkedWorkItemUpdateCard } from "./linkedWorkItemActivity";

const KEY = "monocode.sounds";

export const SOUNDS_DEFAULT = true;

/** Soft enough to sit in the background while a turn runs in another app. */
export const SOUNDS_VOLUME = 0.55;

export const SOUNDS_CHANGE_EVENT = "monocode:sounds-change";

export type SoundCue =
  | "turnFinished"
  | "inboxUnseen"
  | "linkedActivity"
  | "updateAvailable"
  | "switch"
  | "copy";

const CUES: Record<SoundCue, SoundName> = {
  turnFinished: "success",
  inboxUnseen: "bloom",
  linkedActivity: "chime",
  updateAvailable: "arrival",
  switch: "toggle",
  copy: "scan",
};

export function loadSoundsEnabled(): boolean {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw == null) return SOUNDS_DEFAULT;
    return raw === "1" || raw === "true";
  } catch {
    return SOUNDS_DEFAULT;
  }
}

export function saveSoundsEnabled(value: boolean) {
  try {
    localStorage.setItem(KEY, value ? "1" : "0");
  } catch {
    // private mode / quota
  }
  applySoundEngine();
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<boolean>(SOUNDS_CHANGE_EVENT, { detail: value }),
  );
}

function applySoundEngine() {
  setEnabled(loadSoundsEnabled());
  setVolume(SOUNDS_VOLUME);
}

/** Apply the stored mute/volume before the first cue. */
export function initSounds() {
  applySoundEngine();
}

export function playCue(cue: SoundCue) {
  if (!loadSoundsEnabled()) return;
  applySoundEngine();
  play(CUES[cue]);
}

let inboxDotOn = false;
let inboxPrimed = false;
let announcedUpdate: string | undefined;
const announcedLinkedActivities = new Set<string>();

/** Remember each session's activity across notice unmounts when switching tabs. */
export function announceLinkedActivity(
  sessionId: string,
  card: LinkedWorkItemUpdateCard | undefined,
) {
  if (!card || card.status !== "ready") return;
  const key = JSON.stringify([
    sessionId,
    card.kind,
    card.repo.toLowerCase(),
    card.number,
    card.updatedAt,
  ]);
  if (announcedLinkedActivities.has(key)) return;
  announcedLinkedActivities.add(key);
  playCue("linkedActivity");
}

/**
 * Rising edge of the project-rail inbox dot, after the first snapshot.
 * Launching with items already unseen must not chime.
 */
export function noteInboxUnseen(isUnseen: boolean) {
  if (isUnseen && !inboxDotOn && inboxPrimed) playCue("inboxUnseen");
  inboxDotOn = isUnseen;
  inboxPrimed = true;
}

/** One cue per available version, including a later probe of the same build. */
export function announceUpdateAvailable(version: string | null) {
  if (!version) {
    announcedUpdate = undefined;
    return;
  }
  if (announcedUpdate === version) return;
  announcedUpdate = version;
  playCue("updateAvailable");
}

/** Test helper: forget which notification cues already fired. */
export function resetSoundCues() {
  inboxDotOn = false;
  inboxPrimed = false;
  announcedUpdate = undefined;
  announcedLinkedActivities.clear();
}
