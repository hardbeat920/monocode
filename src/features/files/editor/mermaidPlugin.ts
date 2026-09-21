import type { DiagramPlugin, MermaidConfig, MermaidInstance } from "@streamdown/mermaid";

/**
 * `@streamdown/mermaid` is a thin wrapper over a static `import "mermaid"`,
 * which drags mermaid's ~730 KB into the boot chunk for a fence type most
 * sessions never contain. This keeps the same `DiagramPlugin` shape but pulls
 * the engine in on first render — `render` is already async, and the caller
 * already shows a placeholder while it resolves.
 */
const BASE: MermaidConfig = {
  startOnLoad: false,
  theme: "default",
  securityLevel: "strict",
  fontFamily: "monospace",
  suppressErrorRendering: true,
};

export function createLazyMermaidPlugin(
  options: { config?: MermaidConfig } = {},
): DiagramPlugin {
  let config: MermaidConfig = { ...BASE, ...options.config };
  let initialized = false;
  let engine: Promise<typeof import("mermaid").default> | null = null;

  const load = () => (engine ??= import("mermaid").then((mod) => mod.default));

  const instance: MermaidInstance = {
    initialize(next: MermaidConfig) {
      config = { ...BASE, ...options.config, ...next };
      // The real plugin initializes eagerly; deferring it to `render` keeps
      // mermaid off the boot path without changing what gets applied.
      initialized = false;
    },
    async render(id: string, source: string) {
      const mermaid = await load();
      if (!initialized) {
        mermaid.initialize(config);
        initialized = true;
      }
      return mermaid.render(id, source);
    },
  };

  return {
    name: "mermaid",
    type: "diagram",
    language: "mermaid",
    getMermaid(next?: MermaidConfig) {
      if (next) instance.initialize(next);
      return instance;
    },
  };
}
