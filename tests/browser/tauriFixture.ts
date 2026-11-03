import {
  invoke as nativeInvoke,
  type InvokeArgs,
  type InvokeOptions,
} from "../../node_modules/@tauri-apps/api/core";
export * from "../../node_modules/@tauri-apps/api/core";

/** Release-only fixture routing; the installed app never includes this module. */
export function invoke<T>(
  command: string,
  args?: InvokeArgs,
  options?: InvokeOptions,
): Promise<T> {
  const fixture = (
    window as unknown as { workspaceInvoke?: typeof nativeInvoke }
  ).workspaceInvoke;
  return (fixture ?? nativeInvoke)<T>(command, args, options);
}
