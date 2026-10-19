import { File } from "expo-file-system";
import { launchImageLibraryAsync } from "expo-image-picker";
import Storage from "expo-sqlite/kv-store";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ImageSourcePropType } from "react-native";

import { BACKGROUNDS, forgetRenders, prepareImage, renderDither } from "./render";

/** A project's empty-session background, as set on this phone. */
export type BackgroundSettings = {
  /** The picked photo's file name in BACKGROUNDS; none until one is picked. */
  image: string | null;
  /** 0.05–0.65, the desktop's range. */
  opacity: number;
};

export const OPACITY_RANGE = { min: 0.05, max: 0.65 } as const;

const DEFAULTS: BackgroundSettings = { image: null, opacity: 0.45 };

/** How the empty session shows it. */
export type EmptyBackground = {
  source: ImageSourcePropType;
  opacity: number;
};

const storageKey = (projectId: string) => `background:${projectId}`;

/** Every screen showing a project's background, told when it is changed
 *  elsewhere: in settings, say, for the empty session under it. */
const watchers = new Map<string, Set<() => void>>();
/** Names a photo's render, so a new photo never reuses an old one's. */
const renderKey = (projectId: string, image: string) => `${encodeURIComponent(projectId)}.${image}`;

function load(projectId: string): BackgroundSettings {
  try {
    const raw = Storage.getItemSync(storageKey(projectId));
    if (!raw) return DEFAULTS;
    const { image, opacity } = { ...DEFAULTS, ...JSON.parse(raw) };
    return { image, opacity };
  } catch {
    return DEFAULTS;
  }
}

/**
 * A project's empty-session background: its settings, which persist on this
 * phone, the photo to show, dithered as the desktop's effect draws it, and
 * the controls the background sheet changes them with.
 */
export function useEmptyBackground(projectId: string) {
  const [settings, setSettings] = useState(() => load(projectId));
  // The last photo drawn; a new one replaces it once ready.
  // Faded out to clear, so the one render suits every scheme and theme.
  const [drawn, setDrawn] = useState<{ image: string; uri: string | null } | null>(null);
  // Another project's screen, without remounting: its own settings.
  const [shownProject, setShownProject] = useState(projectId);
  if (shownProject !== projectId) {
    setShownProject(projectId);
    setSettings(load(projectId));
  }
  // This screen's ear for changes made on another.
  const listener = useRef<(() => void) | null>(null);
  useEffect(() => {
    const reload = () => setSettings(load(projectId));
    listener.current = reload;
    const set = watchers.get(projectId) ?? new Set<() => void>();
    watchers.set(projectId, set.add(reload));
    return () => {
      set.delete(reload);
    };
  }, [projectId]);
  const [picking, setPicking] = useState(false);

  const update = useCallback(
    (patch: Partial<BackgroundSettings>) => {
      setSettings((current) => {
        const next = { ...current, ...patch };
        void Storage.setItem(storageKey(projectId), JSON.stringify(next)).then(() =>
          watchers.get(projectId)?.forEach(
            (reload) => reload !== listener.current && reload(),
          ),
        );
        return next;
      });
    },
    [projectId],
  );

  const { image, opacity } = settings;
  useEffect(() => {
    if (!image) return;
    let cancelled = false;
    const source = new File(BACKGROUNDS, image).uri;
    const key = renderKey(projectId, image);
    (async () => {
      // Lets the spinner show before the dither takes the JS thread for a moment.
      await new Promise((resolve) => setTimeout(resolve, 16));
      return renderDither(source, key);
    })().then(
      (uri) => {
        if (!cancelled) setDrawn({ image, uri });
      },
      () => {
        if (!cancelled) setDrawn({ image, uri: null });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [projectId, image]);

  /** Lets the user pick a photo, then uses it. False if they cancelled. */
  const pickImage = useCallback(async () => {
    const result = await launchImageLibraryAsync({ mediaTypes: "images", quality: 1 });
    const picked = result.assets?.[0];
    if (result.canceled || !picked) return false;
    setPicking(true);
    try {
      const name = `${encodeURIComponent(projectId)}-${Date.now()}.jpg`;
      await prepareImage(picked.uri, new File(BACKGROUNDS, name));
      removeImageFile(projectId, image);
      update({ image: name });
      return true;
    } finally {
      setPicking(false);
    }
  }, [projectId, image, update]);

  const removeImage = useCallback(() => {
    removeImageFile(projectId, image);
    update({ image: null });
  }, [projectId, image, update]);

  // The last photo's render stays up until this one's is ready.
  const shown = !!image && drawn?.image === image;
  const uri = shown ? drawn.uri : null;
  return {
    settings,
    background: uri ? { source: { uri }, opacity } : null,
    /** While a photo is prepared or dithered, for the sheet's spinner. */
    busy: picking || (!!image && !shown),
    pickImage,
    removeImage,
    setOpacity: (next: number) => update({ opacity: next }),
  };
}

function removeImageFile(projectId: string, image: string | null) {
  if (!image) return;
  forgetRenders(renderKey(projectId, image));
  const file = new File(BACKGROUNDS, image);
  if (file.exists) file.delete();
}
