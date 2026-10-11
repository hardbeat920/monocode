import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { TitleBar, type Tab } from "../../src/app/shell/TitleBar";
import "../../src/styles/index.css";

// Keep the real title bar and window controls, replacing only native IPC.
mockWindows("main");
mockIPC(
  (command) => {
    if (command === "plugin:window|is_maximized") return false;
    if (command === "plugin:window|set_title") return;
    throw new Error(`Unexpected command: ${command}`);
  },
  { shouldMockEvents: true },
);

const tab = (id: string): Tab => ({
  id,
  project: "agent-terminal",
  title: `Tab ${id}`,
  more: [],
  sessionCount: 1,
  harnesses: ["claude"],
  busyHarnesses: [],
  files: [],
});

function Fixture() {
  const [ids, setIds] = useState(["a", "b", "c", "d", "e"]);
  const [activeId, setActiveId] = useState("a");
  const [tick, setTick] = useState(0);
  const [ticking, setTicking] = useState(false);
  useEffect(() => {
    Object.assign(window, {
      order: ids,
      activeId,
      startTicking: () => setTicking(true),
    });
  }, [ids, activeId]);
  // Like App: busy agents re-render the title bar with fresh tab objects.
  useEffect(() => {
    if (!ticking) return;
    const timer = setInterval(() => setTick((t) => t + 1), 50);
    return () => clearInterval(timer);
  }, [ticking]);
  const tabs = ids.map((id) => ({
    ...tab(id),
    busyHarnesses: tick % 2 ? (["claude"] as Tab["harnesses"]) : [],
  }));
  return (
    <>
    <TitleBar
      tabs={tabs}
      activeId={activeId}
      cwd="/tmp/agent-terminal"
      onToggleSidebar={() => {}}
      onSelect={setActiveId}
      onClose={(id) => setIds((list) => list.filter((x) => x !== id))}
      onCloseMany={() => {}}
      onReorder={(next) => setIds(next)}
      onPlaceOnPane={(id) => {
        (window as unknown as { placed: string[] }).placed.push(id);
      }}
    />
    <div data-pane-id="pane-1" style={{ height: 400 }} />
    </>
  );
}

Object.assign(window, { placed: [] });
createRoot(document.getElementById("root")!).render(<Fixture />);
