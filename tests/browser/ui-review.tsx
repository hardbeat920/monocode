import { useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { TitleBar, type Tab } from "../../src/app/shell/TitleBar";
import { Composer } from "../../src/features/sessions/ui/Composer";
import { ProjectTerminalDock } from "../../src/features/terminal/ui/ProjectTerminalDock";
import {
  createProjectTerminal,
  type DockSide,
} from "../../src/features/projects/model/projectTerminal";
import { newTerminalFile } from "../../src/features/workspace/model/layout";
import { seedProjectBranches } from "../../src/features/source-control/hooks/useProjectBranches";
import { setHarnessModels } from "../../src/features/sessions/model/models";
import "../../src/styles/index.css";
import { applyBodyGlass, applySidebarOpacity } from "../../src/features/settings/model/appearance";

// All native operations, including PTYs and file access, stay in this fixture.
mockWindows("main");
const branch = "feature/a-long-checkout-name-for-composer-readability";
mockIPC(
  (command) => {
    if (command === "plugin:window|is_maximized") return false;
    if (command === "pty_status") return { foreground: null };
    if (command === "git_branches")
      return { current: branch, detached: false, branches: [] };
    if (command === "plugin:dialog|open") return ["/fixture/notes.txt"];
    if (command === "inspect_paths")
      return [
        {
          path: "/fixture/notes.txt",
          name: "notes.txt",
          size: 24,
          isDir: false,
        },
      ];
    if (command === "list_project_files")
      return [
        {
          path: "/fixture/src/app/App.tsx",
          relative: "src/app/App.tsx",
          name: "App.tsx",
          isDir: false,
        },
      ];
    if (command.startsWith("plugin:") || command.startsWith("pty_")) return;
    return [];
  },
  { shouldMockEvents: true },
);
seedProjectBranches("/fixture", {
  current: branch,
  detached: false,
  branches: [],
});

const params = new URLSearchParams(location.search);
const modelId = "claude:sonnet-5";
if (params.has("long"))
  setHarnessModels("claude", [
    {
      id: modelId,
      harness: "claude",
      name: "An exceptionally long model name for readability and narrow pane testing",
    },
  ]);
document.documentElement.classList.toggle("theme-light", params.has("light"));
const wallpaper = params.has("wallpaper");
if (params.has("glass")) {
  document.documentElement.classList.add("has-native-glass");
  applyBodyGlass(true);
  applySidebarOpacity(Number(params.get("opacity") ?? "0.9"));
}
const tab = (id: string): Tab => ({
  id,
  project: "Fixture",
  title: `Session ${id}`,
  more: [],
  sessionCount: 1,
  harnesses: ["claude"],
  busyHarnesses: [],
  files: [],
});

function Fixture() {
  const [ids, setIds] = useState(
    params.has("overflow")
      ? Array.from({ length: 16 }, (_, i) => `${i + 1}`)
      : ["1", "2"],
  );
  const [activeId, setActiveId] = useState("1");
  const [created, setCreated] = useState(0);
  const [sidebar, setSidebar] = useState(!params.has("closed"));
  const [dock, setDock] = useState(() =>
    createProjectTerminal(
      "/fixture",
      newTerminalFile("/fixture", "Terminal 1"),
      (params.get("side") ?? "bottom") as DockSide,
    ),
  );
  const [terminalCreates, setTerminalCreates] = useState(0);
  const [model, setModel] = useState(modelId);
  const [permission, setPermission] = useState<"supervised" | "full-access">(
    "supervised",
  );
  const [sent, setSent] = useState("");
  const create = () => {
    const id = `new-${created + 1}`;
    setCreated((n) => n + 1);
    setIds((list) => [...list, id]);
    setActiveId(id);
  };
  return (
    <div
      style={{
        minHeight: "100vh",
        background: wallpaper
          ? "linear-gradient(125deg, #355e83, #978574 50%, #486b59)"
          : "var(--color-background-base)",
      }}
    >
      <TitleBar
        tabs={ids.map(tab)}
        activeId={activeId}
        cwd="/fixture"
        mono={
          params.has("mono")
            ? {
                look: {
                  name: "A long Mono name that should keep its available title width",
                  mascot: "ghost",
                  color: "#8ba4d9",
                  projects: [],
                },
                state: { status: "idle" },
              }
            : undefined
        }
        sessionSidebarOpen={sidebar}
        onToggleSessionSidebar={() => setSidebar(!sidebar)}
        onToggleSidebar={() => setSidebar(!sidebar)}
        onSelect={setActiveId}
        {...{ onNewSession: create }}
        onClose={(id) => setIds((list) => list.filter((x) => x !== id))}
        onCloseMany={() => {}}
        onReorder={setIds}
      />
      <div style={{ display: "flex", minWidth: 0 }}>
        {sidebar && (
          <aside className={params.has("glass") ? "sidebar-glass" : undefined} style={{ width: 180, flexShrink: 0, padding: 12 }}>
            Sessions
          </aside>
        )}
        <main className={params.has("glass") ? "body-glass" : undefined} style={{ minWidth: 0, flex: 1, paddingTop: 120 }}>
          <Composer
            focused={created > 0}
            focusToken={created}
            harness="claude"
            model={model}
            runtimeMode={permission}
            cwd="/fixture"
            executionCwd="/fixture"
            branch={branch}
            hideProjectPicker={!params.has("project")}
            onFocus={() => {}}
            onCwdChange={() => {}}
            onModelChange={(_, next) => setModel(next)}
            onRuntimeModeChange={setPermission}
            onSubmit={(text) => setSent(text)}
            editLastTurnSupported={params.has("edit")}
            lastTurnRecall={{ text: "Editable prompt", attachments: [] }}
            onRecallLastTurnReady={(recall) => {
              Object.assign(window, { reviewBeginEdit: recall });
            }}
          />
          <div style={{ height: 220, marginTop: 32 }}>
            <ProjectTerminalDock
              dock={dock}
              focused={false}
              onFocus={() => {}}
              onHide={() => {}}
              onSideChange={(side) => setDock((d) => ({ ...d, side }))}
              onSizePaint={() => {}}
              onSizeCommit={() => {}}
              onAddTerminal={() => {
                setTerminalCreates((n) => n + 1);
                const file = newTerminalFile(
                  "/fixture",
                  `Terminal ${terminalCreates + 2}`,
                );
                setDock((d) => ({
                  ...d,
                  pane: {
                    ...d.pane,
                    files: [...d.pane.files, file],
                    activeFileId: file.id,
                  },
                }));
              }}
              onSelectTerminal={(id) =>
                setDock((d) => ({
                  ...d,
                  pane: { ...d.pane, activeFileId: id },
                }))
              }
              onCloseTerminal={(id) =>
                setDock((d) => ({
                  ...d,
                  pane: {
                    ...d.pane,
                    files: d.pane.files.filter((f) => f.id !== id),
                  },
                }))
              }
              onCloseOtherTerminals={() => {}}
              onReorderTerminals={(ids) =>
                setDock((d) => ({
                  ...d,
                  pane: {
                    ...d.pane,
                    files: ids.map((id) =>
                      d.pane.files.find((f) => f.id === id)!,
                    ),
                  },
                }))
              }
            />
          </div>
          <output data-created>{created}</output>
          <output data-terminal-created>{terminalCreates}</output>
          <output data-sent>{sent}</output>
        </main>
      </div>
    </div>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
