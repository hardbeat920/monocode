import { ChoiceMenu } from "@/ui/ChoiceMenu";
import { Button, Host, HStack, Menu, Spacer } from "@expo/ui/swift-ui";
import {
  accessibilityLabel,
  contentShape,
  frame,
  shapes,
} from "@expo/ui/swift-ui/modifiers";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  Alert,
  Platform,
  Image,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import Animated, {
  Easing,
  FadeIn,
  FadeOut,
  interpolate,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
} from "react-native-reanimated";

import { PulsingDot, useOffline } from "@/features/connection/ConnectionNotice";
import type { Harness } from "@/features/workspace/types";
import { FileTypeIcon } from "@/features/files/FileTypeIcon";
import { Icon, type IconName } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";
import { themedStyles, useColors, useScheme } from "@/ui/theme";

import {
  isImage,
  MAX_ATTACHMENTS,
  merge,
  pickFiles,
  pickPhotos,
  type Attachment,
} from "./attachments";
import { DictationBar } from "./DictationBar";
import { canDictate, insertDictation } from "./dictation";
import { ProviderLogo } from "./ProviderLogo";
import { COMPOSER_INPUT_ID } from "./SessionScreen";

import {
  useMentionSearch,
  type MentionFile,
} from "@/features/connection/useMentionSearch";
import {
  ComposerMentionText,
  ComposerTextRuns,
  COMPOSER_TEXT_METRICS,
} from "./ComposerMentionText";
import {
  TAG_ICON_SLOT,
  composerMentionParts,
  updateComposerMentions,
  prepareComposerMentions,
  type ComposerMention,
} from "./composerMentions";
import { FileMentionPicker } from "./FileMentionPicker";
import { mentionTokenAt, replaceMentionToken } from "./mentionToken";

export const COMPOSER_PLACEHOLDER = "Plan, ask, build…";
/** Mid-turn the desktop steers with the message or queues it, as its setting says. */
const BUSY_PLACEHOLDER = "Send a follow-up…";

const BUTTON = 36;
/** Gap between the round buttons and the composer's edge. */
const EDGE = 7;
const COLLAPSED = BUTTON + EDGE * 2;
const RADIUS = COLLAPSED / 2;
/** Expanded layout: context row on top, buttons and model along the bottom. */
const TOP_ROW = 46;
/** The attachments' strip, under the context row as on the desktop. */
const THUMB = 48;
const STRIP = THUMB + 10;
const BOTTOM_ROW = BUTTON + EDGE * 2;
const INPUT_MIN = 64;
const INPUT_MAX = 160;
/** Inset from the screen edges: roomier as a pill, wider as a card. */
const MARGIN_COLLAPSED = 20;
const MARGIN_EXPANDED = 12;
const MORPH = { duration: 280, easing: Easing.out(Easing.cubic) };
const DICTATION_FADE = { duration: 180, easing: Easing.out(Easing.cubic) };

/**
 * The prompt field. At rest it's a single glass pill: add, the field and the
 * mic. Focused (or holding a draft) it grows into a card with the project and
 * branch on top and the model along the bottom.
 * One progress value drives the morph, and the TextInput stays the same
 * element throughout so focus and the keyboard are never interrupted. A React
 * Native TextInput, so keyboard-controller can track it (it matches `nativeID`).
 * While the desktop can't be reached, or the session's latest error stands,
 * the field gives way to a line saying so and nothing can be sent; a draft
 * waits under it.
 */
