import { useEffect, useRef, useState } from "react";
import jsQR from "jsqr";

type Props = {
  onScan: (text: string) => void;
  /** Stop the camera while a connect is in flight so the UI does not freeze. */
  paused?: boolean;
};

/**
 * Camera QR scanner for the iPad pairing screen. Streams the rear camera
 * into a canvas loop and decodes with jsQR (no network, fully on-device).
 * Keeps running until unmounted: repeat decodes of the same code are
 * throttled, so a failed connect lets the user simply hold the code up
 * again. Any failure — no camera (simulator), denied permission — surfaces
 * a message and the caller falls back to manual code entry.
 */
export function QrScanner({ onScan, paused = false }: Props) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const onScanRef = useRef(onScan);
  onScanRef.current = onScan;
  const lastRef = useRef<{ value: string; at: number }>({ value: "", at: 0 });
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState(false);
  const [foreground, setForeground] = useState(
    () =>
      typeof document === "undefined" ||
      document.visibilityState === "visible",
  );

  useEffect(() => {
    const onVis = () => {
      setForeground(document.visibilityState === "visible");
    };
    document.addEventListener("visibilitychange", onVis);
    window.addEventListener("pageshow", onVis);
    window.addEventListener("focus", onVis);
    return () => {
      document.removeEventListener("visibilitychange", onVis);
      window.removeEventListener("pageshow", onVis);
      window.removeEventListener("focus", onVis);
    };
  }, []);

  useEffect(() => {
    if (paused || !foreground) {
      setActive(false);
      return;
    }
    setError(null);
    let stream: MediaStream | null = null;
    let raf = 0;
    let stopped = false;

    const stop = () => {
      stopped = true;
      cancelAnimationFrame(raf);
      stream?.getTracks().forEach((track) => track.stop());
      stream = null;
    };

    const scan = () => {
      if (stopped) return;
      const video = videoRef.current;
      const canvas = canvasRef.current;
      if (video && canvas && video.readyState === video.HAVE_ENOUGH_DATA) {
        const context = canvas.getContext("2d", { willReadFrequently: true });
        if (context) {
          const width = video.videoWidth;
          const height = video.videoHeight;
          if (width > 0 && height > 0) {
            canvas.width = width;
            canvas.height = height;
            context.drawImage(video, 0, 0, width, height);
            try {
              const found = jsQR(
                context.getImageData(0, 0, width, height).data,
                width,
                height,
              );
              if (found?.data) {
                const now = Date.now();
                // Throttle repeats: a failed connect keeps the loop alive,
                // so the same code re-fires every few seconds for retry.
                if (
                  found.data !== lastRef.current.value ||
                  now - lastRef.current.at > 3000
                ) {
                  lastRef.current = { value: found.data, at: now };
                  onScanRef.current(found.data);
                }
              }
            } catch {
              // A corrupt frame must not kill the loop.
            }
          }
        }
      }
      raf = requestAnimationFrame(scan);
    };

    (async () => {
      try {
        if (
          typeof navigator === "undefined" ||
          !navigator.mediaDevices?.getUserMedia
        ) {
          setError("No camera available on this device.");
          return;
        }
        stream = await navigator.mediaDevices.getUserMedia({
          video: { facingMode: "environment" },
          audio: false,
        });
        if (stopped) {
          stop();
          return;
        }
        const video = videoRef.current;
        if (!video) return;
        video.srcObject = stream;
        await video.play().catch(() => undefined);
        setActive(true);
        raf = requestAnimationFrame(scan);
      } catch {
        if (!stopped) {
          setError(
            "Camera is blocked — allow access in Settings, or enter the code manually.",
          );
        }
      }
    })();

    return stop;
  }, [paused, foreground]);

  if (error) {
    return <p className="py-3 text-[12px] text-content/50">{error}</p>;
  }

  return (
    <div className="overflow-hidden rounded-lg border border-content/10">
      <video
        ref={videoRef}
        playsInline
        muted
        className="aspect-[4/3] w-full bg-black object-cover"
      />
      <canvas ref={canvasRef} className="hidden" aria-hidden />
      <p className="px-3 py-2 text-[12px] text-content/50">
        {paused
          ? "Connecting…"
          : active
            ? "Point at the pairing code on the Mac…"
            : "Starting camera…"}
      </p>
    </div>
  );
}
