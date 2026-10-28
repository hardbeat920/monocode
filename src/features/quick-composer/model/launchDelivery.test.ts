import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { launchReceiver } from "./launchDelivery";

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});

const request = { prompt: "go", cwd: "/repo", harness: "codex", reveal: false };
function setup() {
  const queue = [
    { id: "first", request },
    { id: "second", request },
  ];
  const accept = vi.fn(async () => {});
  const ack = vi.fn(async (id: string) => {
    const index = queue.findIndex((item) => item.id === id);
    if (index >= 0) queue.splice(index, 1);
  });
  const options = {
    take: vi.fn(async (): Promise<unknown> => queue[0] ?? null),
    accept,
    ack,
    disposed: () => false,
    accepted: new Set<string>(),
    accepting: new Map<string, Promise<void>>(),
  };
  return { queue, options, receive: launchReceiver(options) };
}
it("serializes mount/focus/event triggers and acknowledges each accepted launch", async () => {
  const { receive, options, queue } = setup();
  await Promise.all([receive(), receive(), receive()]);
  expect(options.accept.mock.calls.map((call) => call[1])).toEqual([
    "first",
    "second",
  ]);
  expect(queue).toHaveLength(0);
});
it("retries a failed acceptance without another event", async () => {
  const { receive, options, queue } = setup();
  options.accept.mockRejectedValueOnce(new Error("not ready"));
  await expect(receive()).rejects.toThrow("not ready");
  expect(options.ack).not.toHaveBeenCalled();
  expect(queue).toHaveLength(2);
  await vi.advanceTimersByTimeAsync(250);
  expect(options.accept.mock.calls.map((call) => call[1])).toEqual([
    "first",
    "first",
    "second",
  ]);
  expect(queue).toHaveLength(0);
  expect(vi.getTimerCount()).toBe(0);
});
it("retries a lost ACK without another event or accepting the launch twice", async () => {
  const { receive, options, queue } = setup();
  queue.splice(1);
  options.ack.mockRejectedValueOnce(new Error("IPC interrupted"));
  await expect(receive()).rejects.toThrow("IPC interrupted");
  expect(queue).toHaveLength(1);
  expect(options.accepted.has("first")).toBe(true);
  await vi.advanceTimersByTimeAsync(250);
  expect(options.accept).toHaveBeenCalledTimes(1);
  expect(options.ack).toHaveBeenCalledTimes(2);
  expect(queue).toHaveLength(0);
  expect(options.accepted.size).toBe(0);
  expect(vi.getTimerCount()).toBe(0);
});
it.each([
  { id: "first", request: { prompt: "malformed" } },
  { id: 123, request },
  { id: "", request },
  "invalid envelope",
])(
  "does not automatically retry or acknowledge invalid payloads: %j",
  async (value) => {
    const { receive, options } = setup();
    options.take.mockResolvedValue(value);
    await expect(receive()).rejects.toThrow("Invalid queued session");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(options.take).toHaveBeenCalledTimes(1);
    expect(options.accept).not.toHaveBeenCalled();
    expect(options.ack).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  },
);
it("stops automatic retries if a subsequent take returns an invalid payload", async () => {
  const { receive, options } = setup();
  options.take
    .mockRejectedValueOnce(new Error("IPC interrupted"))
    .mockResolvedValue({ id: "first", request: {} });
  await expect(receive()).rejects.toThrow("IPC interrupted");
  await vi.advanceTimersByTimeAsync(60_000);
  expect(options.take).toHaveBeenCalledTimes(2);
  expect(options.accept).not.toHaveBeenCalled();
  expect(options.ack).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
});
it("caps exponential backoff and resets it after successful delivery", async () => {
  const { receive, options, queue } = setup();
  options.accept.mockRejectedValue(new Error("not ready"));
  await expect(receive()).rejects.toThrow("not ready");
  let attempts = 1;
  for (const delay of [250, 500, 1000, 2000, 4000, 8000, 16000, 30000, 30000]) {
    await vi.advanceTimersByTimeAsync(delay - 1);
    expect(options.accept).toHaveBeenCalledTimes(attempts);
    await vi.advanceTimersByTimeAsync(1);
    expect(options.accept).toHaveBeenCalledTimes(++attempts);
    expect(vi.getTimerCount()).toBe(1);
  }
  options.accept.mockResolvedValue(undefined);
  await vi.advanceTimersByTimeAsync(30000);
  expect(queue).toHaveLength(0);
  expect(vi.getTimerCount()).toBe(0);

  queue.push({ id: "third", request });
  options.accept.mockRejectedValueOnce(new Error("not ready"));
  await expect(receive()).rejects.toThrow("not ready");
  await vi.advanceTimersByTimeAsync(250);
  expect(queue).toHaveLength(0);
  expect(vi.getTimerCount()).toBe(0);
});
it("lets an event retry immediately without leaving a redundant timer", async () => {
  const { receive, options, queue } = setup();
  options.take.mockRejectedValueOnce(new Error("IPC interrupted"));
  await expect(receive()).rejects.toThrow("IPC interrupted");
  expect(vi.getTimerCount()).toBe(1);
  await Promise.all([receive(), receive()]);
  expect(queue).toHaveLength(0);
  expect(vi.getTimerCount()).toBe(0);
  expect(options.accept).toHaveBeenCalledTimes(2);
});
it("cancels pending retries when disposed", async () => {
  const { receive, options } = setup();
  options.accept.mockRejectedValueOnce(new Error("not ready"));
  await expect(receive()).rejects.toThrow("not ready");
  receive.dispose();
  expect(vi.getTimerCount()).toBe(0);
  await vi.advanceTimersByTimeAsync(60_000);
  await receive();
  expect(options.take).toHaveBeenCalledTimes(1);
});
it("does not schedule retries for an in-flight failure after disposal", async () => {
  const { receive, options } = setup();
  options.accept.mockImplementationOnce(async () => {
    receive.dispose();
    throw new Error("not ready");
  });
  await expect(receive()).rejects.toThrow("not ready");
  expect(vi.getTimerCount()).toBe(0);
});
it("keeps the launch queued if the component unmounts during a take", async () => {
  const { receive, options, queue } = setup();
  options.take.mockImplementationOnce(async () => {
    options.disposed = () => true;
    return queue[0];
  });
  await receive();
  expect(options.accept).not.toHaveBeenCalled();
  expect(options.ack).not.toHaveBeenCalled();
  expect(queue).toHaveLength(2);
});
it("shares an in-flight acceptance across effect remounts", async () => {
  const { receive, options, queue } = setup();
  let resolve!: () => void;
  options.accept.mockImplementationOnce(
    () =>
      new Promise<void>((done) => {
        resolve = done;
      }),
  );
  const first = receive();
  await Promise.resolve();
  const second = launchReceiver(options)();
  await Promise.resolve();
  expect(options.ack).not.toHaveBeenCalled();
  resolve();
  await Promise.all([first, second]);
  expect(options.accept).toHaveBeenCalledTimes(2);
  expect(queue).toHaveLength(0);
});
