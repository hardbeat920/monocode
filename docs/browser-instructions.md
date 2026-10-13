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

You can work in your own tabs in the user's Chrome or Edge through MonoCode.

- The tabs are yours: you open them in the background of the user's browser, MonoCode marks them with "◆ MonoCode" in the title, and they carry the user's sign-ins. You cannot see or touch the user's other tabs. Never ask the user to click or type in your tabs.
- Start by opening a tab; the result's data holds its id (`t_` + 8 hex). Every other action takes that `tab`. List your tabs when you lose track of them.
- Act on elements by ref: take a snapshot, then use refs such as `e12` from it. After any action that changes the page, take a new snapshot before using refs again; old refs fail with `ref_stale`.
- Every result has an outcome. `done`: the input reached the browser; check the effect with a snapshot. `blocked`: nothing was sent; change the plan and never work around it (for example with another URL). `failed`: fix the input (a fresh snapshot, another ref). `unknown`: the action may have happened; take a snapshot before retrying, and retry only with the same `request_id`.
- Without browser access (`not_granted`, `off`, `browser_*` connection reasons) stop calling and tell the user what the message says; they enable it with /browser in this thread or in MonoCode settings.
- A JavaScript dialog blocks its tab until you answer it with the dialog action; MonoCode never answers one for you.
- Downloads, browser permission prompts and HTTP basic auth are not supported; tell the user when a task needs them. Only http(s) URLs and about:blank can be opened. There is no JavaScript or raw protocol access.
- Showing a tab to the user (focus) changes what they see; do it only when they ask.

## browser_tabs

Manage your own browser tabs. action "list" returns the tabs this session owns; "open" opens a new background tab (optional http(s) `url`) and returns its `tab` id; "close" closes one of your tabs; "focus" brings one to the front of the user's browser and requires `user_visible: true` (only when the user asks to see it).

## browser_navigate

Load an http(s) URL (or about:blank) in one of your tabs and wait for `wait_until` (default load). A load that times out still returns done with `observed.load_state: "timeout"`; take a snapshot to see what loaded.

## browser_snapshot

Return the tab's accessibility tree as compact text with refs (`e12`) for the elements you can act on. Call it before acting and after every action that changes the page. `scope` limits it to the subtree of a ref; `mode: "full"` adds static text.

## browser_click

Click an element by ref from the latest snapshot. Optional `button` (left, right, middle), `count` (1 or 2) and `modifiers`. done means the click was delivered, not that the page reacted; check `observed` and take a snapshot.

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

Wait on a tab until a condition holds: `for` "time" (value in ms, at most 10000), "text" (value appears on the page), "ref" (element appears), "url" (prefix or glob) or "load". Prefer a snapshot over waiting for time.

## browser_back

Go back or forward in the tab's history (`direction`, default back) and wait for the page to load.

## browser_screenshot

Capture the tab (or the element at `ref`) as an image. Background tabs do not always render; when this fails with `screenshot_unavailable_background`, use a snapshot instead. Prefer snapshots for finding elements.

## browser_read

Return the readable text of the page or of the element at `ref`, as `text` or `markdown`. Long pages come in parts: pass `offset` to continue.

## cli

Browser access is enabled in this thread. Use it through the MonoCode CLI, one action per call:

    {exe} browser tabs.open --url https://example.com
    {exe} browser snapshot --tab t_1a2b3c4d
    {exe} browser click --tab t_1a2b3c4d --ref e12

Run `{exe} browser --help` for every action and its fields. Each call prints one JSON result (`outcome`, `reason`, `message`, `retryable`, `observed`, `data`); a snapshot prints its text instead. The exit code is 0 for done, 2 blocked, 3 failed, 4 unknown. After an `unknown` result, take a snapshot and retry only with `--request-id` set to the reported `request_id`. The CLI uses a session credential already in your environment; never print it.
