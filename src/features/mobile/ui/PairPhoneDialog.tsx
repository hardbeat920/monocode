import { useEffect, useState } from "react";
import { Modal } from "../../../shared/ui/Modal";
import { PixelMascot } from "../../projects/ui/PixelMascot";
import { TAB_GROUP_COLORS } from "../../workspace/model/tabGroups";
import { Check, Clock, Copy, RefreshCw } from "../../../shared/ui/icons";

/** Mascots fanning out from the middle, nearest first, each side in turn. */
const LINEUP = [
  "robot",
  "cat",
  "ghost",
  "frog",
  "invader",
  "crab",
  "dino",
  "mushroom",
  "rocket",
] as const;

const iconButton =
  "grid size-7 place-items-center rounded-md text-content/45 hover:bg-content/8 hover:text-content disabled:opacity-40";

/** Shows a one-use pairing code for the MonoCode mobile app to scan. */
export function PairPhoneDialog({
  image,
  url,
  left,
  busy,
  onNewCode,
  onError,
  onClose,
}: {
  /** QR code data URL; empty while it renders. */
  image: string;
  url: string;
  /** Seconds until the code expires. */
  left: number;
  busy: boolean;
  onNewCode: () => void;
  onError: (reason: string) => void;
  onClose: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const expired = left === 0;

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);

  const copy = () =>
    void navigator.clipboard
      .writeText(url)
      .then(() => setCopied(true))
      .catch((reason) => onError(String(reason)));

  return (
    <Modal
      title="Pair a phone"
      size="md"
      fitViewport
      minimalHeader
      hideClose
      onClose={onClose}
    >
      <MascotLineup />

      <div className="flex flex-col items-center px-6 pb-6">
        <h3 className="text-[22px] font-semibold tracking-tight text-content">
          Connect a phone to this desktop
        </h3>
        <p className="mt-1.5 text-center text-[12px] leading-relaxed text-content/50">
          Open MonoCode on your phone, tap{" "}
          <span className="font-medium text-content/80">
            Connect to desktop
          </span>
          , and scan the code.
        </p>

        <div className="mt-5 flex w-fit flex-col items-center">
          <div className="relative size-96 overflow-hidden rounded-2xl bg-white p-3.5 shadow-sm">
            {image ? (
              <img
                src={image}
                className={`size-full [image-rendering:pixelated] ${expired ? "opacity-15 blur-[2px]" : ""}`}
                alt="Scan this one-use pairing code in the MonoCode mobile app"
              />
            ) : null}
            {expired && (
              <div className="absolute inset-0 flex items-center justify-center">
                <button
                  className="rounded-lg bg-black px-3 py-2 text-[13px] font-medium text-white disabled:opacity-40"
                  disabled={busy}
                  onClick={onNewCode}
                >
                  New code
                </button>
              </div>
            )}
          </div>

          <div className="mt-1.5 flex h-10 w-full items-center gap-2 pl-1 text-[12px] text-content/50">
            <Clock className="size-3.5 shrink-0" />
            <span className="flex-1 tabular-nums" role="status">
              {expired
                ? "Code expired"
                : `Expires in ${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`}
            </span>
            <button
              className={iconButton}
              disabled={expired}
              aria-label="Copy pairing link"
              title="Copy pairing link"
              onClick={copy}
            >
              {copied ? (
                <Check className="size-3.5 text-emerald-500" />
              ) : (
                <Copy className="size-3.5" />
              )}
            </button>
            <button
              className={iconButton}
              disabled={busy}
              aria-label="Make a new code"
              title="Make a new code"
              onClick={onNewCode}
            >
              <RefreshCw className="size-3.5" />
            </button>
          </div>
        </div>

        <p className="mt-4 text-center text-[12px] leading-relaxed text-content/45">
          Can’t scan? Copy the link and open it on your phone. Each code works
          once.
        </p>
      </div>
    </Modal>
  );
}

/** The mascot crew, gathered round a glow, standing in for an illustration. */
function MascotLineup() {
  return (
    <div aria-hidden className="relative h-24 overflow-hidden">
      <div className="absolute bottom-3.5 left-1/2 h-3 w-56 -translate-x-1/2 bg-[radial-gradient(closest-side,color-mix(in_srgb,var(--color-content)_12%,transparent),transparent)]" />
      {LINEUP.map((name, i) => {
        // 0 is the middle; odd steps go left, even steps go right.
        const step = Math.ceil(i / 2);
        const side = i % 2 === 1 ? -1 : 1;
        const size = 40 - step * 6;
        return (
          <div
            key={name}
            className="absolute left-1/2"
            style={{
              bottom: 18 + step * 2,
              marginLeft: side * step * 42 - size / 2,
              width: size,
              height: size,
              opacity: 1 - step * 0.17,
              zIndex: 10 - step,
            }}
          >
            <PixelMascot
              name={name}
              color={TAB_GROUP_COLORS[(i % (TAB_GROUP_COLORS.length - 1)) + 1]}
              className="size-full"
            />
          </div>
        );
      })}
    </div>
  );
}
