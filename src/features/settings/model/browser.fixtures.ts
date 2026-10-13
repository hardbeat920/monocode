import {
  DEFAULT_BROWSER_SETTINGS,
  type BrowserState,
  type BrowserTab,
} from "./browser";

/** A `BrowserState` for tests: on, connected, nobody granted yet. */
export function browserState(patch: Partial<BrowserState> = {}): BrowserState {
  return {
    toggle_on: true,
    connection: {
      state: "connected",
      browser: {
        kind: "chrome",
        channel: "chrome",
        product: "Chrome/144.0.7559.60",
        major: 144,
        user_data_dir: "/home/me/.config/google-chrome",
        tabs_total: 0,
      },
      since_ms: 1_700_000_000_000,
    },
    sessions: [],
    tabs: [],
    settings: DEFAULT_BROWSER_SETTINGS,
    global_epoch: { run: 1, seq: 1 },
    ...patch,
  };
}

export function browserTab(patch: Partial<BrowserTab> = {}): BrowserTab {
  return {
    tab: "t_00000001",
    url: "https://example.com/path?q=1",
    title: "Example",
    state: "open",
    dialog_pending: false,
    busy: false,
    created_at_ms: 1_700_000_000_000,
    last_action_at_ms: 1_700_000_000_000,
    session_id: "s1",
    ...patch,
  };
}
