import { useEffect, useState, type CSSProperties } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit, TauriEvent } from "@tauri-apps/api/event";
import { TitleBar, type Tab } from "../../src/app/shell/TitleBar";
import "../../src/styles/index.css";

// Keep the real title bar and window controls, replacing only native IPC.
const params = new URLSearchParams(location.search);
const inset = params.has("inset");
let fullscreen = params.has("fullscreen");
mockWindows("main");
mockIPC(
  (command) => {
    if (command === "plugin:window|is_maximized") return false;
    if (command === "plugin:window|is_fullscreen") return fullscreen;
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
  const [sidebarOpen, setSidebarOpen] = useState(params.has("sidebar"));
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
  const titleBar = (
    <TitleBar
      tabs={tabs}
      activeId={activeId}
      cwd="/tmp/agent-terminal"
      inset={inset}
      sessionSidebarOpen={sidebarOpen}
      onToggleSessionSidebar={
        inset ? () => setSidebarOpen((open) => !open) : undefined
      }
      onToggleSidebar={() => {}}
      onSelect={setActiveId}
      onClose={(id) => setIds((list) => list.filter((x) => x !== id))}
      onCloseMany={() => {}}
      onReorder={(next) => setIds(next)}
      onPlaceOnPane={(id) => {
        (window as unknown as { placed: string[] }).placed.push(id);
      }}
    />
  );
  return inset ? (
    <div
      className="inset-shell inset-outer"
      style={
        { "--inset-rail-width": sidebarOpen ? "224px" : "0px" } as CSSProperties
      }
    >
      {titleBar}
      <div className="flex" style={{ height: 400 }}>
        <aside
          className="inset-rail-host"
          data-layout-rail
          style={{ width: sidebarOpen ? 224 : 0 }}
        >
          <div className={sidebarOpen ? "h-full" : "hidden"} />
        </aside>
        <main
          data-layout-card
          className="inset-card mr-1.5 flex-1 rounded-[10px] border border-stroke"
        />
      </div>
    </div>
  ) : (
    <>
      {titleBar}
      <div data-pane-id="pane-1" style={{ height: 400 }} />
    </>
  );
}

Object.assign(window, {
  placed: [],
  setFullscreen: async (next: boolean) => {
    fullscreen = next;
    await emit(TauriEvent.WINDOW_RESIZED, {
      width: innerWidth,
      height: innerHeight,
    });
  },
});
createRoot(document.getElementById("root")!).render(<Fixture />);
