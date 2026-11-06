import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { TOAST_MS, useToast } from "@/ui/useToast";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let toast: ReturnType<typeof useToast>;
function Probe() {
  toast = useToast();
  return null;
}

beforeEach(async () => {
  vi.useFakeTimers();
  const root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
});
afterEach(() => {
  vi.useRealTimers();
});

const after = (ms: number) => act(async () => void vi.advanceTimersByTime(ms));

it("clears a toast with nothing to tap sooner than one with Undo", async () => {
  await act(async () => toast.show("Link copied"));
  await after(2999);
  expect(toast.toast?.message).toBe("Link copied");
  await after(1);
  expect(toast.toast).toBeNull();

  await act(async () => toast.show("Archived", () => {}));
  await after(TOAST_MS - 1);
  expect(toast.toast?.message).toBe("Archived");
  await after(1);
  expect(toast.toast).toBeNull();
});

it("lets a newer toast replace the one showing and restart the clock", async () => {
  await act(async () => toast.show("First"));
  await after(2000);
  await act(async () => toast.show("Second"));
  await after(2000);
  expect(toast.toast?.message).toBe("Second");
});
