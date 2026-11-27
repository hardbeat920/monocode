import {
  AudioModule,
  RecordingPresets,
  requestRecordingPermissionsAsync,
  setAudioModeAsync,
  setIsAudioActiveAsync,
  type AudioRecorder,
  type RecordingOptions,
} from "expo-audio";
import { File } from "expo-file-system";
import * as Haptics from "expo-haptics";
import { useEffect, useRef, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  AppState,
  Linking,
  Pressable,
  Text,
  View,
  type LayoutChangeEvent,
} from "react-native";
import Animated, {
  Easing,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
  type SharedValue,
} from "react-native-reanimated";

import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

import { formatElapsed, meterLevel, prepareDictation, transcribe } from "./dictation";

const BUTTON = 36;
/** Readings kept for the waveform, newest last. */
const HISTORY = 64;
/** How often the meter is read: each reading shifts the waveform a bar. */
const METER_INTERVAL = 80;
/** A bar and the gap after it. */
const BAR_PITCH = 5;
const BAR_WIDTH = 2;
const WAVE_HEIGHT = 24;
/** Long enough for a thought; the recording is transcribed when it ends. */
const LIMIT_SECONDS = 5 * 60;
const BAR_TIMING = { duration: 100, easing: Easing.out(Easing.quad) };

const OPTIONS: RecordingOptions = {
  ...RecordingPresets.HIGH_QUALITY,
  isMeteringEnabled: true,
};

type Phase = "preparing" | "recording" | "transcribing" | "done";

/**
 * The composer's bottom row while dictating: cancel, the live waveform and its
 * time, and done. Mounted only while dictating, so the recorder is made then
 * rather than with every composer. `onClose` gets the words heard, or null
 * when cancelled or nothing came of it; unmounting stops and discards the
 * recording.
 */
