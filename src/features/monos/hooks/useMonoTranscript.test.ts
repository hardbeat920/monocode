// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  findMonoTranscript,
  getMonoTranscriptPage,
} from "../../sessions/data/sessionStore";
import {
  newSession,
  type Block,
  type Session,
} from "../../sessions/model/session";
import { useMonoTranscript } from "./useMonoTranscript";

vi.mock("../../sessions/data/sessionStore", () => ({
  getMonoTranscriptPage: vi.fn(),
  findMonoTranscript: vi.fn(),
}));
let container: HTMLDivElement;
let root: Root;
let current: ReturnType<typeof useMonoTranscript>;
let session: Session;
function Probe({
  value,
  enabled = true,
}: {
  value: Session;
  enabled?: boolean;
}) {
  current = useMonoTranscript(value, enabled);
  return null;
}
function turns(start: number, count: number): Block[] {
  return Array.from({ length: count }, (_, i) => ({
    id: `u${start + i}`,
    role: "user" as const,
    text: `Question ${start + i}`,
  }));
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.clearAllMocks();
  container = document.createElement("div");
  root = createRoot(container);
  session = {
    ...newSession("codex", "/tmp"),
    blocks: turns(20, 10),
    monoTranscript: { before: 20, firstBlockId: "u20" },
  };
  act(() => root.render(createElement(Probe, { value: session })));
});
afterEach(() => {
  act(() => root.unmount());
  vi.unstubAllGlobals();
});

it("loads ten older turns at each cursor without changing the writable session", async () => {
  vi.mocked(getMonoTranscriptPage)
    .mockResolvedValueOnce({
      blocks: turns(10, 10),
      before: 10,
      hasNewer: true,
    })
    .mockResolvedValueOnce({
      blocks: turns(0, 10),
      before: null,
      hasNewer: true,
    });
  const beforePrepend = vi.fn(() => expect(current.blocks).toHaveLength(10));
  await act(async () => current.loadEarlier(beforePrepend));
  expect(beforePrepend).toHaveBeenCalledOnce();
  expect(getMonoTranscriptPage).toHaveBeenLastCalledWith(session.id, {
    before: 20,
  });
  expect(current.blocks).toHaveLength(20);
  expect(current.historicalBlockIds).toEqual(
    new Set(turns(10, 10).map((block) => block.id)),
  );
  expect(session.blocks).toHaveLength(10);
  await act(async () => current.loadEarlier());
  expect(getMonoTranscriptPage).toHaveBeenLastCalledWith(session.id, {
    before: 10,
  });
  expect(current.blocks).toHaveLength(30);
  expect(current.hasEarlier).toBe(false);
  act(() => current.latest());
  expect(current.blocks).toBe(session.blocks);
  expect(current.historicalBlockIds).toBeUndefined();
  expect(current.hasEarlier).toBe(true);
});

it("keeps streamed updates authoritative while looking through history", async () => {
  vi.mocked(getMonoTranscriptPage).mockResolvedValueOnce({
    blocks: [...turns(10, 10), ...session.blocks],
    before: 10,
    hasNewer: false,
  });
  await act(async () => current.reveal("u10"));
  session = {
    ...session,
    blocks: [
      ...session.blocks.slice(0, -1),
      { ...session.blocks[9], text: "Updated stream" },
    ],
  };
  act(() => root.render(createElement(Probe, { value: session })));
  expect(current.blocks).toHaveLength(20);
  expect(current.blocks[19].text).toBe("Updated stream");
  expect(current.historicalBlockIds?.has("u29")).toBe(false);
  expect(current.historicalBlockIds?.has("u10")).toBe(true);
});

it("jumps directly to an old page instead of loading every intervening turn", async () => {
  vi.mocked(getMonoTranscriptPage).mockResolvedValueOnce({
    blocks: turns(0, 10),
    before: null,
    hasNewer: true,
  });
  await act(async () => current.reveal("u3"));
  expect(getMonoTranscriptPage).toHaveBeenCalledOnce();
  expect(getMonoTranscriptPage).toHaveBeenCalledWith(session.id, {
    aroundBlockId: "u3",
  });
  expect(current.blocks).toHaveLength(10);
  expect(current.viewingOlderPage).toBe(true);
  expect(current.historicalBlockIds).toEqual(
    new Set(current.blocks.map((block) => block.id)),
  );
  expect(session.blocks[0].id).toBe("u20");
});

it("keeps the cursor on failed loads and ignores a late page after returning to latest", async () => {
  vi.mocked(getMonoTranscriptPage).mockRejectedValueOnce(
    new Error("Disk busy"),
  );
  await act(async () => {
    await expect(current.loadEarlier()).rejects.toThrow("Disk busy");
  });
  expect(current.hasEarlier).toBe(true);
  let resolve!: (
    page: Awaited<ReturnType<typeof getMonoTranscriptPage>>,
  ) => void;
  vi.mocked(getMonoTranscriptPage).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  let pending!: Promise<void>;
  act(() => {
    pending = current.loadEarlier();
    current.latest();
  });
  await act(async () => {
    resolve({ blocks: turns(10, 10), before: 10, hasNewer: true });
    await pending;
  });
  expect(current.blocks).toBe(session.blocks);
});

it("searches archived messages and current text and leaves normal sessions alone", async () => {
  vi.mocked(findMonoTranscript).mockResolvedValue(["u3", "u20", "u29"]);
  const ids = await current.search!("Question 20");
  expect(ids).toEqual(["u3", "u20"]);
  act(() =>
    root.render(createElement(Probe, { value: session, enabled: false })),
  );
  expect(current.blocks).toBe(session.blocks);
  expect(current.search).toBeUndefined();
  expect(current.hasEarlier).toBe(false);
  expect(current.historicalBlockIds).toBeUndefined();
});
