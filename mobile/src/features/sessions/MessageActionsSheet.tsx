import {
  BottomSheet,
  Button,
  Group,
  HStack,
  Host,
  Image,
  List,
  Section,
  Spacer,
  Text,
  VStack,
} from "@expo/ui/swift-ui";
import {
  background,
  buttonStyle,
  contentShape,
  font,
  frame,
  listRowBackground,
  listRowInsets,
  listSectionSpacing,
  listStyle,
  presentationDetents,
  presentationDragIndicator,
  scrollContentBackground,
  shapes,
} from "@expo/ui/swift-ui/modifiers";
import * as Haptics from "expo-haptics";
import { Fragment, useRef, useState } from "react";

import { useColors, withAlpha } from "@/ui/theme";

/** Reactions offered on a Mono's message, two rows of six as Messages lays
 *  them out. They stay on this phone, on the message's bubble. */
const REACTIONS = [
  ["👍", "👎", "❤️", "😂", "🎉", "😮"],
  ["🔥", "👀", "🙏", "😢", "💯", "🤔"],
];

/** A long-pressed message in a Mono's chat, and whose it is. */
export type HeldMessage = { text: string; role: "user" | "agent"; id: string };

type Symbol = "arrowshape.turn.up.left" | "message.badge" | "doc.on.doc" | "character.cursor.ibeam";

/**
 * What can be done with a message, as Messages' sheet under a long press:
 * reply to it or mark the chat unread, then copy or select its text. Each
 * runs once the sheet has gone, so a keyboard or another sheet it opens
 * doesn't fight its dismissal.
 */
export function MessageActionsSheet({
  message,
  onDismiss,
  onReply,
  onMarkUnread,
  onCopy,
  onSelect,
  onReact,
}: {
  message: HeldMessage | null;
  onDismiss: () => void;
  onReply: (message: HeldMessage) => void;
  /** Only for the Mono's own messages. */
  onMarkUnread?: () => void;
  onCopy: (message: HeldMessage) => void;
  onSelect: (message: HeldMessage) => void;
  /** A reaction picked for a Mono's message. */
  onReact: (message: HeldMessage, emoji: string) => void;
}) {
  const after = useRef<(() => void) | null>(null);
  // Kept while the sheet slides away, so its rows don't empty out mid-dismissal.
  const [held, setHeld] = useState(message);
  if (message && message !== held) setHeld(message);
  const unread = held?.role === "agent" && onMarkUnread;
  const pick = (action: () => void) => {
    after.current = action;
    onDismiss();
  };
  return (
    <Host style={{ position: "absolute" }} pointerEvents="none">
      <BottomSheet
        isPresented={!!message}
        onIsPresentedChange={(presented) => {
          if (!presented) onDismiss();
        }}
        onDismiss={() => {
          const action = after.current;
          after.current = null;
          action?.();
        }}
      >
        <Group
          modifiers={[
            frame({ maxWidth: Infinity, alignment: "topLeading" }),
            presentationDetents([{ height: (unread ? 300 : 248) + (held?.role === "agent" ? 140 : 0) }]),
            presentationDragIndicator("visible"),
          ]}
        >
          <List
            modifiers={[
              listStyle("insetGrouped"),
              listSectionSpacing(12),
              scrollContentBackground("hidden"),
            ]}
          >
            {held ? (
              <>
                {held.role === "agent" ? (
                  <Section>
                    <Reactions onPick={(emoji) => pick(() => onReact(held, emoji))} />
                  </Section>
                ) : null}
                <Section>
                  <Row symbol="arrowshape.turn.up.left" label="Reply" onPress={() => pick(() => onReply(held))} />
                  {unread ? (
                    <Row symbol="message.badge" label="Mark as unread" onPress={() => pick(onMarkUnread)} />
                  ) : null}
                </Section>
                <Section>
                  <Row symbol="doc.on.doc" label="Copy" onPress={() => pick(() => onCopy(held))} />
                  <Row
                    symbol="character.cursor.ibeam"
                    label="Select Text"
                    onPress={() => pick(() => onSelect(held))}
                  />
                </Section>
              </>
            ) : null}
          </List>
        </Group>
      </BottomSheet>
    </Host>
  );
}

/** The reaction grid, bare on the sheet rather than in a card. */
function Reactions({ onPick }: { onPick: (emoji: string) => void }) {
  const colors = useColors();
  return (
    <VStack
      spacing={12}
      modifiers={[listRowBackground("clear"), listRowInsets({ top: 4, leading: 4, bottom: 4, trailing: 4 })]}
    >
      {REACTIONS.map((row, index) => (
        <HStack key={index} spacing={0}>
          {row.map((emoji, at) => (
            <Fragment key={emoji}>
              {at > 0 ? <Spacer /> : null}
              <Button
                onPress={() => {
                  void Haptics.selectionAsync();
                  onPick(emoji);
                }}
                modifiers={[buttonStyle("plain")]}
              >
                <Text
                  modifiers={[
                    font({ size: 26 }),
                    frame({ width: 50, height: 50 }),
                    background(withAlpha(colors.content, 0.08), shapes.circle()),
                    contentShape(shapes.circle()),
                  ]}
                >
                  {emoji}
                </Text>
              </Button>
            </Fragment>
          ))}
        </HStack>
      ))}
    </VStack>
  );
}

function Row({ symbol, label, onPress }: { symbol: Symbol; label: string; onPress: () => void }) {
  const colors = useColors();
  return (
    <Button onPress={onPress} modifiers={[buttonStyle("plain")]}>
      <HStack spacing={14} modifiers={[contentShape(shapes.rectangle())]}>
        <Image systemName={symbol} size={18} color={colors.content} modifiers={[frame({ width: 24 })]} />
        <Text>{label}</Text>
        <Spacer />
      </HStack>
    </Button>
  );
}
