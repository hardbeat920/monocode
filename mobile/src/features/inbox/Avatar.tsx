import { useEffect, useState } from "react";
import { Image, Text, View } from "react-native";

import type { InboxClient, InboxPerson } from "./protocol";

const TONES = ["#8b5cf6", "#0ea5a4", "#e0658f", "#e08a3c", "#4c8fe0", "#5c6bd8"];

/** Up to two letters of a name, as the mock's avatars. */
export function initials(name: string) {
  const words = name.replace(/[^A-Za-z ]/g, " ").trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  const letters = words.length > 1 ? words[0][0] + words[1][0] : (words[0][0] + (words[0][1] ?? ""));
  return letters.toUpperCase();
}

function toneOf(name: string) {
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return TONES[hash % TONES.length];
}

/** Avatars fetched from the desktop, by item and avatar key, kept for the app's life. */
const pictures = new Map<string, string | null>();

/**
 * A person's avatar: their picture from the desktop (it sends ones up to 64px,
 * never a URL the phone fetches) and initials until it arrives or if it can't.
 */
export function Avatar({
  person,
  itemKey,
  client,
  size = 28,
}: {
  person: InboxPerson;
  itemKey: string;
  /** Without one, initials only. */
  client?: InboxClient;
  size?: number;
}) {
  const cacheKey = `${itemKey}\n${person.avatarKey}`;
  const [uri, setUri] = useState<string | null>(pictures.get(cacheKey) ?? null);
  useEffect(() => {
    if (!client || !person.avatarKey || pictures.has(cacheKey)) return;
    let cancelled = false;
    client.media(itemKey, { avatarKey: person.avatarKey }).then(
      (media) => {
        const value = `data:${media.mime};base64,${media.base64}`;
        pictures.set(cacheKey, value);
        if (!cancelled) setUri(value);
      },
      () => pictures.set(cacheKey, null),
    );
    return () => {
      cancelled = true;
    };
  }, [client, itemKey, person.avatarKey, cacheKey]);

  if (uri) {
    return (
      <Image
        accessibilityIgnoresInvertColors
        source={{ uri }}
        style={{ width: size, height: size, borderRadius: size / 2 }}
      />
    );
  }
  return (
    <View
      accessibilityElementsHidden
      style={{
        width: size,
        height: size,
        borderRadius: size / 2,
        alignItems: "center",
        justifyContent: "center",
        backgroundColor: toneOf(person.name),
      }}
    >
      <Text style={{ color: "#fff", fontSize: size * 0.4, fontWeight: "700" }}>
        {initials(person.name)}
      </Text>
    </View>
  );
}
