import { useLayoutEffect, useRef, type ReactNode } from "react";
import { Pin } from "../../../shared/ui/icons";
import { savedPromptLabel } from "../model/savedPrompts";
import type { SavedPromptMenuState } from "./useSavedPromptMenu";

/**
 * The `!` saved-prompt list. `floating` draws its own card (above a
 * composer); without it the list sits inside the host's panel.
 */
export function SavedPromptMenu({
  state,
  floating = true,
}: {
  state: SavedPromptMenuState;
  floating?: boolean;
}): ReactNode {
  const activeRef = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => {
    activeRef.current?.scrollIntoView?.({ block: "nearest" });
  }, [state.active, state.query]);

  if (!state.open) return null;
  return (
    <div
      data-saved-prompt-menu
      className={
        floating
          ? "overflow-hidden rounded-lg border border-content/10 bg-content/5 backdrop-blur-xl"
          : "border-t border-stroke"
      }
    >
      {state.matches.length === 0 ? (
        <p className="px-3 py-2.5 text-[12px] text-content/50">
          No saved prompts yet. Add them in Settings → Prompts, or select text
          in a reply and choose Add to Prompts.
        </p>
      ) : (
        <div
          id="saved-prompt-menu"
          role="listbox"
          aria-label="Saved prompts"
          className="max-h-[min(240px,40vh)] overflow-y-auto overscroll-none px-1 py-1"
        >
          {state.matches.map((prompt, index) => {
            const highlighted = index === state.active;
            return (
              <button
                key={prompt.id}
                ref={highlighted ? activeRef : undefined}
                id={`saved-prompt-${prompt.id}`}
                type="button"
                role="option"
                aria-selected={highlighted}
                tabIndex={-1}
                onMouseDown={(event) => event.preventDefault()}
                onMouseEnter={() => state.setActive(index)}
                onClick={() => state.pick(prompt)}
                className={`flex w-full flex-col gap-0.5 rounded-md px-2 py-1.5 text-left text-content ${
                  highlighted ? "bg-content/10" : ""
                }`}
              >
                <span className="flex w-full min-w-0 items-center gap-1.5">
                  <span className="text-[13px] text-content/40">!</span>
                  <span className="truncate text-[13px]">
                    {savedPromptLabel(prompt)}
                  </span>
                  {prompt.pinned ? (
                    <Pin
                      aria-label="Pinned"
                      className="size-3 shrink-0 text-content/40"
                    />
                  ) : null}
                </span>
                <span className="line-clamp-2 whitespace-pre-line text-[11px] leading-4 text-content/50">
                  {prompt.body}
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
