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
  const [width, setWidth] = useState(600);
  const [files, setFiles] = useState([file("a"), file("b"), file("c")]);
  const [active, setActive] = useState("a");
  return (
    <>
      <button onClick={() => setWidth(900)}>Expand</button>
      <button onClick={() => setWidth(260)}>Narrow</button>
      <button onClick={() => setActive("c")}>Select last</button>
      <button onClick={() => setActive("a")}>Select first</button>
      <button
        onClick={() => {
          setFiles((current) => [...current, file("d")]);
          setActive("d");
        }}
      >
        Open file
      </button>
      <output data-order>{files.map((entry) => entry.id).join("")}</output>
      <div data-ancestor className="h-28 overflow-auto">
        <div className="h-20" />
        <div data-surface style={{ width }}>
          <SurfaceTabs
            files={files}
            activeFileId={active}
            dirtyFileIds={new Set()}
            fileErrorCounts={new Map()}
            onSelectFile={setActive}
            onCloseFile={(id) => {
              setFiles((current) => current.filter((entry) => entry.id !== id));
              setActive("a");
            }}
            onCloseOtherFiles={() => {}}
            onReorder={(ids) =>
              setFiles((current) =>
                ids.map((id) => current.find((entry) => entry.id === id)!),
              )
            }
            onPaneDragStart={() => {}}
            trailing={<div data-trailing className="w-6 shrink-0" />}
          />
        </div>
        <div className="h-40" />
      </div>
    </>
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);
