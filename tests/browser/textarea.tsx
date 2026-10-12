import { useState } from "react";
import { createRoot } from "react-dom/client";
import { Composer } from "../../src/features/sessions/ui/Composer";
import { InstructionsField } from "../../src/features/orchestration/ui/OrchestrationPreview";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import "../../src/styles/index.css";

// The composer reads the window and asks the host for skills and files; an
// empty host is enough for layout.
mockWindows("main");
mockIPC(() => []);

const noop = () => {};

function Fixture() {
  const [instructions, setInstructions] = useState("");
  return (
    <>
      <div data-pane="composer" style={{ width: 700 }}>
        <Composer
          focused={false}
          harness="claude"
          model="sonnet"
          runtimeMode="supervised"
          executionCwd="/tmp"
          onFocus={noop}
          onCwdChange={noop}
          onModelChange={noop}
          onRuntimeModeChange={noop}
          onSubmit={noop}
        />
      </div>
      <div data-pane="instructions" style={{ width: 700 }}>
        <InstructionsField
          label="Instructions"
          value={instructions}
          className="w-full text-sm leading-5.5"
          onChange={setInstructions}
        />
      </div>
    </>
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);