export function DictationBar({ onClose }: { onClose: (words: string | null) => void }) {
  const colors = useColors();
  const styles = useStyles();
  const [phase, setPhase] = useState<Phase>("preparing");
  const stage = useRef<Phase>("preparing");
  const recorder = useRef<AudioRecorder | null>(null);
  const [seconds, setSeconds] = useState(0);
  const levels = useSharedValue<number[]>(Array(HISTORY).fill(0));
  // Read through a function: TypeScript keeps a ref's narrowing across awaits.
  const stopped = () => stage.current === "done";
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  });

  const fail = (title: string, reason: unknown) => {
    if (stage.current === "done") return;
    stage.current = "done";
    Alert.alert(title, reason instanceof Error ? reason.message : String(reason));
    close.current(null);
  };

  const finish = async () => {
    const current = recorder.current;
    if (stage.current !== "recording" || !current) return;
    stage.current = "transcribing";
    setPhase("transcribing");
    try {
      await current.stop().catch(() => {});
      // Lets other apps' audio, paused for the recording, play on.
      await endAudioSession();
      const uri = current.uri;
      if (!uri) throw new Error("Nothing was recorded.");
      const words = await transcribe(await new File(uri).arrayBuffer());
      if (stage.current !== "transcribing") return;
      stage.current = "done";
      if (!words) Alert.alert("No speech heard", "Try again a little closer to the microphone.");
      close.current(words || null);
    } catch (reason) {
      fail("Could not transcribe", reason);
    }
  };

  // Asks for the microphone, readies the language's speech assets and starts.
  useEffect(() => {
    let created: AudioRecorder | null = null;
    const start = async () => {
      const permission = await requestRecordingPermissionsAsync();
      if (stage.current === "done") return;
      if (!permission.granted) {
        stage.current = "done";
        Alert.alert(
          "Microphone access",
          "Monocode needs the microphone to take dictation.",
          permission.canAskAgain
            ? [{ text: "OK" }]
            : [
                { text: "Cancel", style: "cancel" },
                { text: "Open Settings", onPress: () => void Linking.openSettings() },
              ],
        );
        close.current(null);
        return;
      }
      await prepareDictation();
      if (stopped()) return;
      await setAudioModeAsync({
        allowsRecording: true,
        playsInSilentMode: true,
        interruptionMode: "doNotMix",
      });
      await setIsAudioActiveAsync(true);
      // Made here rather than with useAudioRecorder, so it is only made when
      // dictation starts, and is stopped before it is released on unmount.
      // eslint-disable-next-line import/namespace -- a native class the rule can't see
      created = new AudioModule.AudioRecorder(OPTIONS);
      recorder.current = created;
      created.addListener("recordingStatusUpdate", (status) => {
        if (status.hasError || status.mediaServicesDidReset) {
          fail("Recording interrupted", status.error ?? "The microphone was taken away.");
        } else if (status.isFinished) {
          // At the time limit: what was said so far goes in.
          void finish();
        }
      });
      await created.prepareToRecordAsync();
      if (stopped()) return;
      created.record({ forDuration: LIMIT_SECONDS });
      stage.current = "recording";
      setPhase("recording");
      void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
    };
    start().catch((reason) => fail("Could not start dictation", reason));

    return () => {
      stage.current = "done";
      if (!created) return;
      const leftover = created;
      void leftover
        .stop()
        .catch(() => {})
        .then(() => {
          try {
            if (leftover.uri) new File(leftover.uri).delete();
          } catch {}
          leftover.release();
        });
      void endAudioSession();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The meter, read on a timer: each reading is the waveform's newest bar.
  useEffect(() => {
    if (phase !== "recording") return;
    const timer = setInterval(() => {
      const status = recorder.current?.getStatus();
      if (!status) return;
      const level = meterLevel(status.metering);
      const history = levels.get();
      levels.set([...history.slice(1), level]);
      setSeconds(Math.floor(status.durationMillis / 1000));
    }, METER_INTERVAL);
    return () => clearInterval(timer);
  }, [phase, levels]);

  // Leaving the app ends the recording rather than holding the microphone.
  useEffect(() => {
    const subscription = AppState.addEventListener("change", (state) => {
      if (state === "background" && stage.current !== "done") {
        stage.current = "done";
        close.current(null);
      }
    });
    return () => subscription.remove();
  }, []);

  const cancel = () => {
    if (stage.current === "done") return;
    stage.current = "done";
    close.current(null);
  };

  return (
    <View style={styles.row}>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel="Cancel dictation"
        hitSlop={4}
        onPress={cancel}
        style={({ pressed }) => [styles.cancel, pressed && styles.pressed]}
      >
        <Icon name="close" size={20} color={colors.content} />
      </Pressable>

      <View style={styles.middle}>
        {phase === "recording" ? (
          <>
            <Waveform levels={levels} />
            <Text style={styles.time}>{formatElapsed(seconds * 1000)}</Text>
          </>
        ) : (
          <Text style={styles.status} numberOfLines={1}>
            {phase === "transcribing" ? "Transcribing…" : "Starting…"}
          </Text>
        )}
      </View>

      <Pressable
        accessibilityRole="button"
        accessibilityLabel="Finish dictation"
        hitSlop={4}
        disabled={phase !== "recording"}
        onPress={() => void finish()}
        style={({ pressed }) => [
          styles.done,
          phase !== "recording" && styles.doneWaiting,
          pressed && styles.pressed,
        ]}
      >
        {phase === "recording" ? (
          <Icon name="check" size={18} color={colors.inverse} />
        ) : (
          <ActivityIndicator size="small" color={colors.muted} />
        )}
      </Pressable>
    </View>
  );
}

/** Releases the audio session; Expo leaves it active once recording stops. */
async function endAudioSession() {
  await setAudioModeAsync({ allowsRecording: false }).catch(() => {});
  await setIsAudioActiveAsync(false).catch(() => {});
}

/** As many bars as fit, the newest on the right; a quiet one is a faint dot. */
function Waveform({ levels }: { levels: SharedValue<number[]> }) {
  const styles = useStyles();
  const [count, setCount] = useState(0);
  const onLayout = (event: LayoutChangeEvent) =>
    setCount(Math.min(HISTORY, Math.floor(event.nativeEvent.layout.width / BAR_PITCH)));
  return (
    <View style={styles.wave} onLayout={onLayout}>
      {Array.from({ length: count }, (_, index) => (
        <Bar key={index} levels={levels} index={HISTORY - count + index} />
      ))}
    </View>
  );
}

function Bar({ levels, index }: { levels: SharedValue<number[]>; index: number }) {
  const styles = useStyles();
  const style = useAnimatedStyle(() => {
    const level = levels.get()[index] ?? 0;
    return {
      opacity: withTiming(0.3 + level * 0.7, BAR_TIMING),
      transform: [
        { scaleY: withTiming((BAR_WIDTH + level * (WAVE_HEIGHT - BAR_WIDTH)) / WAVE_HEIGHT, BAR_TIMING) },
      ],
    };
  });
  return <Animated.View style={[styles.bar, style]} />;
}

const useStyles = themedStyles((colors) => ({
  row: {
    flex: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  cancel: {
    width: BUTTON,
    height: BUTTON,
    alignItems: "center",
    justifyContent: "center",
  },
  middle: {
    flex: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    height: BUTTON,
  },
  wave: {
    flex: 1,
    height: WAVE_HEIGHT,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  bar: {
    width: BAR_WIDTH,
    height: WAVE_HEIGHT,
    borderRadius: BAR_WIDTH / 2,
    backgroundColor: colors.content,
  },
  time: {
    color: colors.muted,
    fontSize: 15,
    fontVariant: ["tabular-nums"],
  },
  status: {
    flex: 1,
    textAlign: "center",
    color: colors.muted,
    fontSize: 15,
  },
  done: {
    width: BUTTON,
    height: BUTTON,
    borderRadius: BUTTON / 2,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.inverseSurface,
  },
  doneWaiting: {
    backgroundColor: colors.fill,
  },
  pressed: {
    opacity: 0.6,
  },
}));
