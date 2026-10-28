import { createHash, randomUUID } from "node:crypto";
import { readFile, rename, rm, writeFile } from "node:fs/promises";
import { get } from "node:https";
import { setTimeout as waitFor } from "node:timers/promises";

const maxAttempts = 4;
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

class DownloadError extends Error {
  constructor(message, { cause, retryable = false } = {}) {
    super(message, { cause });
    this.retryable = retryable;
  }
}

// Use Node's HTTPS client as an independent transport when Windows fetch stalls.
// The caller's abort signal bounds both the request and its response stream.
export function fetchViaHttps(url, { signal }, getImpl = get) {
  return new Promise((resolve, reject) => {
    getImpl(url, { signal }, (response) => {
      resolve({
        ok: response.statusCode >= 200 && response.statusCode < 300,
        status: response.statusCode,
        statusText: response.statusMessage,
        body: { cancel: async () => response.destroy() },
        async arrayBuffer() {
          const chunks = [];
          for await (const chunk of response) chunks.push(chunk);
          return Buffer.concat(chunks);
        },
      });
    }).on("error", reject);
  });
}

async function downloadAttempt(url, fetchImpl, timeoutMs) {
  const controller = new AbortController();
  let phase = "waiting for response headers";
  // Keep this deadline active until the entire response body has been read.
  const timer = setTimeout(() => {
    controller.abort(
      new DOMException("Runtime download timed out", "TimeoutError"),
    );
  }, timeoutMs);
  try {
    const response = await fetchImpl(url, { signal: controller.signal });
    phase = "reading response body";
    if (!response.ok) {
      // Release the failed response without waiting for its body to download.
      void response.body?.cancel().catch(() => {});
      throw new DownloadError(
        `HTTP ${response.status} ${response.statusText}`.trim(),
        {
          retryable:
            response.status === 408 ||
            response.status === 429 ||
            response.status >= 500,
        },
      );
    }
    return Buffer.from(await response.arrayBuffer());
  } catch (error) {
    if (error instanceof DownloadError) throw error;
    const detail = controller.signal.aborted
      ? `Timed out after ${timeoutMs}ms ${phase}`
      : `${error.name}: ${error.message}${error.cause ? ` (${error.cause.code ?? error.cause.message})` : ""}`;
    throw new DownloadError(detail, { cause: error, retryable: true });
  } finally {
    clearTimeout(timer);
  }
}

export async function ensureRuntimeArchive(
  { archive, url, checksum },
  {
    fetchImpl = fetch,
    wait = waitFor,
    timeoutMs = 120_000,
    log = console.warn,
    fallbackFetchImpl = process.platform === "win32" ? fetchViaHttps : null,
  } = {},
) {
  let cached;
  try {
    cached = await readFile(archive);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  if (cached) {
    if (sha256(cached) === checksum) return archive;
    // A restored CI cache is untrusted until checked against the pinned hash.
    await rm(archive);
    log(`Discarded Node runtime cache with checksum mismatch: ${archive}`);
  }

  let bytes;
  let transport = fetchImpl;
  let transportName = "fetch";
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    try {
      bytes = await downloadAttempt(url, transport, timeoutMs);
      break;
    } catch (error) {
      const message = `Node runtime download failed: ${url} (attempt ${attempt}/${maxAttempts}, ${transportName}): ${error.message}`;
      if (!error.retryable || attempt === maxAttempts) {
        throw new Error(message, { cause: error });
      }
      if (fallbackFetchImpl) {
        transport = fallbackFetchImpl;
        transportName = "Node HTTPS";
      }
      const delayMs = 1_000 * 2 ** (attempt - 1);
      log(`${message}; retrying in ${delayMs}ms using ${transportName}`);
      await wait(delayMs);
    }
  }
  const actual = sha256(bytes);
  if (actual !== checksum) {
    throw new Error(
      `Node runtime checksum mismatch: ${url}; expected ${checksum}, got ${actual}`,
    );
  }

  // Publish only complete, verified archives. Interrupted writes cannot become hits.
  const temporary = `${archive}.${randomUUID()}.tmp`;
  try {
    await writeFile(temporary, bytes, { flag: "wx" });
    await rename(temporary, archive);
  } finally {
    await rm(temporary, { force: true });
  }
  return archive;
}
