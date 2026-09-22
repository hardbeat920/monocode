import type { NewThreadBackgroundEffect } from "./appearance";

type WorkerResponse = { id: number; result?: Blob; error?: string };

let worker: Worker | null = null;
let nextRequestId = 0;
let appliedRevision = 0;
let activeObjectUrl: string | null = null;
const pending = new Map<
  number,
  { resolve: (result?: Blob) => void; reject: (error: Error) => void }
>();
const loadedSources = new Map<string, Promise<void>>();
const effectCache = new Map<string, Promise<Blob>>();

function backgroundWorker() {
  if (worker) return worker;
  worker = new Worker(
    new URL("./newThreadBackgroundEffects.worker.ts", import.meta.url),
    { type: "module" },
  );
  worker.onmessage = ({ data }: MessageEvent<WorkerResponse>) => {
    const request = pending.get(data.id);
    if (!request) return;
    pending.delete(data.id);
    if (data.error) request.reject(new Error(data.error));
    else request.resolve(data.result);
  };
  return worker;
}

function request(message: object, transfer?: Transferable[]) {
  const id = ++nextRequestId;
  return new Promise<Blob | undefined>((resolve, reject) => {
    pending.set(id, { resolve, reject });
    backgroundWorker().postMessage({ ...message, id }, transfer ?? []);
  });
}

async function ensureSource(sourceKey: string, src: string) {
  let loading = loadedSources.get(sourceKey);
  if (!loading) {
    loading = fetch(src)
      .then((response) => {
        if (!response.ok)
          throw new Error("Unable to read the background image.");
        return response.arrayBuffer();
      })
      .then(async (bytes) => {
        await request({ kind: "load", sourceKey, bytes }, [bytes]);
      });
    loadedSources.set(sourceKey, loading);
  }
  return loading;
}

export async function prepareNewThreadBackgroundEffect(
  sourceKey: string,
  src: string,
  effect: NewThreadBackgroundEffect,
  light: boolean,
) {
  const themeKey = effect === "none" || effect === "dither" ? false : light;
  const cacheKey = `${sourceKey}:${effect}:${themeKey}`;
  let prepared = effectCache.get(cacheKey);
  if (!prepared) {
    prepared = ensureSource(sourceKey, src).then(async () => {
      const result = await request({
        kind: "render",
        sourceKey,
        effect,
        light: themeKey,
      });
      if (!result) throw new Error("The background effect returned no image.");
      return result;
    });
    effectCache.set(cacheKey, prepared);
  }
  return prepared;
}

export function clearPreparedNewThreadBackground() {
  appliedRevision += 1;
  if (activeObjectUrl) URL.revokeObjectURL(activeObjectUrl);
  activeObjectUrl = null;
  document.documentElement.style.removeProperty("--chat-background-image");
  document.documentElement.classList.remove("chat-background-effect-ready");
}

export async function applyPreparedNewThreadBackground(
  sourceKey: string,
  src: string,
  effect: NewThreadBackgroundEffect,
  light: boolean,
) {
  const revision = ++appliedRevision;
  const root = document.documentElement;
  root.classList.remove("chat-background-effect-ready");
  let blob: Blob;
  try {
    blob = await prepareNewThreadBackgroundEffect(
      sourceKey,
      src,
      effect,
      light,
    );
  } catch {
    if (revision !== appliedRevision) return;
    root.style.setProperty(
      "--chat-background-image",
      `url(${JSON.stringify(src)})`,
    );
    root.classList.add("chat-background-effect-ready");
    return;
  }
  if (revision !== appliedRevision) return;
  const objectUrl = URL.createObjectURL(blob);
  if (activeObjectUrl) URL.revokeObjectURL(activeObjectUrl);
  activeObjectUrl = objectUrl;
  root.style.setProperty(
    "--chat-background-image",
    `url(${JSON.stringify(objectUrl)})`,
  );
  requestAnimationFrame(() => {
    if (revision === appliedRevision) {
      root.classList.add("chat-background-effect-ready");
    }
  });
}
