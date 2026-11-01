import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useBrowserRequest, type BrowserRequest } from "@/features/sessions/browserRequest";

const state = vi.hoisted(() => ({
  answer: vi.fn(),
  alert: vi.fn(),
}));
vi.mock("react-native", () => ({ Alert: { alert: state.alert } }));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({ answerBrowser: state.answer }),
}));

let root: ReturnType<typeof createRoot>;
let shown: BrowserRequest | null = null;
function Probe({ requested }: { requested: boolean }) {
  shown = useBrowserRequest("s1", requested);
  return null;
}
const render = (requested: boolean) =>
  act(() => root.render(<Probe requested={requested} />));
const settle = () => act(async () => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
});

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  root = createRoot(document.createElement("div"));
});
afterEach(() => act(() => root.unmount()));

it("answers the desktop and hides the request until the next one", async () => {
  state.answer.mockResolvedValue(undefined);
  render(false);
  expect(shown).toBeNull();
  render(true);
  act(() => shown!.allow());
  expect(state.answer).toHaveBeenCalledWith("s1", true);
  expect(shown!.answering).toBe(true);
  await settle();
  // Answered here, before the desktop's next poll says so.
  expect(shown).toBeNull();
  render(true);
  expect(shown).toBeNull();
  render(false);
  render(true);
  act(() => shown!.decline());
  expect(state.answer).toHaveBeenLastCalledWith("s1", false);
});

it("keeps the request and says why when the desktop refuses the answer", async () => {
  state.answer.mockRejectedValue(new Error("The agent is no longer waiting for the browser"));
  render(true);
  act(() => shown!.allow());
  await settle();
  expect(state.alert).toHaveBeenCalledWith(
    "Couldn't answer",
    "The agent is no longer waiting for the browser",
  );
  expect(shown?.answering).toBe(false);
});
