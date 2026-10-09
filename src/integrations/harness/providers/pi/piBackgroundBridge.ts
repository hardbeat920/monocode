// Loaded only into live Pi RPC sessions, never catalog/title probes.
// pi-subagents exposes this versioned registry contract for host keep-alive:
// src/integrations/pi-web-session-liveness.js in pi-subagents.
export const PI_BACKGROUND_STATUS_KEY = "monocode.pi-background.v1";
export const PI_BACKGROUND_BRIDGE = String.raw`
export default function (pi) {
  const key = Symbol.for("@agegr/pi-web/session-liveness/v1");
  const previous = globalThis[key];
  const entries = new Set();
  let ctx;
  let timer;
  let last;
  const publish = () => {
    if (!ctx) return;
    const sessionId = ctx.sessionManager.getSessionId();
    const tasks = [];
    for (const entry of entries) {
      if (entry.sessionId !== sessionId) continue;
      let active = true;
      // If an observer fails, retain the host rather than lose its results.
      try { active = entry.isActive(); } catch {}
      if (active) tasks.push(entry.name);
    }
    const text = JSON.stringify({ version: 1, tasks: [...new Set(tasks)].sort() });
    if (text !== last) {
      ctx.ui.setStatus("monocode.pi-background.v1", text);
      last = text;
    }
  };
  const registry = {
    version: 1,
    register(entry) {
      if (!entry || typeof entry.isActive !== "function" ||
          typeof entry.sessionId !== "string" || typeof entry.name !== "string") {
        throw new Error("Invalid background liveness registration");
      }
      const release = previous?.version === 1 ? previous.register(entry) : undefined;
      entries.add(entry);
      publish();
      return () => { entries.delete(entry); release?.(); publish(); };
    },
  };
  globalThis[key] = registry;
  pi.on("session_start", (_event, context) => {
    clearInterval(timer);
    ctx = context;
    last = undefined;
    publish();
    timer = setInterval(publish, 250);
    timer.unref?.();
  });
  // Publish synchronously before the foreground run can settle.
  pi.on("tool_result", publish);
  pi.on("agent_end", publish);
  pi.on("agent_settled", publish);
  pi.on("session_shutdown", () => {
    clearInterval(timer);
    ctx = undefined;
    entries.clear();
    if (globalThis[key] === registry) {
      if (previous === undefined) delete globalThis[key];
      else globalThis[key] = previous;
    }
  });
}
`;
