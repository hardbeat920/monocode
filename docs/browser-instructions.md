# MonoCode browser tools: agent instructions

This file is the single source for what an agent reads about the browser
tools. The `monocode browser-mcp` server builds its `initialize` instructions
from the overview section and each tool description from the section with the
tool's name; the text injected into providers without MCP is the overview plus
the CLI section. Keep the sections short: models read every word on every
turn. Section names (`## name`) are looked up by code; do not rename them.

Provider support: Claude, Codex, Cursor and OpenCode 1 get the MCP server;
other providers get the CLI section. OpenCode 2 is not supported: it starts
MCP servers and runs shell commands from its shared background service,
which MonoCode starts without a session's environment, so neither the MCP
server nor the CLI receives the session's MONOCODE_BROWSER_TOKEN. Its
sessions get a note to tell the user instead of these instructions, and
MonoCode does not register the MCP server with it.

## overview

You can work in your own tabs in Chrome or Edge through MonoCode: either the MonoCode browser, which MonoCode starts with a profile of its own, or the user's own browser, as the user chose in MonoCode settings.

- The tabs are yours: MonoCode marks them with "◆ MonoCode" in the title. A tab the user opened in the MonoCode browser is free until a thread adopts it; when the user mentions a page they opened, list your tabs and adopt it. In the user's own browser they can also hand you one of their tabs, which then shows up in your list. In the user's own browser they open in the background and carry the user's sign-ins; you cannot see or touch the user's other tabs. Never ask the user to click or type in your tabs. When a site needs a sign-in you do not have, stop and tell the user; they can sign in once through Open browser in MonoCode's browser settings.
- Start by opening a tab; the result's data holds its id (`t_` + 8 hex). Every other action takes that `tab`. List your tabs when you lose track of them.
- Act on elements by ref (`e12`) from a snapshot. Opening a page and every action that can change it (navigate, click, type, press, select, hover, scroll, upload, dialog, wait, back) already return the page snapshot after it: the whole page after a navigation, otherwise only the lines that changed since your last snapshot (`+` added, `-` removed, `@@` names where), or that nothing changed. Refs you have not seen change keep working, so you rarely need a separate snapshot call. Pass `snapshot: false` when you do not need it. Refs from before a navigation fail with `ref_stale`.
- Every result has an outcome. `done`: the input reached the browser; check the effect in the snapshot that came with it. `blocked`: nothing was sent; change the plan and never work around it (for example with another URL). `failed`: fix the input (a fresh snapshot, another ref). `unknown`: the action may have happened; take a snapshot before retrying, and retry only with the same `request_id`.
- When the thread has no access yet, your first action asks the user in MonoCode and waits for the answer. On `not_granted` continue without the browser and do not retry in this turn. On `off` or a `browser_*` connection reason stop calling and tell the user what the message says.
- A JavaScript dialog blocks its tab until you answer it with the dialog action; MonoCode never answers one for you.
- Downloads, browser permission prompts and HTTP basic auth are not supported; tell the user when a task needs them. Only http(s) URLs and about:blank can be opened. There is no JavaScript or raw protocol access.
- Showing a tab to the user (focus) changes what they see; do it only when they ask.
- The user can point at an element in one of your tabs; their message then says "Element I picked in the browser" with the tab and a ref, or asks you to take a snapshot of that tab, where the element is marked `[picked by the user]`. That is the element they mean.
- When you test a web app, check the console and network actions: they show JavaScript errors and failed requests the page itself may hide. A result whose `observed.signals` counts new console errors or failed requests means the page broke since your last look; read them before you go on.

## browser_tabs

Manage your own browser tabs. action "list" returns the tabs this session owns, and in the MonoCode browser also `free` tabs the user opened; "adopt" takes one of those by its `free` id (`f_` + 8 hex), after which it is your tab; "open" opens a new background tab (optional http(s) `url`) and returns its `tab` id; "close" closes one of your tabs; "focus" brings one to the front of the user's browser and requires `user_visible: true` (only when the user asks to see it).

