import { buildUnifiedFile } from "./unifiedDiff";

self.onmessage = ({
  data,
}: MessageEvent<{ id: number; original: string; current: string }>) => {
  try {
    self.postMessage({
      id: data.id,
      result: buildUnifiedFile(data.original, data.current),
    });
  } catch (error) {
    self.postMessage({ id: data.id, error: String(error) });
  }
};
