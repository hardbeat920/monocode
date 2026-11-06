import { useCallback, useEffect, useMemo, useState } from "react";
import { ActivityIndicator, Image, Linking, Pressable, Text, View } from "react-native";

import { Markdown, type RenderImage } from "@/features/sessions/Markdown";
import type { MarkdownImage } from "@/features/sessions/markdownParser";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

import { imageUrls, keptImage, readImage, type ImagePlace } from "./bodyImages";
import type { InboxClient } from "./protocol";

/** The shape of a picture whose size nothing has said yet. */
const USUAL_RATIO = 16 / 10;

type Size = { width: number; height: number };

/**
 * A provider's text: Markdown with HTML mixed in, as people write pull
 * requests. Its images come through the desktop.
 */
export function InboxMarkdown({
  text,
  place,
  client,
}: {
  text: string;
  place: ImagePlace;
  client: Pick<InboxClient, "media">;
}) {
  const urls = useMemo(() => imageUrls(text), [text]);
  const { itemKey, commentId } = place;
  const renderImage = useCallback<RenderImage>(
    (image, href) => (
      <BodyImage
        image={image}
        href={href}
        // The first of a repeated address: it is the same picture.
        index={urls.indexOf(image.src)}
        place={{ itemKey, commentId }}
        client={client}
      />
    ),
    [urls, itemKey, commentId, client],
  );
  return <Markdown text={text} renderImage={renderImage} />;
}

function openable(url: string) {
  return /^https?:\/\//.test(url);
}

function BodyImage({
  image,
  href,
  index,
  place,
  client,
}: {
  image: MarkdownImage;
  href?: string;
  /** Its place in the host's count; below zero when the host wouldn't find it. */
  index: number;
  place: ImagePlace;
  client: Pick<InboxClient, "media">;
}) {
  const styles = useStyles();
  const colors = useColors();
  const { src, alt } = image;
  const [uri, setUri] = useState(() => keptImage(place, src));
  const [failed, setFailed] = useState(index < 0);
  const [room, setRoom] = useState(0);
  const [natural, setNatural] = useState<Size | null>(null);
  const { itemKey, commentId } = place;

  useEffect(() => {
    if (uri || index < 0) return;
    let cancelled = false;
    readImage(client, { itemKey, commentId }, { src, index }).then(
      (read) => {
        if (!cancelled) setUri(read);
      },
      () => {
        if (!cancelled) setFailed(true);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [client, itemKey, commentId, src, index, uri]);

  const target = href ?? src;
  const open = openable(target) ? () => void Linking.openURL(target) : undefined;

  if (failed) {
    return (
      <Pressable
        accessibilityRole={open ? "link" : "text"}
        accessibilityLabel={alt ? `Image: ${alt}` : "Image"}
        disabled={!open}
        onPress={open}
        style={({ pressed }) => [styles.missing, pressed && styles.pressed]}
      >
        <Icon name="image" size={16} color={colors.muted} />
        <Text numberOfLines={1} style={styles.missingLabel}>
          {alt || "Image"}
        </Text>
        {open ? <Icon name="external" size={14} color={colors.faint} /> : null}
      </Pressable>
    );
  }

  // The `<img>` tag's size first: it holds the space before the picture arrives.
  const asked = image.width && image.height ? { width: image.width, height: image.height } : null;
  const shape = natural ?? asked;
  const ratio = shape ? shape.width / shape.height : USUAL_RATIO;
  const width = Math.min(room, image.width ?? natural?.width ?? room);
  const box = { width, height: width / ratio };

  return (
    <View onLayout={(event) => setRoom(event.nativeEvent.layout.width)}>
      {room > 0 ? (
        <Pressable
          accessibilityRole={open ? "imagebutton" : "image"}
          accessibilityLabel={alt || "Image"}
          disabled={!open}
          onPress={open}
          style={[styles.frame, box]}
        >
          {uri ? (
            <Image
              accessibilityIgnoresInvertColors
              source={{ uri }}
              resizeMode="contain"
              onLoad={({ nativeEvent: { source } }) => setNatural(source)}
              onError={() => setFailed(true)}
              style={box}
            />
          ) : (
            <ActivityIndicator color={colors.muted} />
          )}
        </Pressable>
      ) : null}
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  frame: {
    alignItems: "center",
    justifyContent: "center",
    overflow: "hidden",
    borderRadius: 10,
    borderCurve: "continuous",
    backgroundColor: colors.fill,
  },
  missing: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    minHeight: 44,
    paddingHorizontal: 12,
    borderRadius: 10,
    borderCurve: "continuous",
    backgroundColor: colors.fill,
  },
  missingLabel: {
    flex: 1,
    color: colors.muted,
    fontSize: 14,
  },
  pressed: {
    opacity: 0.7,
  },
}));