## browser_navigate

Load an http(s) URL (or about:blank) in one of your tabs and wait for `wait_until` (default load). A load that times out still returns done with `observed.load_state: "timeout"`; the snapshot with the result shows what loaded.

## browser_snapshot

Return the tab's accessibility tree as compact text with refs (`e12`) for the elements you can act on. Actions already return the snapshot after them, so call this when you need the whole page again or a part of it. `scope` limits it to the subtree of a ref; `mode: "full"` adds static text. An element the user pointed at in the browser is marked `[picked by the user]`, and the header names its ref.

## browser_click

Click an element by ref from the latest snapshot. Optional `button` (left, right, middle), `count` (1 or 2) and `modifiers`. done means the click was delivered, not that the page reacted; check `observed` and the snapshot that comes with the result.

## browser_hover

Move the mouse over an element by ref, for menus and tooltips that open on hover.

## browser_type

Focus an editable element by ref and insert `text`. `clear: true` replaces the current content first; `submit: true` presses Enter afterwards.

## browser_press

Press a key or combination such as `Enter`, `Escape`, `Tab`, `ArrowDown` or `Control+A` on the focused element, or on `ref` after focusing it.

## browser_select

Choose options in a native `<select>` element by value or visible label (`values`). For custom dropdowns, click them instead.

## browser_scroll

Scroll an element into view (`ref`), or scroll the page (or the element at `ref`) by `amount` pixels (default 600) in `direction`.

## browser_upload

Set files on a file input by ref. `paths` must be inside the session's working directory or an upload root the user allowed in MonoCode settings.

## browser_dialog

Answer the JavaScript dialog (alert, confirm, prompt, beforeunload) waiting on a tab: `accept` or `dismiss`, with `text` for a prompt. Only answer a dialog the task requires; when unsure, ask the user.

## browser_wait

Wait on a tab until a condition holds: `for` "time" (value in ms, at most 10000), "text" (value appears on the page), "ref" (element appears), "url" (prefix or glob) or "load". Prefer waiting for text or a ref over waiting for time.

## browser_back

Go back or forward in the tab's history (`direction`, default back) and wait for the page to load.

## browser_screenshot

Capture the tab (or the element at `ref`) as an image. Background tabs do not always render; when this fails with `screenshot_unavailable_background`, use a snapshot instead. Prefer snapshots for finding elements.

## browser_read

Return the readable text of the page or of the element at `ref`, as `text` or `markdown`. Long pages come in parts: pass `offset` to continue.

## browser_console

Read what the page in your tab logged: `console.*` messages, uncaught exceptions with their stack, and the browser's own errors and warnings (CSP, mixed content). Errors point to `url:line:column` in the page's scripts. `level` narrows it to `error` or `warning`; pass the `next_since` of your previous call as `since` to see only what is new. On a local server (localhost) everything is captured from the start; elsewhere capture starts with your first call, which replays the messages the browser kept and keeps capturing on that tab.

## browser_network

List requests of your tab that failed: HTTP 4xx and 5xx responses, network errors and CORS blocks, with method, URL, status and resource type. For a local server the start of the error response body is included. Addresses of other sites are shown without their query. Pass `next_since` from your previous call as `since` for only new failures.

## cli

Browser access is available in this thread. Use it through the MonoCode CLI, one action per call:

    {exe} browser tabs.open --url https://example.com
    {exe} browser snapshot --tab t_1a2b3c4d
    {exe} browser click --tab t_1a2b3c4d --ref e12

Run `{exe} browser --help` for every action and its fields. Each call prints one JSON result (`outcome`, `reason`, `message`, `retryable`, `observed`, `data`); a snapshot prints its text instead, and an action that changes the page prints the page snapshot (or its changes) as text after the JSON line (`--snapshot false` leaves it out). The exit code is 0 for done, 2 blocked, 3 failed, 4 unknown. After an `unknown` result, take a snapshot and retry only with `--request-id` set to the reported `request_id`. The CLI uses a session credential already in your environment; never print it.
