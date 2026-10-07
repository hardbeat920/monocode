import {
  browserHistory,
  closeBrowserView,
  evalInBrowser,
  navigateBrowser,
  screenshotBrowser,
} from "../../../platform/tauri/browser";
import { loadAgentBrowser } from "../../settings/model/displayPrefs";
import type { BrowserTabSource } from "../../workspace/model/layout";
import {
  closeBrowserTab,
  dockOfTab,
  findBrowserDock,
  getBrowserState,
  openBrowserTab,
  patchBrowserTab,
  selectBrowserTab,
  subscribeBrowser,
  touchAgentTab,
} from "./browserStore";
import {
  clickScript,
  consoleScript,
  readScript,
  typeScript,
  waitScript,
} from "./browserScripts";
import { browserUrlFromInput, isBrowsableUrl } from "./browserUrl";
import { whenNativeTabReady } from "./nativeTabs";

type Input = Record<string, unknown>;

const LOAD_TIMEOUT_MS = 15_000;
const EVAL_TIMEOUT_MS = 10_000;
const MAX_TIMEOUT_MS = 25_000;

function text(input: Input, key: string): string | undefined {
  const value = input[key];
  if (value === undefined || value === null) return undefined;
  if (typeof value !== "string") throw new Error(`${key} must be a string`);
  return value;
}

function required(input: Input, key: string): string {
  const value = text(input, key);
  if (!value) throw new Error(`${key} is required`);
  return value;
}

function bool(input: Input, key: string): boolean | undefined {
  const value = input[key];
  return typeof value === "boolean" ? value : undefined;
}

function clampNumber(
  input: Input,
  key: string,
  fallback: number,
  min: number,
  max: number,
): number {
  const value = input[key];
  if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value)));
}

function tabSource(id: string): BrowserTabSource | undefined {
  return dockOfTab(getBrowserState(), id)?.pane.files.find(
    (file) => file.id === id,
  )?.browser;
}

function summary(id: string) {
  const tab = tabSource(id);
  return { tabId: id, url: tab?.url, title: tab?.title || undefined };
}

function pageUrl(input: Input): string {
  const url = browserUrlFromInput(required(input, "url"));
  if (!isBrowsableUrl(url)) throw new Error("Only http(s) pages can be opened");
  return url;
}

/**
 * Resolve once a load that starts within `startWithinMs` finishes, or right
 * away when none starts (a hash change, a click that did not navigate).
 */
function waitForLoad(
  id: string,
  { startWithinMs = 1_500, timeoutMs = LOAD_TIMEOUT_MS } = {},
): Promise<{ timedOut?: true }> {
  return new Promise((resolve) => {
    let started = !!tabSource(id)?.loading;
    let unsubscribe = () => {};
    const finish = (result: { timedOut?: true }) => {
      unsubscribe();
      clearTimeout(startTimer);
      clearTimeout(timer);
      resolve(result);
    };
    const check = () => {
      const tab = tabSource(id);
      if (!tab) return finish({});
      if (tab.loading) started = true;
      else if (started) finish({});
    };
    const startTimer = setTimeout(() => {
      if (!started) finish({});
    }, startWithinMs);
    const timer = setTimeout(() => finish({ timedOut: true }), timeoutMs);
    unsubscribe = subscribeBrowser(check);
    check();
  });
}