export function Composer({
  context,
  branch,
  model,
  effort,
  harness,
  onSend,
  busy = false,
  onStop,
  onModelPress,
  workspace,
  notice,
  projectId,
  worktreePath,
}: {
  /** Where this runs: the project, or the mono. */
  context: string;
  /** Scope on the paired desktop, shared with Files and Changes. */
  projectId?: string;
  worktreePath?: string;
  branch: string;
  model: string;
  /** The model's effort, shown dimmer after its name as on desktop. */
  effort?: string;
  harness?: Harness;
  onSend: (text: string, attachments: Attachment[]) => void | Promise<void>;
  busy?: boolean;
  onStop?: () => Promise<void>;
  /** Opens the model sheet. */
  onModelPress?: () => void;
  /** Where it works, shown in place of the context and branch. */
  workspace?: ReactNode;
  /** The session's latest error, in place of the field until tapped away. */
  notice?: { text: string; dismiss: () => void } | null;
}) {
  const scheme = useScheme();
  const colors = useColors();
  const styles = useStyles();
  const offline = useOffline();
  const blocked = offline ? "offline" : notice ? "error" : null;
  const input = useRef<TextInput>(null);
  const [text, setText] = useState("");
  const [taggedFiles, setTaggedFiles] = useState<ComposerMention[]>([]);
  const mentionParts = useMemo(
    () => composerMentionParts(text, taggedFiles),
    [text, taggedFiles],
  );
  const hasMentions = mentionParts.some((part) => part.file);
  const [sending, setSending] = useState(false);
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [inputHeight, setInputHeight] = useState(INPUT_MIN);
  const [dictating, setDictating] = useState(false);
  // Where dictated words go: the cursor, or the selection they replace.
  const selection = useRef({ start: 0, end: 0 });
  const [cursor, setCursor] = useState(0);
  const [rangeSelected, setRangeSelected] = useState(false);
  const [inputSelection, setInputSelection] = useState<{
    start: number;
    end: number;
  }>();
  const [mentionFocused, setMentionFocused] = useState(false);
  const [dismissedMention, setDismissedMention] = useState<string | null>(null);
  const [activeChoice, setActiveChoice] = useState({ key: "", index: 0 });
  const editingTag = taggedFiles.find(
    (file) =>
      file.start <= cursor &&
      cursor <= file.end &&
      text.slice(file.start + 1, file.end) === file.label,
  );
  const token = rangeSelected
    ? null
    : editingTag
      ? {
          start: editingTag.start,
          end: editingTag.end,
          query: text.slice(editingTag.start + 1, cursor),
        }
      : mentionTokenAt(text, cursor);
  const mentionKey = JSON.stringify([projectId, worktreePath, token, cursor]);
  const showMentions =
    !!projectId &&
    !!token &&
    mentionFocused &&
    !blocked &&
    !dictating &&
    dismissedMention !== mentionKey;
  const mentionSearch = useMentionSearch(
    projectId,
    showMentions ? token!.query : null,
    worktreePath,
  );
  const activeMention =
    activeChoice.key === mentionKey ? activeChoice.index : 0;
  const pickMention = (file: MentionFile) => {
    if (!showMentions || !token || !mentionSearch.results.includes(file))
      return;
    const label = file.name;
    const replaced = replaceMentionToken(text, token, label);
    const next =
      replaced.slice(0, token.start) +
      TAG_ICON_SLOT +
      replaced.slice(token.start + 1);
    setTaggedFiles((current) => [
      ...updateComposerMentions(text, next, current, {
        start: token.start,
        end: token.end,
      }),
      {
        ...file,
        label,
        canonicalLabel: file.label,
        start: token.start,
        end: token.start + 1 + label.length,
      },
    ]);
    let position = token.start + 1 + label.length;
    if (next[position] === " ") position += 1;
    selection.current = { start: position, end: position };
    setInputSelection(selection.current);
    setCursor(position);
    setRangeSelected(false);
    setText(next);
    input.current?.focus();
  };
  const canSend =
    (text.trim().length > 0 || attachments.length > 0) && !sending;
  // A draft, in words or files, keeps the card open.
  const hasText = text.length > 0 || attachments.length > 0;

  // Focus lives on the UI thread rather than in React state. The keyboard
  // starts moving as the field focuses, and a React commit in that same frame
  // makes Reanimated skip the KeyboardStickyView animation on iOS (new arch),
  // so the composer jumps up before the keyboard arrives. Typing commits, but
  // by then the keyboard is up. See "Missing animations on iOS" in the
  // keyboard-controller KeyboardStickyView docs.
  const focused = useSharedValue(false);
  const progress = useSharedValue(0);
  const morph = (expand: boolean) =>
    progress.set(withTiming(expand ? 1 : 0, MORPH));
  useEffect(() => {
    if (hasText) progress.set(withTiming(1, MORPH));
    else if (!focused.get()) progress.set(withTiming(0, MORPH));
  }, [hasText, focused, progress]);

  // Lets go of the keyboard as the field gives way.
  useEffect(() => {
    if (blocked) input.current?.blur();
  }, [blocked]);

  const strip = attachments.length > 0 ? STRIP : 0;
  const expandedHeight = TOP_ROW + strip + inputHeight + BOTTOM_ROW;

  const containerStyle = useAnimatedStyle(() => ({
    height: interpolate(progress.get(), [0, 1], [COLLAPSED, expandedHeight]),
    marginHorizontal: interpolate(
      progress.get(),
      [0, 1],
      [MARGIN_COLLAPSED, MARGIN_EXPANDED],
    ),
  }));
  // The field slides from between the pill's buttons to its own band in the card.
  const inputStyle = useAnimatedStyle(() => {
    const p = progress.get();
    return {
      left: interpolate(p, [0, 1], [EDGE + BUTTON + 8, 18]),
      right: interpolate(p, [0, 1], [EDGE + BUTTON + 8, 18]),
      top: interpolate(p, [0, 1], [EDGE - 1, TOP_ROW + strip]),
      bottom: interpolate(p, [0, 1], [EDGE, BOTTOM_ROW]),
    };
  });
  // Dictation takes over the bottom row: its buttons, and the pill's field.
  const dictation = useSharedValue(0);
  // Android still hit-tests transparent views. These absolute rows overlap the
  // collapsed input, so only their children may receive touches once the card
  // has finished opening. Keep this on the UI thread with the morph (no focus
  // state/React commit that would interrupt the iOS keyboard animation).
  const expandedOnly = useAnimatedStyle(() => ({
    opacity:
      interpolate(progress.get(), [0.35, 1], [0, 1], "clamp") *
      (1 - dictation.get()),
    ...(Platform.OS === "android"
      ? {
          pointerEvents:
            progress.get() === 1 && dictation.get() === 0
              ? ("box-none" as const)
              : ("none" as const),
        }
      : {}),
  }));
  useEffect(() => {
    dictation.set(withTiming(dictating ? 1 : 0, DICTATION_FADE));
  }, [dictating, dictation]);
  const undictated = useAnimatedStyle(() => ({
    opacity: 1 - dictation.get(),
    ...(Platform.OS === "android"
      ? {
          pointerEvents:
            dictation.get() === 0 && !blocked
              ? ("auto" as const)
              : ("none" as const),
        }
      : {}),
  }));
  const fieldShown = useAnimatedStyle(() => ({
    opacity: 1 - dictation.get() * (1 - progress.get()),
  }));

  const inputScroll = useSharedValue({ x: 0, y: 0 });

  const dictate = () => {
    if (!canDictate()) {
      Alert.alert("Dictation unavailable", "Dictation needs iOS 26 or later.");
      return;
    }
    // The keyboard makes way; a draft keeps the card open above the recording.
    input.current?.blur();
    setDictating(true);
  };
  const endDictation = (words: string | null) => {
    setDictating(false);
    if (!words) return;
    const next = insertDictation(text, words, selection.current);
    setTaggedFiles((current) =>
      updateComposerMentions(text, next.text, current),
    );
    selection.current = { start: next.cursor, end: next.cursor };
    setCursor(next.cursor);
    setText(next.text);
  };

  const attach = async (pick: (room: number) => Promise<Attachment[]>) => {
    const room = MAX_ATTACHMENTS - attachments.length;
    if (room <= 0) {
      Alert.alert(
        "Attachment limit",
        `A message can carry up to ${MAX_ATTACHMENTS} files.`,
      );
      return;
    }
    try {
      const picked = await pick(room);
      if (!picked.length) return;
      // Merged here, not in an updater, so its size error lands in the catch.
      setAttachments(merge(attachments, picked));
      // Ready to say what they're for.
      input.current?.focus();
    } catch (reason) {
      Alert.alert(
        "Could not attach",
        reason instanceof Error ? reason.message : String(reason),
      );
    }
  };

  // Clears, lets go of the keyboard and folds back to the pill at once; the
  // draft comes back if the desktop never takes it.
  const send = async () => {
    // The draft stays put until the desktop can take it.
    if (!canSend || blocked) return;
    const draft = text;
    const message = prepareComposerMentions(text, taggedFiles).trim();
    const tags = taggedFiles;
    setTaggedFiles([]);
    inputScroll.set({ x: 0, y: 0 });
    const files = attachments;
    setSending(true);
    setText("");
    selection.current = { start: 0, end: 0 };
    setCursor(0);
    setInputSelection(undefined);
    setRangeSelected(false);
    setMentionFocused(false);
    setAttachments([]);
    focused.set(false);
    morph(false);
    input.current?.blur();
    try {
      await onSend(message, files);
    } catch (reason) {
      setTaggedFiles((current) => [...tags, ...current]);
      setText((current) => current || draft);
      setAttachments((current) => (current.length ? current : files));
      Alert.alert(
        "Message not sent",
        reason instanceof Error ? reason.message : String(reason),
      );
    } finally {
      setSending(false);
    }
  };

  return (
    <View pointerEvents="box-none">
      {showMentions ? (
        <FileMentionPicker
          files={mentionSearch.results}
          composerHeight={expandedHeight}
          active={activeMention}
          searching={mentionSearch.searching}
          error={mentionSearch.error}
          onPick={pickMention}
          onDismiss={() => setDismissedMention(mentionKey)}
        />
      ) : null}
      <Animated.View style={containerStyle}>
        <LiquidView
          fill
          row={false}
          borderRadius={RADIUS}
          tintColor={
            Platform.OS === "android" && scheme === "light"
              ? "rgba(255,255,255,0.88)"
              : undefined
          }
          style={StyleSheet.absoluteFill}
        >
          <View />
        </LiquidView>

        <Animated.View style={[styles.contextRow, expandedOnly]}>
          <View style={styles.picker}>
            {workspace ?? (
              <Text style={styles.context} numberOfLines={1}>
                {context} <Text style={styles.branch}>{branch}</Text>
              </Text>
            )}
          </View>
        </Animated.View>

        {attachments.length > 0 ? (
          <Animated.View style={[styles.strip, expandedOnly]}>
            <ScrollView
              horizontal
              showsHorizontalScrollIndicator={false}
              keyboardShouldPersistTaps="handled"
              contentContainerStyle={styles.stripContent}
            >
              {attachments.map((attachment) => (
                <AttachmentChip
                  key={attachment.id}
                  attachment={attachment}
                  onRemove={() =>
                    setAttachments((current) =>
                      current.filter((item) => item.id !== attachment.id),
                    )
                  }
                />
              ))}
            </ScrollView>
          </Animated.View>
        ) : null}

        <Animated.View style={[styles.inputSlot, inputStyle, fieldShown]}>
          {hasMentions && !blocked ? (
            <ComposerMentionText
              text={text}
              parts={mentionParts}
              scroll={inputScroll}
              iconsOnly={Platform.OS === "android"}
            />
          ) : null}
          <TextInput
            ref={input}
            nativeID={COMPOSER_INPUT_ID}
            value={Platform.OS === "android" ? undefined : text}
            editable={!dictating && !blocked}
            selection={inputSelection}
            onChangeText={(value) => {
              // Native key events cannot cancel a text edit. Accept a hardware
              // Tab from the text change itself so it cannot leave a stray tab.
              const range = selection.current;
              if (
                showMentions &&
                mentionSearch.results[activeMention] &&
                value ===
                  `${text.slice(0, range.start)}\t${text.slice(range.end)}`
              ) {
                pickMention(mentionSearch.results[activeMention]);
                return;
              }
              // Native selection follows text; predict it for this render.
              const position = Math.max(
                0,
                Math.min(
                  value.length,
                  selection.current.end + value.length - text.length,
                ),
              );
              selection.current = { start: position, end: position };
              setCursor(position);
              setRangeSelected(false);
              setInputSelection(undefined);
              setTaggedFiles((current) =>
                updateComposerMentions(text, value, current, range),
              );
              setText(value);
              setMentionFocused(true);
            }}
            onSelectionChange={(event) => {
              selection.current = event.nativeEvent.selection;
              setCursor(selection.current.end);
              setRangeSelected(
                selection.current.start !== selection.current.end,
              );
              setInputSelection(undefined);
              if (
                focused.get() &&
                (mentionTokenAt(text, selection.current.end) ||
                  taggedFiles.some(
                    (file) =>
                      file.start <= selection.current.end &&
                      selection.current.end <= file.end,
                  ))
              )
                setMentionFocused(true);
            }}
            onKeyPress={(event) => {
              if (!showMentions) return;
              const key = event.nativeEvent.key;
              if (key === "Escape") {
                event.preventDefault();
                setDismissedMention(mentionKey);
              } else if (key === "ArrowDown" || key === "ArrowUp") {
                event.preventDefault();
                const count = mentionSearch.results.length;
                if (count)
                  setActiveChoice({
                    key: mentionKey,
                    index:
                      (activeMention + (key === "ArrowDown" ? 1 : count - 1)) %
                      count,
                  });
              } else if (
                Platform.OS === "web" &&
                key === "Tab" &&
                mentionSearch.results[activeMention]
              ) {
                event.preventDefault();
                pickMention(mentionSearch.results[activeMention]);
              }
            }}
            onFocus={() => {
              // Keep the UI-thread-only focus animation for an empty field.
              if (token) setMentionFocused(true);
              focused.set(true);
              morph(true);
            }}
            onBlur={() => {
              setMentionFocused(false);
              focused.set(false);
              if (!hasText) morph(false);
            }}
            onContentSizeChange={(event) =>
              setInputHeight(
                Math.min(
                  INPUT_MAX,
                  Math.max(
                    INPUT_MIN,
                    event.nativeEvent.contentSize.height + 12,
                  ),
                ),
              )
            }
            onScroll={(event) =>
              inputScroll.set(event.nativeEvent.contentOffset)
            }
            textBreakStrategy="simple"
            lineBreakStrategyIOS="none"
            underlineColorAndroid="transparent"
            placeholder={busy ? BUSY_PLACEHOLDER : COMPOSER_PLACEHOLDER}
            placeholderTextColor={colors.muted}
            multiline
            keyboardAppearance={scheme}
            cursorColor={colors.accent}
            selectionColor={colors.accent}
            style={[
              styles.input,
              Platform.OS !== "android" &&
                hasMentions &&
                styles.transparentInput,
              blocked && styles.hidden,
            ]}
          >
            {Platform.OS === "android" ? (
              <ComposerTextRuns parts={mentionParts} />
            ) : null}
          </TextInput>
          {blocked === "offline" ? (
            <Animated.View
              key="offline"
              entering={FadeIn.duration(DICTATION_FADE.duration)}
              exiting={FadeOut.duration(DICTATION_FADE.duration)}
              accessibilityRole="text"
              accessibilityLabel="Reconnecting to your desktop. Messages can't be sent until it's back."
              style={styles.status}
            >
              <PulsingDot />
              <Text style={styles.statusLabel} numberOfLines={1}>
                Reconnecting to desktop
              </Text>
            </Animated.View>
          ) : blocked === "error" && notice ? (
            <Animated.View
              key="error"
              entering={FadeIn.duration(DICTATION_FADE.duration)}
              exiting={FadeOut.duration(DICTATION_FADE.duration)}
              style={StyleSheet.absoluteFill}
            >
              <Pressable
                accessibilityRole="button"
                accessibilityLabel={`Couldn't continue: ${notice.text}`}
                accessibilityHint="Dismisses the error so you can write again"
                onPress={notice.dismiss}
                style={({ pressed }) => [
                  styles.status,
                  pressed && styles.pressed,
                ]}
              >
                <Text
                  style={[styles.statusLabel, styles.errorLabel]}
                  numberOfLines={1}
                >
                  {notice.text}
                </Text>
                <Icon name="close" size={14} color={colors.muted} />
              </Pressable>
            </Animated.View>
          ) : null}
        </Animated.View>

        {/* Bottom row, shared by both layouts: the buttons stay put as the card grows. */}
        <Animated.View
          pointerEvents={blocked ? "none" : "auto"}
          style={[
            styles.button,
            styles.leading,
            undictated,
            blocked && styles.disabled,
          ]}
        >
          <AddMenu
            onPhotos={() => void attach(pickPhotos)}
            onFiles={() => void attach(pickFiles)}
          />
        </Animated.View>

        <Animated.View style={[styles.modelChip, expandedOnly]}>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={`Model: ${model || "Desktop default"}${effort ? `, ${effort} effort` : ""}`}
            accessibilityHint="Choose the model, account, effort and permissions"
            onPress={onModelPress}
            style={({ pressed }) => [
              styles.modelPill,
              pressed && styles.pressed,
            ]}
          >
            {harness ? (
              <ProviderLogo
                harness={harness}
                size={16}
                color={colors.content}
              />
            ) : null}
            <Text style={styles.model} numberOfLines={1}>
              {model || "Desktop default"}
              {effort ? <Text style={styles.effort}> {effort}</Text> : null}
            </Text>
            <Icon name="chevronDown" size={14} color={colors.muted} />
          </Pressable>
        </Animated.View>

        <Animated.View
          pointerEvents={blocked ? "none" : "auto"}
          style={[
            styles.button,
            styles.trailing,
            undictated,
            blocked && styles.disabled,
          ]}
        >
          {/* A draft mid-turn sends; without one the button stops the turn. */}
          {canSend ? (
            <RoundButton
              icon="send"
              label="Send"
              onPress={() => void send()}
              primary
            />
          ) : busy && onStop ? (
            <RoundButton
              label="Stop"
              onPress={() => {
                void onStop().catch((reason) =>
                  Alert.alert("Could not stop", String(reason)),
                );
              }}
              primary
            >
              <View style={styles.stop} />
            </RoundButton>
          ) : (
            <RoundButton icon="mic" label="Dictate" onPress={dictate} />
          )}
        </Animated.View>

        {dictating ? (
          <Animated.View
            entering={FadeIn.duration(DICTATION_FADE.duration)}
            exiting={FadeOut.duration(DICTATION_FADE.duration)}
            style={styles.dictation}
          >
            <DictationBar onClose={endDictation} />
          </Animated.View>
        ) : null}
      </Animated.View>
    </View>
  );
}

