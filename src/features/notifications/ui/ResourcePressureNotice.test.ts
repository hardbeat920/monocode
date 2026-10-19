// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  isTauri: () => false,
  convertFileSrc: (path: string) => path,
}));
import { ResourcePressureNotice } from "./ResourcePressureNotice";
import { newSession } from "../../sessions/model/session";

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  invoke.mockReset();
});

it("monitors silently without a footer and closes its dialog when the footer disappears", async () => {
  vi.useFakeTimers();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  invoke.mockImplementation(async (command: string) => {
    if (command === "resource_usage")
      return { cpuPercent: 95, memoryPressure: 1 };
    if (command === "resource_processes") return [];
    throw new Error(command);
  });
  const container = document.createElement("div");
  const footer = document.createElement("span");
  document.body.append(container, footer);
  const root = createRoot(container);
  const onPressureChange = vi.fn();
  const props = {
    sessions: { current: [] },
    onStopAgent: vi.fn(),
    onStopSession: vi.fn(),
    onPressureChange,
  };
  const render = (chipHost?: HTMLElement) =>
    act(async () => {
      root.render(
        createElement(ResourcePressureNotice, { ...props, chipHost }),
      );
    });
  const processPolls = () =>
    invoke.mock.calls.filter(([command]) => command === "resource_processes")
      .length;
  try {
    await render();
    await act(async () => vi.advanceTimersByTimeAsync(10_000));
    expect(onPressureChange).toHaveBeenLastCalledWith(true);
    expect(document.querySelector(".approval-toast")).toBeNull();
    expect(
      document.querySelector('[aria-label="High resource usage"]'),
    ).toBeNull();
    expect(container.textContent).toBe("");
    expect(processPolls()).toBe(0);

    await render(footer);
    expect(footer.textContent).toContain("High usage");
    await act(async () => footer.querySelector("button")!.click());
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    await render();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(footer.querySelector("button")).toBeNull();
    const polls = processPolls();
    await act(async () => vi.advanceTimersByTimeAsync(6_000));
    expect(processPolls()).toBe(polls);
    expect(document.querySelector(".approval-toast")).toBeNull();

    await render(footer);
    expect(footer.textContent).toContain("High usage");
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
    container.remove();
    footer.remove();
  }
});

it("only lists processes when opened and stops a tool only after its button is clicked", async () => {
  vi.useFakeTimers();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const session = {
    ...newSession("claude", "/repo"),
    id: "owned",
    title: "Build",
  };
  const tool = {
    pid: 42,
    identity: "42:123:456",
    sessionId: "owned",
    cwd: "/repo/tree",
    command: "cargo build",
    cpuPercent: 150,
    memoryBytes: 1024,
    agent: false,
  };
  const other = { ...tool, pid: 43, identity: "43:124:456", command: "rustc" };
  const agent = {
    ...tool,
    pid: 41,
    identity: "41:122:456",
    command: "claude",
    agent: true,
  };
  let rows = [tool, other, agent];
  let cpuPercent = 95;
  invoke.mockImplementation(async (command: string) => {
    if (command === "resource_usage") return { cpuPercent, memoryPressure: 1 };
    if (command === "resource_processes") return rows;
    if (command === "resource_stop_tool") return;
    throw new Error(command);
  });
  const container = document.createElement("div");
  const chipHost = document.createElement("span");
  document.body.append(container);
  document.body.append(chipHost);
  const root = createRoot(container);
  const onStopAgent = vi.fn();
  const onStopSession = vi.fn();
  try {
    await act(async () =>
      root.render(
        createElement(ResourcePressureNotice, {
          sessions: { current: [session] },
          onStopAgent,
          onStopSession,
          chipHost,
        }),
      ),
    );
    await act(async () => vi.advanceTimersByTimeAsync(10_000));
    expect(chipHost.textContent).toContain("High usage");
    expect(
      invoke.mock.calls.some(([command]) => command !== "resource_usage"),
    ).toBe(false);
    await act(async () => chipHost.querySelector("button")!.click());
    expect(document.querySelectorAll("[data-resource-session]")).toHaveLength(
      1,
    );
    expect(document.querySelector("details")?.open).toBe(false);
    expect(document.querySelector("summary")?.textContent).toContain(
      "3 processes",
    );
    expect(document.body.textContent).not.toContain("UI stall time");
    await act(async () => document.querySelector("summary")!.click());
    expect(document.body.textContent).toContain("cargo build");
    const stop = [...document.querySelectorAll("button")].find(
      (button) =>
        button.getAttribute("aria-label") ===
        "Stop build/tool for Build (PID 42)",
    )!;
    rows = [{ ...other, cpuPercent: 180 }, { ...tool, cpuPercent: 10 }, agent];
    await act(async () => vi.advanceTimersByTimeAsync(3_000));
    const controls = [...document.querySelectorAll("button")].filter((button) =>
      button.getAttribute("aria-label")?.startsWith("Stop build/tool"),
    );
    expect(controls[0]).toBe(stop);
    expect(controls[1].getAttribute("aria-label")).toContain("PID 43");
    expect(document.body.textContent).toContain("10% CPU");
    await act(async () => stop.click());
    expect(invoke).toHaveBeenCalledWith("resource_stop_tool", {
      pid: 42,
      identity: "42:123:456",
    });
    expect(onStopAgent).not.toHaveBeenCalled();
    expect(onStopSession).not.toHaveBeenCalled();
    const stopAgent = document.querySelector<HTMLButtonElement>(
      '[aria-label="Stop agent for Build (PID 41)"]',
    )!;
    await act(async () => stopAgent.click());
    expect(onStopAgent).toHaveBeenCalledExactlyOnceWith("owned");
    expect(onStopSession).not.toHaveBeenCalled();
    const stopSession = document.querySelector<HTMLButtonElement>(
      '[aria-label="Stop session Build"]',
    )!;
    let finishStop!: () => void;
    onStopSession.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finishStop = resolve;
        }),
    );
    await act(async () => stopSession.click());
    expect(onStopSession).toHaveBeenCalledExactlyOnceWith("owned");
    expect(stopSession.disabled).toBe(true);
    expect(stopSession.textContent).toContain("Stopping…");
    await act(async () => finishStop());
    expect(stopSession.disabled).toBe(false);

    onStopSession.mockRejectedValueOnce(new Error("stop failed"));
    await act(async () => stopSession.click());
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      "Couldn't stop this work",
    );
    expect(stopSession.disabled).toBe(false);

    cpuPercent = 10;
    await act(async () => vi.advanceTimersByTimeAsync(18_000));
    expect(document.body.textContent).toContain("Usage is back to normal");
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    expect(chipHost.querySelector("button")).toBeNull();
  } finally {
    await act(async () => root.unmount());
    container.remove();
    chipHost.remove();
  }
});