/** Run browser tools for one session against its own tabs only. */
export function createBrowserAgent(sessionId: string) {
  const dock = () => findBrowserDock(getBrowserState().docks, sessionId);

  /** The tab a call acts on, made live (resumed if suspended). */
  const useTab = async (input: Input): Promise<string> => {
    const requested = text(input, "tabId");
    const current = dock();
    const id = requested ?? current?.pane.activeFileId;
    if (!id || !current?.pane.files.some((file) => file.id === id)) {
      throw new Error(
        requested
          ? `No tab ${requested} in this session. Call browser_tabs to list them.`
          : "This session has no browser tab yet. Use browser_open first.",
      );
    }
    touchAgentTab(id);
    await whenNativeTabReady(id, 8_000);
    return id;
  };

  const run = async (id: string, script: string, timeoutMs = EVAL_TIMEOUT_MS) =>
    evalInBrowser(id, script, timeoutMs);

  /** Click and type may navigate away before their result comes back. */
  const runAction = async (id: string, script: string) => {
    let result: unknown;
    try {
      result = await run(id, script);
    } catch (error) {
      if (!String(error).includes("navigated away")) throw error;
      result = { navigated: true };
    }
    const load = await waitForLoad(id);
    return { ...(result as object), page: summary(id), ...load };
  };

  const actions: Record<string, (input: Input) => Promise<unknown>> = {
    async browser_tabs() {
      const current = dock();
      return {
        panelOpen: !!current?.open,
        tabs: (current?.pane.files ?? []).map((file) => ({
          tabId: file.id,
          url: file.browser?.url,
          title: file.browser?.title || undefined,
          active: file.id === current?.pane.activeFileId,
          loading: !!file.browser?.loading,
        })),
      };
    },

    async browser_open(input) {
      const url = pageUrl(input);
      const id = openBrowserTab(url, { sessionId, background: true });
      if (!id) throw new Error("This session cannot open a browser tab");
      touchAgentTab(id);
      patchBrowserTab(id, { loading: true });
      await whenNativeTabReady(id, 8_000);
      const load = await waitForLoad(id);
      return { ...summary(id), ...load };
    },

    async browser_navigate(input) {
      const id = await useTab(input);
      const url = pageUrl(input);
      patchBrowserTab(id, { url, loading: true, error: undefined });
      await navigateBrowser(id, url).catch((error: unknown) => {
        patchBrowserTab(id, { loading: false });
        throw error;
      });
      return { ...summary(id), ...(await waitForLoad(id)) };
    },

    async browser_history(input) {
      const action = required(input, "action");
      if (action !== "back" && action !== "forward" && action !== "reload") {
        throw new Error("action must be back, forward, or reload");
      }
      const id = await useTab(input);
      await browserHistory(id, action);
      return { ...summary(id), ...(await waitForLoad(id)) };
    },

    async browser_select(input) {
      const id = await useTab({ tabId: required(input, "tabId") });
      selectBrowserTab(id);
      return summary(id);
    },

    async browser_close(input) {
      const id = required(input, "tabId");
      if (!dock()?.pane.files.some((file) => file.id === id)) {
        throw new Error(`No tab ${id} in this session`);
      }
      closeBrowserTab(id);
      await closeBrowserView(id).catch(() => undefined);
      return { closed: id };
    },

    async browser_read(input) {
      const format = text(input, "format");
      if (format && format !== "text" && format !== "html") {
        throw new Error("format must be text or html");
      }
      const id = await useTab(input);
      return run(
        id,
        readScript({
          format: format as "text" | "html" | undefined,
          selector: text(input, "selector"),
          maxChars: clampNumber(input, "maxChars", 20_000, 100, 200_000),
        }),
      );
    },

    async browser_eval(input) {
      const script = required(input, "script");
      const id = await useTab(input);
      const timeoutMs = clampNumber(
        input,
        "timeoutMs",
        EVAL_TIMEOUT_MS,
        100,
        MAX_TIMEOUT_MS,
      );
      return { result: await run(id, script, timeoutMs) };
    },

    async browser_console(input) {
      const id = await useTab(input);
      return run(
        id,
        consoleScript({
          limit: clampNumber(input, "limit", 100, 1, 500),
          clear: bool(input, "clear"),
        }),
      );
    },

    async browser_click(input) {
      const selector = text(input, "selector");
      const label = text(input, "text");
      if (!selector && !label) throw new Error("Pass a selector or text");
      const id = await useTab(input);
      return runAction(id, clickScript({ selector, text: label }));
    },

    async browser_type(input) {
      const id = await useTab(input);
      return runAction(
        id,
        typeScript({
          selector: required(input, "selector"),
          text: typeof input.text === "string" ? input.text : "",
          clear: bool(input, "clear"),
          submit: bool(input, "submit"),
        }),
      );
    },

    async browser_wait(input) {
      const selector = text(input, "selector");
      const label = text(input, "text");
      if (!selector && !label) throw new Error("Pass a selector or text");
      const timeoutMs = clampNumber(input, "timeoutMs", 10_000, 100, 24_000);
      const id = await useTab(input);
      return run(
        id,
        waitScript({ selector, text: label, timeoutMs }),
        timeoutMs + 1_000,
      );
    },

    async browser_screenshot(input) {
      const id = await useTab(input);
      const image = await screenshotBrowser(id);
      const page = summary(id);
      return {
        image,
        mimeType: "image/png",
        text: `${page.title ?? ""} ${page.url ?? ""}`.trim(),
      };
    },
  };

  return async (action: string, input: Input): Promise<unknown> => {
    if (!loadAgentBrowser()) {
      throw new Error(
        "Browser access for agents is turned off in MonoCode settings.",
      );
    }
    const handler = actions[action];
    if (!handler) throw new Error(`Unknown browser tool: ${action}`);
    return handler(input ?? {});
  };
}

/** Handle one `browser` namespace control request. */
export function handleBrowserRequest(
  sessionId: string,
  action: string,
  input: Input,
): Promise<unknown> {
  return createBrowserAgent(sessionId)(action, input);
}
