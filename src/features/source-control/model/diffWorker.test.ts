import { afterEach, expect, it, vi } from "vitest";
import { buildUnifiedFile } from "./unifiedDiff";

class TestWorker {
  static instances: TestWorker[] = [];
  sent: { id: number; original: string; current: string }[] = [];
  onmessage?: (event: {
    data: { id: number; result: ReturnType<typeof buildUnifiedFile> };
  }) => void;
  onerror?: () => void;
  terminate = vi.fn();
  constructor() {
    TestWorker.instances.push(this);
  }
  postMessage(job: { id: number; original: string; current: string }) {
    this.sent.push(job);
  }
  finish() {
    const job = this.sent.at(-1)!;
    this.onmessage?.({
      data: { id: job.id, result: buildUnifiedFile(job.original, job.current) },
    });
  }
}
afterEach(() => {
  vi.unstubAllGlobals();
  TestWorker.instances = [];
  vi.resetModules();
});

it("cancels running and queued obsolete work and gives the visible file priority", async () => {
  vi.stubGlobal("Worker", TestWorker);
  const { prepareUnifiedDiff } = await import("./diffWorker");
  const obsolete = new AbortController();
  const current = new AbortController();
  const cancelled = prepareUnifiedDiff("a", "b", obsolete.signal).catch(
    (error: Error) => error.name,
  );
  const queued = prepareUnifiedDiff("a", "c", obsolete.signal).catch(
    (error: Error) => error.name,
  );
  const background = prepareUnifiedDiff("a", "history", current.signal, 1);
  const visible = prepareUnifiedDiff("a", "visible", current.signal, 0);
  const previous = TestWorker.instances[0];
  expect(previous.sent).toHaveLength(1);
  obsolete.abort();
  expect(await cancelled).toBe("AbortError");
  expect(await queued).toBe("AbortError");
  expect(previous.terminate).toHaveBeenCalledOnce();
  const worker = TestWorker.instances[1];
  expect(worker.sent[0].current).toBe("visible");
  previous.finish(); // A late response from the terminated worker is obsolete.
  previous.onerror?.();
  expect(worker.sent).toHaveLength(1);
  expect(worker.terminate).not.toHaveBeenCalled();
  worker.finish();
  expect(await visible).toEqual(buildUnifiedFile("a", "visible"));
  expect(worker.sent[1].current).toBe("history");
  worker.finish();
  expect(await background).toEqual(buildUnifiedFile("a", "history"));
});

it("rejects every waiting request on failure and can start a fresh worker", async () => {
  vi.stubGlobal("Worker", TestWorker);
  const { prepareUnifiedDiff } = await import("./diffWorker");
  const controller = new AbortController();
  const first = prepareUnifiedDiff("a", "b", controller.signal).catch(
    (error: Error) => error.message,
  );
  const second = prepareUnifiedDiff("a", "c", controller.signal).catch(
    (error: Error) => error.message,
  );
  TestWorker.instances[0].onerror?.();
  expect(await first).toBe("Could not prepare the diff");
  expect(await second).toBe("Could not prepare the diff");
  const recovered = prepareUnifiedDiff("a", "d", controller.signal);
  TestWorker.instances[1].finish();
  expect(await recovered).toEqual(buildUnifiedFile("a", "d"));
});
