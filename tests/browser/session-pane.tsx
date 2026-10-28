import { captureReaderAnchor, readerAnchorTop } from "./readerAnchor";
import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  SessionPane,
  type SessionPaneProps,
} from "../../src/features/sessions/ui/SessionPane";
import {
  TranscriptPool,
  TranscriptPoolOutlet,
} from "../../src/features/sessions/ui/TranscriptPool";
import type { Block } from "../../src/features/sessions/model/session";
import {
  saveTranscriptAnchor,
  saveTranscriptLayout,
} from "../../src/features/settings/model/appearance";
import "../../src/styles/index.css";

// Use the actual composer, review card and transcript. Only native IPC is fake.
Object.assign(window, {
  __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
  __TAURI_INTERNALS__: {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main" },
    },
    transformCallback: () => 0,
    invoke: async (command: string) => {
      if (command === "plugin:event|listen") return 0;
      if (command === "plugin:event|unlisten") return;
      if (command === "control_load") return null;
      if (command === "session_checkpoint_status") return { files: [] };
      throw new Error(`Unexpected fixture command: ${command}`);
    },
  },
});
saveTranscriptAnchor(true);
saveTranscriptLayout("chat");
const noop = () => {};
const callbacks = {
  onFocus: noop,
  onClose: noop,
  onCwdChange: noop,
  onBranchChange: noop,
  onWorkspaceModeChange: noop,
  onWorktreeBaseChange: noop,
  onModelChange: noop,
  onModelSettingsChange: noop,
  onRuntimeModeChange: noop,
  onSubmit: noop,
  onSaveDraft: noop,
  onRemoveDraft: noop,
  onStop: noop,
  onCompactContext: () => false,
  onPlaceSessionInFolder: noop,
  onDeleteQueuedMessage: noop,
  onEditQueuedMessage: noop,
  onQueuedMessageEditingChange: noop,
  onSteerQueuedMessage: noop,
  onResumeQueue: noop,
  onUsageLimitResume: noop,
  onUsageLimitResumeAtReset: noop,
  onUsageLimitDismiss: noop,
  onApproval: noop,
  onQuestionReply: noop,
  onOpenFile: noop,
  onOpenDiff: noop,
  onOpenPlan: noop,
  onBuildPlan: noop,
  onNewTerminal: noop,
} satisfies Partial<SessionPaneProps>;

function Fixture() {
  const [pool] = useState(() => new TranscriptPool());
  const [blocks, setBlocks] = useState<Block[]>([]);
  const [busy, setBusy] = useState(true);
  useEffect(() => {
    Object.assign(window, {
      captureReaderAnchor,
      readerAnchorTop,
      setReplayBlocks: (blocks: Block[], busy: boolean) => {
        setBlocks(blocks);
        setBusy(busy);
      },
    });
  }, []);
  return (
    <>
      <SessionPane
        {...callbacks}
        session={{
          id: "replay",
          title: "Replay",
          cwd: "/repo",
          harness: "claude",
          model: "claude:opus",
          modelSettings: {},
          runtimeMode: "supervised",
          blocks,
          busy,
        }}
        visible
        focused
        composerFocused
        inSplit={false}
        recents={[]}
        transcriptPool={pool}
      />
      <TranscriptPoolOutlet pool={pool} />
    </>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
