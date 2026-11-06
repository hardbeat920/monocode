import { useEffect, useState } from "react";
import { Image, Text, View } from "react-native";

import { initials } from "./model";
import type { InboxClient, InboxPerson } from "./protocol";

const TONES = ["#8b5cf6", "#0ea5a4", "#e0658f", "#e08a3c", "#4c8fe0", "#5c6bd8"];

function toneOf(name: string) {
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return TONES[hash % TONES.length];
}

/**
 * Avatars fetched from the desktop, by avatar key, kept for the app's life.
 * Not by item: the item an author is listed through changes with the filters.
 */
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
  const { avatarKey } = person;
  const [uri, setUri] = useState<string | null>(
    avatarKey ? (pictures.get(avatarKey) ?? null) : null,
  );
  useEffect(() => {
    if (!client || !avatarKey || pictures.has(avatarKey)) return;
    let cancelled = false;
    client.media(itemKey, { avatarKey }).then(
      (media) => {
        const value = `data:${media.mime};base64,${media.base64}`;
        pictures.set(avatarKey, value);
        if (!cancelled) setUri(value);
      },
      () => pictures.set(avatarKey, null),
    );
    return () => {
      cancelled = true;
    };
  }, [client, itemKey, avatarKey]);

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
