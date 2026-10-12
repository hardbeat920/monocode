import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { AgentTranscript } from "../../src/features/sessions/ui/AgentTranscript";
import type { Block } from "../../src/features/sessions/model/session";
import {
  saveTranscriptAnchor,
  saveTranscriptLayout,
} from "../../src/features/settings/model/appearance";
import "../../src/styles/index.css";

saveTranscriptAnchor(false);
saveTranscriptLayout("chat");

const REPLY = Array.from(
  { length: 6 },
  (_, i) =>
    `Paragraph ${i}. This reply streams in word by word and then settles before the tab is hidden.`,
).join("\n\n");

type State = { text: string; streaming: boolean; busy: boolean };

function Fixture() {
  const [state, setState] = useState<State>({
    text: "",
    streaming: true,
    busy: true,
  });
  const [hidden, setHidden] = useState(false);
  useEffect(() => {
    Object.assign(window, {
      REPLY,
      setReply: (next: State) => setState(next),
      hideTab: () => setHidden(true),
      showTab: () => setHidden(false),
    });
  }, []);
  const blocks: Block[] = [
    { id: "user-1", role: "user", text: "Explain this." },
    {
      id: "reply-1",
      role: "assistant",
      text: state.text,
      streaming: state.streaming,
    },
  ];
  // Like App: inactive workspace tabs stay mounted under display: none.
  return (
    <div className={hidden ? "hidden" : "flex h-full flex-col"}>
      <AgentTranscript blocks={blocks} busy={state.busy} visible={!hidden} />
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);
