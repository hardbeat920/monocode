import type { ComponentPropsWithRef } from "react";

type Props = Omit<ComponentPropsWithRef<"button">, "className"> & {
  danger?: boolean;
};

export function SecondaryButton({
  danger = false,
  type = "button",
  children,
  ...props
}: Props) {
  return (
    <button
      {...props}
      type={type}
      className={`flex shrink-0 items-center gap-1.5 rounded-md border border-content/10 px-2.5 py-1 text-[12px] ${
        danger
          ? "text-red-400 hover:border-red-400/40 hover:bg-red-400/10"
          : "text-content/70 hover:bg-content/10 hover:text-content"
      } focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-default disabled:opacity-40 disabled:hover:bg-transparent`}
    >
      {children}
    </button>
  );
}
