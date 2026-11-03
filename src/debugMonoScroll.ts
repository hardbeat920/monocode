// TEMPORARY: traces what scrolls the Mono chat. Remove once found.
const CHAT = "[data-mono-view] .agent-transcript";
const inChat = (el: unknown) => el instanceof Element && !!el.closest(CHAT);
const trace = (what: string, el?: Element) =>
  console.log(`[mono-scroll] ${what}`, el, new Error().stack);

const scrollTop = Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop")!;
Object.defineProperty(Element.prototype, "scrollTop", {
  ...scrollTop,
  set(this: Element, value: number) {
    if (inChat(this) && Math.abs(value - this.scrollTop) > 40)
      trace(`scrollTop ${Math.round(this.scrollTop)} -> ${Math.round(value)}`, this);
    scrollTop.set!.call(this, value);
  },
});
for (const name of ["scrollIntoView", "scrollTo", "scrollBy"] as const) {
  const original = Element.prototype[name] as (...args: unknown[]) => void;
  (Element.prototype as unknown as Record<string, unknown>)[name] = function (
    this: Element,
    ...args: unknown[]
  ) {
    if (inChat(this)) trace(name, this);
    return original.apply(this, args);
  };
}
const originalFocus = HTMLElement.prototype.focus;
HTMLElement.prototype.focus = function (this: HTMLElement, options?: FocusOptions) {
  if (inChat(this) && !options?.preventScroll) trace("focus", this);
  return originalFocus.call(this, options);
};
// Anything else (layout, browser focus) shows up as a jump with no trace above.
const last = new WeakMap<Element, number>();
document.addEventListener(
  "scroll",
  (event) => {
    const el = event.target;
    if (!(el instanceof Element) || !el.matches(CHAT)) return;
    const previous = last.get(el) ?? el.scrollTop;
    if (Math.abs(el.scrollTop - previous) > 40)
      console.log(`[mono-scroll] scrolled ${Math.round(previous)} -> ${Math.round(el.scrollTop)}`, document.activeElement);
    last.set(el, el.scrollTop);
  },
  true,
);
export {};
