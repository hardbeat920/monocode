const PROJECT_PATHS_CHANGED = "monocode:project-paths-changed";

/** Rail projects, pins, groups, or their appearance changed in storage. */
export function notifyProjectPathsChanged(): void {
  if (typeof window !== "undefined") {
    window.dispatchEvent(new Event(PROJECT_PATHS_CHANGED));
  }
}

export function subscribeProjectPathsChanged(onChange: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(PROJECT_PATHS_CHANGED, onChange);
  return () => window.removeEventListener(PROJECT_PATHS_CHANGED, onChange);
}
