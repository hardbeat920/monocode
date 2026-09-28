import { useEffect, useState, useSyncExternalStore } from "react";
import {
  isLightScheme,
  SCHEME_CHANGE_EVENT,
  type NewThreadBackgroundEffect,
} from "../../settings/model/appearance";
import { prepareNewThreadBackgroundEffect } from "../../settings/model/newThreadBackgroundEffects";
import { projectChatBackgroundSrc } from "../model/chatBackground";

function subscribeScheme(listener: () => void) {
  window.addEventListener(SCHEME_CHANGE_EVENT, listener);
  return () => window.removeEventListener(SCHEME_CHANGE_EVENT, listener);
}

/** Returns a pane-local image URL, leaving the global background untouched. */
export function useProjectBackgroundEffect(
  path: string | null,
  effect: NewThreadBackgroundEffect,
  revision: number,
): string | null {
  const light = useSyncExternalStore(
    subscribeScheme,
    isLightScheme,
    () => false,
  );
  const src = path ? projectChatBackgroundSrc(path, revision) : null;
  const sourceKey = path ? `${path}?v=${revision}` : null;
  const themeKey =
    effect === "dither" || effect === "none" || effect === "gradient-blur"
      ? false
      : light;
  const key = sourceKey ? `${sourceKey}:${effect}:${themeKey}` : null;
  const [prepared, setPrepared] = useState<{
    key: string;
    url: string;
  } | null>(null);

  useEffect(() => {
    if (
      !sourceKey ||
      !src ||
      effect === "none" ||
      effect === "gradient-blur" ||
      !key
    )
      return;
    let cancelled = false;
    let objectUrl: string | null = null;
    void prepareNewThreadBackgroundEffect(sourceKey, src, effect, themeKey)
      .then((blob) => {
        if (cancelled) return;
        objectUrl = URL.createObjectURL(blob);
        setPrepared({ key, url: objectUrl });
      })
      .catch(() => {
        if (!cancelled) setPrepared({ key, url: src });
      });
    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [sourceKey, src, effect, themeKey, key]);

  if (!src || effect === "none" || effect === "gradient-blur") return src;
  return prepared?.key === key ? prepared.url : null;
}
