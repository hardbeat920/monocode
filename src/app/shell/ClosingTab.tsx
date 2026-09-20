import {
  useLayoutEffect,
  useRef,
  type CSSProperties,
  type ReactNode,
} from "react";
import { tabCloseDuration } from "../../shared/lib/motion";

type Props = {
  phase: "opening" | "closing";
  width?: number;
  onFinish: () => void;
  children: ReactNode;
};

type TabStyle = CSSProperties & { "--tab-slot-width"?: string };

export function TabWidthMotion({
  phase,
  width = 0,
  onFinish,
  children,
}: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const onFinishRef = useRef(onFinish);
  onFinishRef.current = onFinish;

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      onFinishRef.current();
      return;
    }

    const duration = tabCloseDuration();
    let innerFrame = 0;
    let timer = 0;
    const outerFrame = requestAnimationFrame(() => {
      innerFrame = requestAnimationFrame(() => {
        el.toggleAttribute("data-collapsed", phase === "closing");
        timer = window.setTimeout(() => onFinishRef.current(), duration);
      });
    });
    return () => {
      cancelAnimationFrame(outerFrame);
      cancelAnimationFrame(innerFrame);
      window.clearTimeout(timer);
    };
  }, [phase]);

  const style: TabStyle | undefined =
    phase === "closing"
      ? { "--tab-slot-width": width > 1 ? `${width}px` : "14rem" }
      : undefined;

  return (
    <div
      ref={ref}
      aria-hidden={phase === "closing" || undefined}
      inert={phase === "closing" || undefined}
      data-closing-tab={phase === "closing" || undefined}
      data-opening-tab={phase === "opening" || undefined}
      data-collapsed={phase === "opening" || undefined}
      className={`tab-${phase} relative flex h-full min-w-0 self-stretch`}
      style={style}
    >
      {children}
    </div>
  );
}
