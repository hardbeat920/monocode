import type { CSSProperties } from "react";
import { PROJECT_MASCOTS } from "../../projects/model/projectMascots";
import { PixelMascot } from "../../projects/ui/PixelMascot";
import { TAB_GROUP_COLORS } from "../../workspace/model/tabGroups";
import { Popover } from "../../../shared/ui/Popover";
type Look = { mascot: string; color: string };

type Props = {
  /** The rail's add-a-Mono button, retained as the outside-click anchor. */
  anchor: HTMLElement | null;
  /** The mascot and color the user's first Mono will have. */
  look: Look;
  /** Creates the first Mono and opens it. */
  onCreate: () => void;
  /** Passes on the intro for good; the rail still offers a new Mono. */
  onLater: () => void;
};

/** Gap between the mascots lining up at the back. */
const CROWD_STEP = 36;

/** The rest of the mascots, each in its own color, to line up behind the mono. */
export function monoIntroCrowd(look: Look) {
  const colors = TAB_GROUP_COLORS.slice(1).filter(
    (color) => color !== look.color,
  );
  return PROJECT_MASCOTS.map((mascot) => mascot.name)
    .filter((name) => name !== look.mascot)
    .map((name, i) => ({ name, color: colors[i % colors.length] }));
}

/**
 * Meets a user who has no Mono yet, centered in the window, inviting them to
 * make their first. It shows once ever and stays until they choose: clicking
 * away or Escape does not put it away.
 */
export function MonoIntroPopover({ anchor, look, onCreate, onLater }: Props) {
  const crowd = monoIntroCrowd(look);
  const middle = (crowd.length - 1) / 2;

  return (
    <Popover
      anchor={anchor}
      centered
      rounded="rounded-md"
      width={360}
      role="dialog"
      aria-label="Meet Monos"
    >
      <div
        aria-hidden
        data-mono-intro-stage
        className="mono-intro relative h-52 overflow-hidden"
      >
        {crowd.map((mascot, i) => {
          const offset = (i - middle) * CROWD_STEP;
          // Fill in from the outside, alternating sides.
          const order =
            Math.min(i, crowd.length - 1 - i) * 2 + (i < middle ? 0 : 1);
          return (
            <div
              key={mascot.name}
              className="mono-intro-march absolute top-6 left-1/2 opacity-60"
              style={
                {
                  marginLeft: offset - 12,
                  "--from": `${offset < 0 ? -240 : 240}px`,
                  "--delay": `${order * 90}ms`,
                } as CSSProperties
              }
            >
              <div className="mono-intro-hop">
                <PixelMascot
                  name={mascot.name}
                  color={mascot.color}
                  className="size-6"
                />
              </div>
            </div>
          );
        })}

        <div className="absolute bottom-6 left-1/2 h-4 w-36 -translate-x-1/2 bg-[radial-gradient(closest-side,color-mix(in_srgb,var(--color-content)_12%,transparent),transparent)]" />

        {/* The user's mono drops into the spotlight once the rest are in. */}
        <div className="mono-intro-drop absolute bottom-8 left-1/2 -ml-12">
          <PixelMascot
            name={look.mascot}
            color={look.color}
            className="size-24"
          />
        </div>
      </div>

      <div className="px-5 pb-5 text-center">
        <h3 className="flex items-center justify-center gap-2 text-xl font-semibold text-content">
          Meet Monos
          <span className="rounded-full bg-content/8 px-1.5 py-px text-[10px] font-medium text-content/55">
            Experimental
          </span>
        </h3>
        <p className="mx-auto mt-1.5 text-[13px] leading-relaxed text-content/55">
          Agents of your own that live on the rail and work across your
          projects. They remember what matters and pick up habits they run on
          their own.
        </p>
        <div className="mt-6 flex flex-col gap-1">
          <button
            type="button"
            autoFocus
            onClick={onCreate}
            className="w-full rounded-lg bg-content py-2 text-[13px] font-medium text-background-base hover:bg-content/85 active:scale-[0.98]"
          >
            Create your mono
          </button>
          <button
            type="button"
            onClick={onLater}
            className="w-full rounded-lg py-2 text-[13px] text-content/60 hover:bg-content/8 hover:text-content"
          >
            Not now
          </button>
        </div>
      </div>
    </Popover>
  );
}