/**
 * The add button: a native menu to attach photos or files. The menu lies
 * over the button, its label an invisible box of the button's size, as in
 * SessionFilterMenu.
 */
function AddMenu({ onPhotos, onFiles }: { onPhotos: () => void; onFiles: () => void }) {
  const styles = useStyles();
  const colors = useColors();
  if (Platform.OS === "android") return (
    <ChoiceMenu label="Add photos or files" actions={[
      { label: "Photo Library", onPress: onPhotos },
      { label: "Files", onPress: onFiles },
    ]}>
      <View style={styles.round}><Icon name="add" size={18} color={colors.content} /></View>
    </ChoiceMenu>
  );
  return (
    <View>
      <View style={styles.round}>
        <Icon name="add" size={18} color={colors.content} />
      </View>
      <Host style={StyleSheet.absoluteFill}>
        <Menu
          modifiers={[accessibilityLabel("Add photos or files")]}
          label={
            <HStack
              modifiers={[
                frame({ width: BUTTON, height: BUTTON }),
                contentShape(shapes.circle()),
              ]}
            >
              <Spacer />
            </HStack>
          }
        >
          <Button label="Photo Library" systemImage="photo.on.rectangle" onPress={onPhotos} />
          <Button label="Files" systemImage="folder" onPress={onFiles} />
        </Menu>
      </Host>
    </View>
  );
}

