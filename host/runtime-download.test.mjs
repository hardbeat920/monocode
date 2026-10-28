import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createServer, get } from "node:http";
import {
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { ensureRuntimeArchive, fetchViaHttps } from "./runtime-download.mjs";

const bytes = Buffer.from("verified runtime fixture");
const checksum = createHash("sha256").update(bytes).digest("hex");
const url =
  "https://nodejs.org/dist/v24.21.0/node-v24.21.0-darwin-arm64.tar.gz";
const delays = [1_000, 2_000, 4_000];

async function fixture(t) {
  const cache = await mkdtemp(join(tmpdir(), "monocode-runtime-"));
  t.after(() => rm(cache, { recursive: true, force: true }));
  const archive = join(cache, "runtime.tar.gz");
  const wait = t.mock.fn(async () => {});
  const log = t.mock.fn();
  return {
    archive,
    wait,
    log,
    load: (fetchImpl, options = {}) =>
      ensureRuntimeArchive(
        { archive, url, checksum },
        { fetchImpl, wait, log, fallbackFetchImpl: null, ...options },
      ),
    async assertCached() {
      assert.deepEqual(await readFile(archive), bytes);
      assert.deepEqual(await readdir(cache), ["runtime.tar.gz"]);
    },
    async assertEmpty() {
      assert.deepEqual(await readdir(cache), []);
    },
    assertDelays(expected = delays) {
      assert.deepEqual(
        wait.mock.calls.map(({ arguments: args }) => args[0]),
        expected,
      );
    },
  };
}

function assertFailure(error, attempt, reason) {
  assert.ok(error.message.includes(url), error.message);
  assert.ok(error.message.includes(`attempt ${attempt}/4,`), error.message);
  assert.match(error.message, reason);
  assert.ok(error.cause instanceof Error);
  return true;
}

test("retries network, HTTP and interrupted body failures with bounded backoff before caching verified bytes", async (t) => {
  const f = await fixture(t);
  const signals = [];
  let cancelled = false;
  const fetchImpl = t.mock.fn(async (_url, { signal }) => {
    assert.equal(_url, url);
    signals.push(signal);
    switch (signals.length) {
      case 1:
        throw new TypeError("fetch failed", {
          cause: Object.assign(new Error("socket closed"), {
            code: "ECONNRESET",
          }),
        });
      case 2:
        return new Response(
          new ReadableStream({
            cancel() {
              cancelled = true;
            },
          }),
          { status: 503 },
        );
      case 3:
        return new Response(
          new ReadableStream({
            start(controller) {
              controller.enqueue(Buffer.from("partial runtime"));
              controller.error(new TypeError("terminated"));
            },
          }),
        );
      default:
        await f.assertEmpty();
        return new Response(bytes);
    }
  });
  assert.equal(await f.load(fetchImpl), f.archive);
  assert.equal(fetchImpl.mock.callCount(), 4);
  assert.equal(
    new Set(signals).size,
    4,
    "each attempt has its own timeout signal",
  );
  assert.ok(cancelled, "failed HTTP responses are cancelled");
  f.assertDelays();
  await f.assertCached();
  assert.equal(f.log.mock.callCount(), 3);
  for (const [
    i,
    {
      arguments: [message],
    },
  ] of f.log.mock.calls.entries()) {
    assert.ok(message.includes(url));
    assert.ok(message.includes(`attempt ${i + 1}/4,`));
    assert.ok(message.includes(`retrying in ${delays[i]}ms`));
  }
  assert.match(f.log.mock.calls[0].arguments[0], /ECONNRESET/);
});

for (const status of [408, 429, 500, 502, 503, 504]) {
  test(`HTTP ${status} stops after four attempts without caching`, async (t) => {
    const f = await fixture(t);
    const fetchImpl = t.mock.fn(
      async () => new Response("unavailable", { status }),
    );
    await assert.rejects(f.load(fetchImpl), (error) =>
      assertFailure(error, 4, new RegExp(`HTTP ${status}`)),
    );
    assert.equal(fetchImpl.mock.callCount(), 4);
    f.assertDelays();
    assert.equal(f.log.mock.callCount(), 3);
    await f.assertEmpty();
  });
}

test("network failure exhaustion preserves the final cause and leaves no cache", async (t) => {
  const f = await fixture(t);
  const cause = new TypeError("fetch failed", {
    cause: Object.assign(new Error("connection reset"), { code: "ECONNRESET" }),
  });
  const fetchImpl = t.mock.fn(async () => {
    throw cause;
  });
  await assert.rejects(f.load(fetchImpl), (error) => {
    assertFailure(error, 4, /ECONNRESET/);
    assert.equal(error.cause.cause, cause);
    return true;
  });
  assert.equal(fetchImpl.mock.callCount(), 4);
  f.assertDelays();
  await f.assertEmpty();
});

for (const status of [400, 401, 403, 404]) {
  test(`HTTP ${status} fails immediately without caching`, async (t) => {
    const f = await fixture(t);
    const fetchImpl = t.mock.fn(
      async () => new Response("permanent failure", { status }),
    );
    await assert.rejects(f.load(fetchImpl), (error) =>
      assertFailure(error, 1, new RegExp(`HTTP ${status}`)),
    );
    assert.equal(fetchImpl.mock.callCount(), 1);
    f.assertDelays([]);
    await f.assertEmpty();
  });
}

test("the 120 second deadline includes both fetching headers and reading the body, then retries successfully", async (t) => {
  const f = await fixture(t);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const headers = Promise.withResolvers();
  const fetching = Promise.withResolvers();
  const reading = Promise.withResolvers();
  const signals = [];
  const fetchImpl = t.mock.fn(async (_url, { signal }) => {
    signals.push(signal);
    if (signals.length > 1) return new Response(bytes);
    fetching.resolve();
    await headers.promise;
    return {
      ok: true,
      arrayBuffer() {
        reading.resolve();
        return new Promise((_resolve, reject) => {
          signal.addEventListener("abort", () => reject(signal.reason), {
            once: true,
          });
        });
      },
    };
  });
  const operation = f.load(fetchImpl);
  await fetching.promise;
  t.mock.timers.tick(70_000);
  assert.equal(signals[0].aborted, false);
  headers.resolve();
  await reading.promise;
  t.mock.timers.tick(49_999);
  assert.equal(signals[0].aborted, false);
  t.mock.timers.tick(1);
  assert.equal(signals[0].aborted, true);
  assert.equal(signals[0].reason.name, "TimeoutError");
  assert.equal(await operation, f.archive);
  assert.equal(fetchImpl.mock.callCount(), 2);
  f.assertDelays([1_000]);
  assert.match(
    f.log.mock.calls[0].arguments[0],
    /Timed out after 120000ms reading response body/,
  );
  t.mock.timers.tick(120_000);
  assert.equal(
    signals[1].aborted,
    false,
    "the successful attempt's timer was cleared",
  );
  await f.assertCached();
});

for (const phase of ["headers", "body"]) {
  test(`persistent ${phase} timeouts exhaust four fresh deadlines without caching`, async (t) => {
    const f = await fixture(t);
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const started = Array.from({ length: 4 }, () => Promise.withResolvers());
    const signals = [];
    const fetchImpl = t.mock.fn(async (_url, { signal }) => {
      signals.push(signal);
      const stall = () => {
        started[signals.length - 1].resolve();
        return new Promise((_resolve, reject) => {
          signal.addEventListener("abort", () => reject(signal.reason), {
            once: true,
          });
        });
      };
      return phase === "headers" ? stall() : { ok: true, arrayBuffer: stall };
    });
    const operation = assert.rejects(f.load(fetchImpl), (error) => {
      assertFailure(
        error,
        4,
        new RegExp(
          `Timed out after 120000ms ${phase === "headers" ? "waiting for response headers" : "reading response body"}`,
        ),
      );
      assert.equal(error.cause.cause.name, "TimeoutError");
      return true;
    });
    for (let i = 0; i < 4; i++) {
      await started[i].promise;
      assert.equal(signals[i].aborted, false);
      t.mock.timers.tick(120_000);
      assert.equal(signals[i].aborted, true);
    }
    await operation;
    assert.equal(fetchImpl.mock.callCount(), 4);
    assert.equal(new Set(signals).size, 4);
    f.assertDelays();
    await f.assertEmpty();
  });
}

test("a verified cache hit works offline without fetching or waiting", async (t) => {
  const f = await fixture(t);
  await writeFile(f.archive, bytes);
  const fetchImpl = t.mock.fn(async () => {
    throw new Error("network must not be used");
  });
  assert.equal(await f.load(fetchImpl), f.archive);
  assert.equal(fetchImpl.mock.callCount(), 0);
  assert.equal(f.log.mock.callCount(), 0);
  f.assertDelays([]);
  await f.assertCached();
});

test("every cache hit is checked, and corrupt cached archives are replaced with verified downloads", async (t) => {
  const f = await fixture(t);
  const fetchImpl = t.mock.fn(async () => {
    await f.assertEmpty();
    return new Response(bytes);
  });
  await writeFile(f.archive, "corrupt restored cache");
  await f.load(fetchImpl);
  await f.assertCached();
  await f.load(fetchImpl);
  assert.equal(fetchImpl.mock.callCount(), 1);
  await writeFile(f.archive, "truncated cache");
  await f.load(fetchImpl);
  assert.equal(fetchImpl.mock.callCount(), 2);
  assert.equal(f.log.mock.callCount(), 2);
  assert.match(
    f.log.mock.calls[0].arguments[0],
    /Discarded Node runtime cache with checksum mismatch/,
  );
  f.assertDelays([]);
  await f.assertCached();
});

test("checksum mismatches fail immediately and leave neither invalid cached bytes nor temporary files", async (t) => {
  const f = await fixture(t);
  await writeFile(f.archive, "bad cached bytes");
  const fetchImpl = t.mock.fn(
    async () => new Response("invalid downloaded bytes"),
  );
  await assert.rejects(f.load(fetchImpl), (error) => {
    assert.match(error.message, /Node runtime checksum mismatch/);
    assert.ok(error.message.includes(url));
    assert.ok(error.message.includes(`expected ${checksum}`));
    assert.match(error.message, /got [a-f0-9]{64}/);
    return true;
  });
  assert.equal(fetchImpl.mock.callCount(), 1);
  f.assertDelays([]);
  await f.assertEmpty();
});

test("an invalid cache is removed even when the replacement download fails", async (t) => {
  const f = await fixture(t);
  await writeFile(f.archive, "corrupt cached runtime");
  await assert.rejects(
    f.load(async () => {
      await f.assertEmpty();
      return new Response("missing", { status: 404 });
    }),
    /HTTP 404/,
  );
  await f.assertEmpty();
});

test("cache filesystem errors are surfaced without downloading", async (t) => {
  const f = await fixture(t);
  await mkdir(f.archive);
  const fetchImpl = t.mock.fn();
  await assert.rejects(f.load(fetchImpl), { code: "EISDIR" });
  assert.equal(fetchImpl.mock.callCount(), 0);
  f.assertDelays([]);
});

// HTTPS uses Node's HTTP response streams. Exercise those real streams over
// loopback, injecting only the connection so tests need no TLS keys or internet.
async function nativeTransport(t, handler) {
  const server = createServer(handler);
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  t.after(
    () =>
      new Promise((resolve) => {
        server.closeAllConnections();
        server.close(resolve);
      }),
  );
  return (requestedUrl, options) =>
    fetchViaHttps(
      requestedUrl,
      options,
      (actualUrl, requestOptions, onResponse) => {
        assert.equal(actualUrl, url, "fallback uses the same pinned URL");
        return get(
          `http://127.0.0.1:${server.address().port}`,
          requestOptions,
          onResponse,
        );
      },
    );
}

for (const phase of ["headers", "body"]) {
  test(
    `Windows fallback recovers from a fetch ${phase} timeout through the native transport and verifies its cache`,
    { timeout: 10_000 },
    async (t) => {
      const f = await fixture(t);
      let requests = 0;
      const fallbackFetchImpl = await nativeTransport(
        t,
        (_request, response) => {
          requests++;
          response.end(bytes);
        },
      );
      t.mock.timers.enable({ apis: ["setTimeout"] });
      const started = Promise.withResolvers();
      const fetchImpl = t.mock.fn(async (_url, { signal }) => {
        const stall = () => {
          started.resolve();
          return new Promise((_resolve, reject) =>
            signal.addEventListener("abort", () => reject(signal.reason), {
              once: true,
            }),
          );
        };
        return phase === "headers" ? stall() : { ok: true, arrayBuffer: stall };
      });
      const operation = f.load(fetchImpl, { fallbackFetchImpl });
      await started.promise;
      t.mock.timers.tick(120_000);
      await operation;
      assert.equal(fetchImpl.mock.callCount(), 1);
      assert.equal(requests, 1);
      f.assertDelays([1_000]);
      assert.match(f.log.mock.calls[0].arguments[0], /using Node HTTPS/);
      await f.assertCached();
      await f.load(fetchImpl, { fallbackFetchImpl });
      assert.equal(
        requests,
        1,
        "the verified fallback result is reused offline",
      );
      assert.equal(fetchImpl.mock.callCount(), 1);
    },
  );
}

for (const phase of ["headers", "body"]) {
  test(
    `native transport abort closes a stalled ${phase} connection`,
    { timeout: 10_000 },
    async (t) => {
      const arrived = Promise.withResolvers();
      const closed = Promise.withResolvers();
      const transport = await nativeTransport(t, (_request, response) => {
        response.once("close", closed.resolve);
        if (phase === "body") {
          response.writeHead(200, { "content-length": bytes.length + 100 });
          response.write(bytes);
        }
        arrived.resolve();
      });
      const controller = new AbortController();
      const headers = transport(url, { signal: controller.signal });
      const operation =
        phase === "body" ? (await headers).arrayBuffer() : headers;
      const rejected = assert.rejects(operation, (error) => {
        // Aborting the request may surface as a reset on its response stream.
        assert.match(error.code, /^(ABORT_ERR|ECONNRESET)$/);
        return true;
      });
      await arrived.promise;
      controller.abort(new DOMException("test deadline", "TimeoutError"));
      await rejected;
      await closed.promise;
    },
  );
}

test(
  "native transport retries server errors within the original four-attempt budget",
  { timeout: 10_000 },
  async (t) => {
    const f = await fixture(t);
    let requests = 0;
    const fallbackFetchImpl = await nativeTransport(t, (_request, response) => {
      if (++requests === 1) response.writeHead(503).end("unavailable");
      else response.end(bytes);
    });
    const fetchImpl = t.mock.fn(async () => {
      throw new TypeError("fetch failed");
    });
    await f.load(fetchImpl, { fallbackFetchImpl });
    assert.equal(fetchImpl.mock.callCount(), 1);
    assert.equal(requests, 2);
    f.assertDelays([1_000, 2_000]);
    assert.match(
      f.log.mock.calls[1].arguments[0],
      /attempt 2\/4, Node HTTPS.*HTTP 503/,
    );
    await f.assertCached();
  },
);

test(
  "truncated native response bodies exhaust the remaining attempts and are never cached",
  { timeout: 10_000 },
  async (t) => {
    const f = await fixture(t);
    let requests = 0;
    const fallbackFetchImpl = await nativeTransport(t, (_request, response) => {
      requests++;
      response.writeHead(200, {
        "content-length": bytes.length,
        connection: "close",
      });
      response.end(bytes.subarray(0, 3));
    });
    const fetchImpl = t.mock.fn(async () => {
      throw new TypeError("fetch failed");
    });
    await assert.rejects(f.load(fetchImpl, { fallbackFetchImpl }), (error) =>
      assertFailure(error, 4, /Node HTTPS.*aborted/),
    );
    assert.equal(fetchImpl.mock.callCount(), 1);
    assert.equal(requests, 3);
    f.assertDelays();
    await f.assertEmpty();
  },
);

test(
  "native fallback bytes must match the pinned checksum before caching",
  { timeout: 10_000 },
  async (t) => {
    const f = await fixture(t);
    let requests = 0;
    const fallbackFetchImpl = await nativeTransport(t, (_request, response) => {
      requests++;
      response.end("invalid runtime");
    });
    await assert.rejects(
      f.load(
        async () => {
          throw new TypeError("fetch failed");
        },
        { fallbackFetchImpl },
      ),
      /Node runtime checksum mismatch/,
    );
    assert.equal(requests, 1);
    f.assertDelays([1_000]);
    await f.assertEmpty();
  },
);

test("permanent HTTP and checksum failures never invoke a fallback", async (t) => {
  const f = await fixture(t);
  const fallbackFetchImpl = t.mock.fn();
  await assert.rejects(
    f.load(async () => new Response("missing", { status: 404 }), {
      fallbackFetchImpl,
    }),
    /HTTP 404/,
  );
  await assert.rejects(
    f.load(async () => new Response("invalid runtime"), { fallbackFetchImpl }),
    /Node runtime checksum mismatch/,
  );
  assert.equal(fallbackFetchImpl.mock.callCount(), 0);
  f.assertDelays([]);
  await f.assertEmpty();
});

for (const phase of ["headers", "body"]) {
  test(
    `stalled native ${phase} exhaust the shared attempt and timeout budget`,
    { timeout: 10_000 },
    async (t) => {
      const f = await fixture(t);
      const started = Array.from({ length: 3 }, () => Promise.withResolvers());
      let requests = 0;
      const transport = await nativeTransport(t, (_request, response) => {
        if (phase === "headers") started[requests++].resolve();
        else
          response
            .writeHead(200, { "content-length": bytes.length + 100 })
            .write(bytes);
      });
      const fallbackFetchImpl = async (...args) => {
        const response = await transport(...args);
        return {
          ...response,
          arrayBuffer() {
            started[requests++].resolve();
            return response.arrayBuffer();
          },
        };
      };
      t.mock.timers.enable({ apis: ["setTimeout"] });
      const fetchImpl = t.mock.fn(async () => {
        throw new TypeError("fetch failed");
      });
      const operation = assert.rejects(
        f.load(fetchImpl, { fallbackFetchImpl }),
        (error) => {
          assertFailure(
            error,
            4,
            new RegExp(
              `Node HTTPS.*Timed out after 120000ms ${phase === "headers" ? "waiting for response headers" : "reading response body"}`,
            ),
          );
          return true;
        },
      );
      for (let i = 0; i < 3; i++) {
        await started[i].promise;
        t.mock.timers.tick(120_000);
      }
      await operation;
      assert.equal(fetchImpl.mock.callCount(), 1);
      assert.equal(requests, 3);
      f.assertDelays();
      await f.assertEmpty();
    },
  );
}
