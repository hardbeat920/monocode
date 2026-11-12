import { useState } from "react";
import { createRoot } from "react-dom/client";
import { SurfaceTabs } from "../../src/features/workspace/ui/SurfaceTabs";
import type { FilePaneTab } from "../../src/features/workspace/model/layout";
import "../../src/styles/index.css";

const file = (id: string): FilePaneTab => ({
  id,
  path: `/repo/${id}.tsx`,
  cwd: "/repo",
});

function Fixture() {
  const [files, setFiles] = useState([file("a"), file("b"), file("c")]);
  const [active, setActive] = useState("a");
  return (
    <>
      <button onClick={() => setActive("c")}>Select last</button>
      <button
        onClick={() => {
          setFiles((current) => [...current, file("d")]);
          setActive("d");
        }}
      >
        Open file
      </button>
      <div data-ancestor className="h-28 overflow-auto">
        <div className="h-20" />
        <div className="w-64">
          <SurfaceTabs
            files={files}
            activeFileId={active}
            dirtyFileIds={new Set()}
            fileErrorCounts={new Map()}
            onSelectFile={setActive}
            onCloseFile={() => {}}
            onCloseOtherFiles={() => {}}
            onReorder={() => {}}
          />
        </div>
        <div className="h-40" />
      </div>
    </>
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);
