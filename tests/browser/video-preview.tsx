import { StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { FilePane } from "../../src/features/files/ui/FilePane";
import {
  newEditorPane,
  newFileTab,
} from "../../src/features/workspace/model/layout";
import "../../src/styles/index.css";

// Keep the real file pane and binary loader, replacing only native IPC.
Object.assign(window, {
  __TAURI_INTERNALS__: {
    invoke: async (command: string) => {
      if (command === "read_binary_file") {
        return fetch("./fixtures/preview.mp4").then((response) =>
          response.arrayBuffer(),
        );
      }
      if (command === "stat_files") return [];
      throw new Error(`Unexpected command: ${command}`);
    },
  },
});

const first = newFileTab("/repo/clip.mp4", "/repo");
const second = newFileTab("/repo/other.mp4", "/repo");
const initialPane = { ...newEditorPane(first), files: [first, second] };
const noop = () => {};
const dirtyFileIds = new Set<string>();
const fileErrorCounts = new Map<string, number>();

function Fixture() {
  const [pane, setPane] = useState(initialPane);
  const [visible, setVisible] = useState(true);
  return (
    <div className="flex h-full flex-col">
      <button type="button" onClick={() => setVisible(!visible)}>
        {visible ? "Hide workspace" : "Show workspace"}
      </button>
      <div className={visible ? "min-h-0 flex-1" : "hidden"}>
        <FilePane
          pane={pane}
          focused
          visible={visible}
          dirtyFileIds={dirtyFileIds}
          fileErrorCounts={fileErrorCounts}
          sessions={[]}
          onFocus={noop}
          onSelectFile={(_paneId, activeFileId) =>
            setPane({ ...pane, activeFileId })
          }
          onCloseFile={noop}
          onCloseOtherFiles={noop}
          onDirtyChange={noop}
          onErrorCountChange={noop}
          onReorderFiles={noop}
          onOpenFile={noop}
          onUpdatePlan={noop}
          onBuildPlan={noop}
        />
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Fixture />
  </StrictMode>,
);
