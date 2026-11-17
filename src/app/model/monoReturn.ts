/** The Mono a file was opened from, so closing that file returns to it. */
export type MonoReturn<View> = {
  monoId: string;
  sessionId: string;
  tabId: string;
  path: string;
  view: View;
};

/** The return a close fulfils: the file opened from a Mono, closed in the
 * tab it opened in, with no Mono already showing. */
export function monoReturnFor<View>(
  back: MonoReturn<View> | undefined,
  closed: { tabId: string; paths: readonly string[] },
  monoShowing: boolean,
): MonoReturn<View> | undefined {
  if (!back || monoShowing) return undefined;
  return back.tabId === closed.tabId && closed.paths.includes(back.path)
    ? back
    : undefined;
}
