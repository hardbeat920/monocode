// Opt-in production measurements, shared by the real shortcut and tab renderer.
// No text, commands, paths, or session content is recorded.
const enabled =
  typeof localStorage !== "undefined" &&
  localStorage.getItem("monocode.measureNavigation") === "1";
let pending: { id: string; start: number } | undefined;
let frame = 0;

export function beginTabSwitch(id: string) {
  if (!enabled) return;
  pending = { id, start: performance.now() };
  cancelAnimationFrame(frame);
}

export function commitTabSwitch(id: string) {
  if (!enabled || pending?.id !== id) return;
  const sample = pending;
  const commit = performance.now();
  frame = requestAnimationFrame(() => {
    frame = requestAnimationFrame(() => {
      if (pending !== sample) return;
      pending = undefined;
      // The second frame is an upper bound on presentation, not a claim that
      // JavaScript can observe the compositor's exact presentation timestamp.
      if (performance.getEntriesByName("monocode:tab-switch").length >= 100)
        performance.clearMeasures("monocode:tab-switch");
      performance.measure("monocode:tab-switch", {
        start: sample.start,
        end: performance.now(),
        detail: { commitMs: commit - sample.start },
      });
    });
  });
}
