import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useSyncExternalStore } from "react";
import {
  githubMonocodeStarStatus,
  starMonocodeOnGithub,
} from "../lib/githubTasks";
import { Loader, Star, X } from "./icons";

const MONOCODE_GITHUB_URL = "https://github.com/hardbeat920/monocode";
const DISMISSED_STORAGE_KEY = "monocode.githubStarPrompt.dismissed.v1";

type PromptSnapshot = "loading" | "visible" | "starring" | "hidden";

let promptSnapshot: PromptSnapshot = "loading";
let checkRequest: Promise<void> | null = null;
let starRequest: Promise<void> | null = null;
const promptListeners = new Set<() => void>();

function subscribePrompt(listener: () => void) {
  promptListeners.add(listener);
  return () => promptListeners.delete(listener);
}

function getPromptSnapshot() {
  return promptSnapshot;
}

function setPromptSnapshot(next: PromptSnapshot) {
  if (next === promptSnapshot) return;
  promptSnapshot = next;
  for (const listener of promptListeners) listener();
}

function isPromptDismissed() {
  try {
    return localStorage.getItem(DISMISSED_STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

function dismissPrompt() {
  try {
    localStorage.setItem(DISMISSED_STORAGE_KEY, "1");
  } catch {
    // Dismiss for this session even when storage is unavailable.
  }
  setPromptSnapshot("hidden");
}

function checkStarStatus(force = false): Promise<void> {
  if (isPromptDismissed()) {
    setPromptSnapshot("hidden");
    return Promise.resolve();
  }
  if (starRequest) return starRequest;
  if (checkRequest) return checkRequest;
  if (!force && promptSnapshot !== "loading") return Promise.resolve();

  const initial = promptSnapshot === "loading";
  checkRequest = githubMonocodeStarStatus()
    .then((status) => {
      if (status === "starred") setPromptSnapshot("hidden");
      else if (status === "notStarred") setPromptSnapshot("visible");
      else if (initial) setPromptSnapshot("hidden");
    })
    .catch(() => {
      if (initial) setPromptSnapshot("hidden");
    })
    .finally(() => {
      checkRequest = null;
    });
  return checkRequest;
}

function starFromPrompt(): Promise<void> {
  if (starRequest) return starRequest;
  setPromptSnapshot("starring");
  starRequest = starMonocodeOnGithub()
    .then(() => setPromptSnapshot("hidden"))
    .catch(async () => {
      setPromptSnapshot("visible");
      await openUrl(MONOCODE_GITHUB_URL).catch(() => undefined);
    })
    .finally(() => {
      starRequest = null;
    });
  return starRequest;
}

/** Clear module state between isolated component tests. */
export function resetGithubStarPromptCacheForTest() {
  promptSnapshot = "loading";
  checkRequest = null;
  starRequest = null;
  promptListeners.clear();
}

/** A quiet rail CTA that disappears once the active GitHub account has starred us. */
export function GithubStarPrompt() {
  const snapshot = useSyncExternalStore(
    subscribePrompt,
    getPromptSnapshot,
    getPromptSnapshot,
  );
  const show = snapshot === "visible" || snapshot === "starring";
  const busy = snapshot === "starring";

  useEffect(() => {
    void checkStarStatus();
    const refresh = () => void checkStarStatus(true);
    window.addEventListener("focus", refresh);
    return () => {
      window.removeEventListener("focus", refresh);
    };
  }, []);

  if (!show) return null;

  return (
    <div data-github-star-prompt className="relative mb-1 h-8 w-full">
      <button
        type="button"
        aria-label="Star MonoCode on GitHub"
        aria-busy={busy}
        disabled={busy}
        onClick={() => void starFromPrompt()}
        className="group flex h-full w-full items-center justify-center gap-2 rounded-md bg-orange-300/10 pl-2.5 pr-8 text-orange-400 transition-[background-color,transform] duration-150 ease-out hover:bg-orange-300/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-orange-400/60 active:scale-[0.98] disabled:cursor-default disabled:opacity-65"
      >
        {busy ? (
          <Loader aria-hidden className="size-3 shrink-0 animate-spin" />
        ) : (
          <Star
            className="size-3 shrink-0"
            fill="currentColor"
            strokeWidth={1.5}
          />
        )}
        <span className="min-w-0 truncate text-xs font-medium leading-tight">
          {busy ? "Starring…" : "Star on GitHub"}
        </span>
      </button>
      <button
        type="button"
        title="Don't show again"
        aria-label="Dismiss GitHub star prompt"
        onClick={dismissPrompt}
        className="absolute right-1 top-1/2 grid size-6 -translate-y-1/2 place-items-center rounded-md text-orange-400/60 transition-[color,background-color,transform] duration-150 ease-out hover:bg-orange-300/15 hover:text-orange-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange-400/60 active:scale-95"
      >
        <X className="size-3" strokeWidth={1.75} />
      </button>
    </div>
  );
}
