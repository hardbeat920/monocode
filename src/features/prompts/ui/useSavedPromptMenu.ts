import {
  useCallback,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import {
  insertSavedPrompt,
  markSavedPromptUsed,
  promptTokenAt,
  rankSavedPrompts,
  type PromptToken,
  type SavedPrompt,
} from "../model/savedPrompts";
import { useSavedPrompts } from "./useSavedPrompts";

export type SavedPromptMenuState = {
  open: boolean;
  query: string;
  matches: SavedPrompt[];
  /** True when the user has no saved prompts yet (shows a hint). */
  empty: boolean;
  active: number;
  setActive: (index: number) => void;
  pick: (prompt: SavedPrompt) => void;
};

/**
 * The `!` saved-prompt picker for any textarea composer. The host calls
 * `sync` after input, clicks and caret moves, routes keys through
 * `onKeyDown` first, and renders `<SavedPromptMenu state={menu} />`.
 * `apply` writes the new text and caret into the host's own state.
 */
export function useSavedPromptMenu({
  apply,
}: {
  apply: (text: string, cursor: number) => void;
}): SavedPromptMenuState & {
  sync: (field: HTMLTextAreaElement) => void;
  onKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => boolean;
  close: () => void;
} {
  // Loaded on the first `!`, so composers that never use it never query.
  const [wanted, setWanted] = useState(false);
  const { prompts } = useSavedPrompts(wanted);
  const [token, setToken] = useState<PromptToken | null>(null);
  const tokenRef = useRef<PromptToken | null>(null);
  const [active, setActive] = useState(0);
  const field = useRef<HTMLTextAreaElement | null>(null);
  // Escape hides the menu for this `!` until a new one is typed.
  const dismissedAt = useRef<number | null>(null);

  const matches = useMemo(
    () => (token && prompts ? rankSavedPrompts(prompts, token.query) : []),
    [prompts, token],
  );
  const empty = prompts !== null && prompts.length === 0;
  // An empty `!` shows the hint; a query that matches nothing closes the
  // menu, so `!important` or `!=` read as normal text.
  const open =
    token !== null &&
    prompts !== null &&
    (matches.length > 0 || (empty && token.query === ""));

  const sync = useCallback((element: HTMLTextAreaElement) => {
    field.current = element;
    const next =
      element.selectionStart === element.selectionEnd
        ? promptTokenAt(element.value, element.selectionStart ?? 0)
        : null;
    if (next?.start !== dismissedAt.current) dismissedAt.current = null;
    const shown = next && dismissedAt.current === null ? next : null;
    if (shown) setWanted(true);
    const current = tokenRef.current;
    if (current?.query !== shown?.query || current?.start !== shown?.start)
      setActive(0);
    tokenRef.current = shown;
    setToken(shown);
  }, []);

  const close = useCallback(() => {
    tokenRef.current = null;
    setToken(null);
  }, []);

  const pick = useCallback(
    (prompt: SavedPrompt) => {
      const element = field.current;
      if (!element || !token) return;
      const result = insertSavedPrompt(element.value, token, prompt.body);
      tokenRef.current = null;
      setToken(null);
      apply(result.text, result.cursor);
      markSavedPromptUsed(prompt.id);
    },
    [apply, token],
  );

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (!open || event.nativeEvent.isComposing) return false;
    const count = matches.length;
    if ((event.key === "ArrowDown" || event.key === "ArrowUp") && count) {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActive((index) => (index + step + count) % count);
      return true;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismissedAt.current = token?.start ?? null;
      tokenRef.current = null;
      setToken(null);
      return true;
    }
    if (
      (event.key === "Enter" || event.key === "Tab") &&
      !event.shiftKey &&
      !event.altKey &&
      !event.ctrlKey &&
      !event.metaKey &&
      count
    ) {
      event.preventDefault();
      pick(matches[Math.min(active, count - 1)]!);
      return true;
    }
    return false;
  };

  return {
    open,
    query: token?.query ?? "",
    matches,
    empty,
    active,
    setActive,
    pick,
    sync,
    onKeyDown,
    close,
  };
}