/** A held attachment: a photo's thumbnail, or a file's icon and name. */
function AttachmentChip({
  attachment,
  onRemove,
}: {
  attachment: Attachment;
  onRemove: () => void;
}) {
  const styles = useStyles();
  const colors = useColors();
  const image = isImage(attachment);
  return (
    <View style={image ? styles.thumb : styles.fileChip}>
      {image ? (
        <Image source={{ uri: attachment.uri }} style={styles.thumbImage} resizeMode="cover" />
      ) : (
        <>
          <FileTypeIcon name={attachment.name} isDir={false} size={20} />
          <Text style={styles.fileName} numberOfLines={1}>
            {attachment.name}
          </Text>
        </>
      )}
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Remove ${attachment.name}`}
        hitSlop={8}
        onPress={onRemove}
        style={({ pressed }) => [styles.remove, pressed && styles.pressed]}
      >
        <Icon name="close" size={10} color={colors.content} />
      </Pressable>
    </View>
  );
}

function RoundButton({
  icon,
  label,
  onPress,
  primary = false,
  children,
}: {
  icon?: IconName;
  label: string;
  onPress?: () => void;
  primary?: boolean;
  children?: ReactNode;
}) {
  const colors = useColors();
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      hitSlop={4}
      onPress={onPress}
      style={({ pressed }) => [
        styles.round,
        primary && styles.roundPrimary,
        pressed && styles.pressed,
      ]}
    >
      {children ??
        (icon ? (
          <Icon name={icon} size={18} color={primary ? colors.inverse : colors.content} />
        ) : null)}
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  stop: {
    width: 12,
    height: 12,
    borderRadius: 3,
    backgroundColor: colors.inverse,
  },
  contextRow: {
    position: "absolute",
    top: 12,
    left: 14,
    right: 14,
    flexDirection: "row",
  },
  picker: {
    flexShrink: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    height: 30,
    paddingHorizontal: 6,
  },
  context: {
    flexShrink: 1,
    color: colors.content,
    fontSize: 15,
    fontWeight: "500",
  },
  branch: {
    color: colors.muted,
    fontWeight: "400",
  },
  inputSlot: {
    position: "absolute",
  },
  strip: {
    position: "absolute",
    top: TOP_ROW,
    left: 0,
    right: 0,
    height: STRIP,
  },
  stripContent: {
    alignItems: "center",
    gap: 8,
    paddingHorizontal: 16,
  },
  thumb: {
    width: THUMB,
    height: THUMB,
  },
  thumbImage: {
    width: THUMB,
    height: THUMB,
    borderRadius: 10,
    backgroundColor: colors.fill,
  },
  fileChip: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    height: THUMB,
    maxWidth: 180,
    paddingLeft: 10,
    paddingRight: 14,
    borderRadius: 10,
    backgroundColor: colors.fill,
  },
  fileName: {
    flexShrink: 1,
    color: colors.content,
    fontSize: 13,
  },
  // Over the chip's top-right corner, as the desktop's.
  remove: {
    position: "absolute",
    top: -5,
    right: -5,
    width: 18,
    height: 18,
    borderRadius: 9,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.surfaceRaised,
  },
  input: {
    flex: 1,
    paddingTop: 8,
    paddingBottom: 8,
    color: colors.content,
    backgroundColor: "transparent",
    ...COMPOSER_TEXT_METRICS,
    textAlignVertical: "top",
  },
  transparentInput: { color: "transparent" },
  button: {
    position: "absolute",
    bottom: EDGE,
  },
  leading: {
    left: EDGE,
  },
  trailing: {
    right: EDGE,
  },
  // Over the bottom row, edge to edge between the pill's ends.
  dictation: {
    position: "absolute",
    left: EDGE,
    right: EDGE,
    bottom: EDGE,
    height: BUTTON,
  },
  modelChip: {
    position: "absolute",
    bottom: EDGE,
    left: EDGE + BUTTON + 8,
    right: EDGE + BUTTON + 8,
    flexDirection: "row",
  },
  // Shaped like the round buttons beside it: their height, tint and full rounding.
  modelPill: {
    flexShrink: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    height: BUTTON,
    paddingHorizontal: 12,
    borderRadius: BUTTON / 2,
    backgroundColor: colors.fill,
  },
  model: {
    flexShrink: 1,
    color: colors.content,
    fontSize: 15,
    fontWeight: "500",
  },
  effort: {
    color: colors.muted,
    fontWeight: "400",
  },
  round: {
    width: BUTTON,
    height: BUTTON,
    borderRadius: BUTTON / 2,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.fill,
  },
  roundPrimary: {
    backgroundColor: colors.inverseSurface,
  },
  pressed: {
    opacity: 0.6,
  },
  // In the field's place while nothing can be sent.
  status: {
    ...StyleSheet.absoluteFill,
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
  },
  statusLabel: {
    flexShrink: 1,
    color: colors.muted,
    fontSize: 15,
  },
  errorLabel: {
    flex: 1,
    color: colors.deletions,
  },
  hidden: {
    opacity: 0,
  },
  disabled: {
    opacity: 0.4,
  },
}));
