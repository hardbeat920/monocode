import { Fragment, type ReactNode } from "react";

/** Splits a message on `{name}` slots so each can be rendered as an element. */
export function withSlots(
  text: string,
  slots: Record<string, ReactNode>,
): ReactNode[] {
  return text.split(/(\{\w+\})/).map((part, index) => {
    const slot = /^\{(\w+)\}$/.exec(part)?.[1];
    return (
      <Fragment key={index}>
        {slot && slot in slots ? slots[slot] : part}
      </Fragment>
    );
  });
}
