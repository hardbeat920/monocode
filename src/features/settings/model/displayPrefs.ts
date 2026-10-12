import { useSyncExternalStore } from "react";
import { readFlag, writeFlag } from "./storageFlags";

const SHOW_REMAINING_USAGE_KEY = "monocode.showRemainingUsage";
const MASK_EMAILS_KEY = "monocode.maskEmails";
const COMPOSER_AUTOCORRECT_KEY = "monocode.composerAutocorrect";
const RAIL_MONOS_PINNED_KEY = "monocode.railMonosPinned";

export const SHOW_REMAINING_USAGE_DEFAULT = false;
export const MASK_EMAILS_DEFAULT = true;
export const COMPOSER_AUTOCORRECT_DEFAULT = true;
export const RAIL_MONOS_PINNED_DEFAULT = false;

/** Fired on `window` whenever the usage meter direction flips (detail: boolean). */
export const SHOW_REMAINING_USAGE_CHANGE_EVENT =
  "monocode:showremainingusagechange";
/** Fired on `window` whenever email masking flips (detail: boolean). */
export const MASK_EMAILS_CHANGE_EVENT = "monocode:maskemailschange";
/** Fired on `window` whenever composer autocorrect flips (detail: boolean). */
export const COMPOSER_AUTOCORRECT_CHANGE_EVENT =
  "monocode:composerautocorrectchange";
/** Fired on `window` whenever Monos are pinned to or unpinned from the icon rail (detail: boolean). */
export const RAIL_MONOS_PINNED_CHANGE_EVENT = "monocode:railmonospinnedchange";

function flagStore(key: string, fallback: boolean, event: string) {
  // Holds a saved value only while storage failed to keep it, so the switch
  // still flips for this window when localStorage is unavailable.
  let unsaved: boolean | null = null;
  const load = () => unsaved ?? readFlag(key) ?? fallback;
  const save = (value: boolean) => {
    writeFlag(key, value);
    unsaved = readFlag(key) === value ? null : value;
    if (typeof window === "undefined") return;
    window.dispatchEvent(new CustomEvent<boolean>(event, { detail: value }));
  };
  const subscribe = (onStoreChange: () => void) => {
    if (typeof window === "undefined") return () => {};
    // Other windows write the same localStorage; `storage` reports their saves.
    const onStorage = (storage: StorageEvent) => {
      if (storage.key !== key && storage.key !== null) return;
      unsaved = null;
      onStoreChange();
    };
    window.addEventListener(event, onStoreChange);
    window.addEventListener("storage", onStorage);
    return () => {
      window.removeEventListener(event, onStoreChange);
      window.removeEventListener("storage", onStorage);
    };
  };
  const useFlag = () => useSyncExternalStore(subscribe, load, () => fallback);
  return { load, save, subscribe, useFlag };
}

const showRemainingUsage = flagStore(
  SHOW_REMAINING_USAGE_KEY,
  SHOW_REMAINING_USAGE_DEFAULT,
  SHOW_REMAINING_USAGE_CHANGE_EVENT,
);
const maskEmails = flagStore(
  MASK_EMAILS_KEY,
  MASK_EMAILS_DEFAULT,
  MASK_EMAILS_CHANGE_EVENT,
);
const composerAutocorrect = flagStore(
  COMPOSER_AUTOCORRECT_KEY,
  COMPOSER_AUTOCORRECT_DEFAULT,
  COMPOSER_AUTOCORRECT_CHANGE_EVENT,
);
const railMonosPinned = flagStore(
  RAIL_MONOS_PINNED_KEY,
  RAIL_MONOS_PINNED_DEFAULT,
  RAIL_MONOS_PINNED_CHANGE_EVENT,
);

/** Usage meters fill with what is left instead of what is used. */
export const loadShowRemainingUsage = showRemainingUsage.load;
export const saveShowRemainingUsage = showRemainingUsage.save;
export const subscribeShowRemainingUsage = showRemainingUsage.subscribe;
export const useShowRemainingUsage = showRemainingUsage.useFlag;

/** Account emails stay blurred until clicked. */
export const loadMaskEmails = maskEmails.load;
export const saveMaskEmails = maskEmails.save;
export const subscribeMaskEmails = maskEmails.subscribe;
export const useMaskEmails = maskEmails.useFlag;

/** Session and mono composers spell check and autocorrect what you type. */
export const loadComposerAutocorrect = composerAutocorrect.load;
export const saveComposerAutocorrect = composerAutocorrect.save;
export const subscribeComposerAutocorrect = composerAutocorrect.subscribe;
export const useComposerAutocorrect = composerAutocorrect.useFlag;

/** Monos sit at the top of the icon rail instead of inside the project picker. */
export const loadRailMonosPinned = railMonosPinned.load;
export const saveRailMonosPinned = railMonosPinned.save;
export const subscribeRailMonosPinned = railMonosPinned.subscribe;
export const useRailMonosPinned = railMonosPinned.useFlag;
