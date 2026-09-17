/** Probe the current environment for a usable WebGL2 context. */
export function supportsWebGL2(
  probe?: () => unknown,
): boolean {
  try {
    if (probe) return probe() != null;
    if (typeof document === "undefined") return false;
    const canvas = document.createElement("canvas");
    const gl = canvas.getContext("webgl2");
    // Lose the test context immediately; some drivers cap live contexts.
    const lose = (
      gl as unknown as { getExtension?: (name: string) => unknown }
    )?.getExtension?.("WEBGL_lose_context") as
      | { loseContext?: () => void }
      | undefined;
    lose?.loseContext?.();
    return gl != null;
  } catch {
    return false;
  }
}
