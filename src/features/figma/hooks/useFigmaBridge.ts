import { useEffect, useState } from "react";
import { saveFigmaPanelEnabled } from "../../settings/model/settings";
import {
  loadFigmaBridgeStatus,
  subscribeFigmaBridge,
  type FigmaBridgeStatus,
} from "../model/figma";

export function useFigmaBridge() {
  const [status, setStatus] = useState<FigmaBridgeStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (status) saveFigmaPanelEnabled(status.enabled);
  }, [status]);

  useEffect(() => {
    let cancelled = false;
    const stop = subscribeFigmaBridge(
      (next) => {
        if (!cancelled) setStatus(next);
      },
      (message) => {
        if (!cancelled) setError(message);
      },
    );
    void loadFigmaBridgeStatus()
      .then((next) => {
        if (!cancelled) setStatus(next);
      })
      .catch((reason: unknown) => {
        if (!cancelled)
          setError(String(reason instanceof Error ? reason.message : reason));
      });
    return () => {
      cancelled = true;
      stop();
    };
  }, []);

  return { status, setStatus, error, setError };
}
