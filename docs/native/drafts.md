# Drafts

An agent can build a live page, look at it out of sight, and then publish it
for you to use. The page is a card in the thread at the point where the agent
published it: a WebKit view hosted in MonoCode's own window (macOS only), clipped to the
transcript as it scrolls. It is not a side panel and not a separate window.
Native visuals (`visual_show`) are unchanged; see [session visuals](visuals.md).

## Tools

`visual_docs` with `{"topic": "draft"}` returns the full guide; `kit` returns
the optional component kit and `app` returns MonoCode's real design language
(theme-derived ink alphas, radii, type and sizes) so a mock of MonoCode looks
like MonoCode.

The kit has an optional controls part: `mc.state` (a store mirrored to
`--s-*` CSS variables and `data-s-*` attributes) and `mc.controls` (sliders,
segmented controls, selects, steppers, swatches, text, toggles and buttons in a
rail, bar or dock outside the content). A page that uses them reports the
values as `state` in every snapshot. `examples/controls-kit-demo.html` shows
the three layouts and `examples/controls-kit-all.html` every control type;
`scripts/verify-native-draft.py` drives both.

`draft_preview` takes `html` or a localhost `url` plus a short `title` to start
a draft, or `draft` to continue one, with an optional `action`:

| Action | Effect |
| --- | --- |
| `{kind:"snapshot"}` (default) | Look again, optionally at a new `width`, `height` or `appearance` |
| `{kind:"click", reference}` | Click a control by its ref; a ref lasts as long as its element is on the page |
| `{kind:"type", text, reference?, replace}` | Type into the field with that ref, or the focused one |
| `{kind:"resume"}` | Take control back after the person had it |
| `{kind:"close"}` | Close the draft |

Every call returns a screenshot, the page text, controls with refs, console
output and diagnostics. The screenshots go to the agent only: the thread shows
the card, not a picture per call. The page is saved as it changes, so a card
comes back live after the app was restarted. A card names a draft once; a second row for the same draft
is not drawn.

## Feedback loop

The agent can look, fix and look again on its own, and the person can answer
from the page itself:

- **Comment** on the card turns on comment mode. Clicking the page pins a
  numbered note to the element under the click; **Send N notes** hands them
  over. Notes carry the element's tag and label and their place on the page,
  and the pins show in the agent's screenshots.
- `draft_wait {draft, seconds?}` pauses the agent (up to 30 seconds per call)
  until the person sends notes or presses **Hand back**. The card shows
  "Waiting for you". It returns the usual screenshot and numbers with
  `feedback`, `userActions` (clicks they made by hand) and `event`
  (`feedback`, `handed back` or `timeout`).
- If the agent was not waiting, **Send** delivers the notes as a message in the
  conversation, queued to steer a running turn where the provider can take it.
- Every result also lists `issues` the page reveals: low contrast, small tap
  targets, controls without a name, images without alt text, clipped text.
- The draft tool calls themselves are hidden from the thread; the card is the
  display.

## Behavior

- A new draft runs out of sight (the bridge picks its id) until the agent
  shows it: `draft_preview` with `publish: true`, or `draft_wait`. The bridge
  then puts its card into the running turn once that call's page has answered, so a failed call adds no card, and a failed first
  call closes the new draft and deletes its file. Later versions of a shown
  draft appear in its card as they come. A draft the agent never shows
  stays out of the thread; its card stays held in the bridge, so a later turn
  can still publish it. Each agent request, `draft_wait` included, holds a lease on the page, so it opens
  and runs whether or not the card is on screen: the view is parked clear and
  out of the pointer's way in the window. Without a lease an unseen page
  pauses.
- The card is as wide as the column (up to the page's width, scaled down when
  the page is wider) and as tall as the page content unless the agent sets a
  height. Up to three drafts stay live per conversation; the oldest is retired
  and its card turns into a "closed" note with **Open again**.
- The app saves every draft as it changes (page, title, size, appearance and
  `mc.state`) in `threads/<session>/drafts/<id>.json` beside the
  conversation's other files. After a restart a card whose page is not open
  opens it again from there the first time the card is drawn or the agent
  sends to it, under the same id. A retired draft comes back the same way. Closing a draft deletes its file;
  deleting the conversation deletes the folder.
- Agent clicks and typing are sent as script events. They never take focus from
  the user's composer. When the person clicks or types in the page they have
  control: snapshots still work, actions are refused until `resume`. The
  page's own scripts can post the same reports, so a report only counts within
  two seconds of a real click or key press in the view: a page cannot take
  control, pin notes or message the agent for the person.
- The page pings the app (a main-frame-only `mcWake` message) whenever it
  reports something, and the app reads it then, so takeover, notes and Send
  land at once. A one-second tick backs the ping up.
- HTML runs in a sandboxed frame with a content security policy: inline scripts
  and styles, data images, and a CDN allowlist for scripts, styles and fonts;
  no network from the page. Markup may be 512 KiB, plus embedded images.
  A `url` must be on localhost, so the card doubles as a dev-server browser.
- Pages receive the app's theme through CSS variables, derived from the real
  theme at the time of the call, and follow `appearance:"light"|"dark"`.
- The native view is placed from the card's bounds on every paint, clipped to
  the thread's visible area, and hidden when its row is not painted. A popover
  over it swaps in a still.

## On the phone

A draft runs inside the thread on a paired phone too, at its place in the
conversation, with no screen of its own. Each one is a card: its title, the
page at the phone's width, and the desktop's controls (comment, close, send
notes, hand back). On iOS the card is a native view the transcript owns
(`TranscriptDraft.swift`), because rows are recycled and a page has to outlive
them; on Android it is a React view (`DraftCard.tsx`). Both are driven by
`DraftController.ts`, and the rules for what crosses between the screens are in
`sync.ts`.

- **Reading never opens.** A card reads with `peek`, so scrolling past a closed
  draft leaves it closed and retires nothing on the desktop. A paused card offers
  "Paused · tap to run"; only that press opens the page. A draft the person
  closed says so in one line, with **Open again**.
- **On screen means running.** A card off screen stops following the record and
  pauses its page. iOS keeps at most three pages per thread and gives up the one
  used longest ago; the card asks the desktop for it when it comes back.
- **Height.** The page reports its height (`h`) and the card follows it, up to
  12,000 points. The row is measured again alone, so nothing else in the thread
  is laid out for it. Until the page speaks, the card uses the height the
  desktop measured at phone width.
- **Touch.** A vertical drag scrolls the thread; a sideways drag on the page
  belongs to the page. What the page reports counts only within two seconds of a
  real touch, and a touch that became a scroll does not count: the input it
  reported is dropped, so scrolling past a card does not take control of it.
  While a finger is on a page, the screen's own pans sit out the touch (iOS 26's
  swipe back from anywhere). The edge swipe stays.
- **Touch events.** WebKit on iOS sends touches only where the outer document
  listens, and the sandboxed page's listeners do not count, so the phone bridge
  adds passive ones. Without them a tap arrives as a bare click, with no
  `pointerdown`, and `mc.send` refuses it.
- **State.** The desktop page's first report starts the record. If that page has
  not run, the person's first change on the phone starts it instead.
- **Dev servers** show pictures from `draft.frame`, once a second, with no page.
- **Full view.** iOS moves the same page into a full screen view and back, so
  nothing the page keeps to itself is lost. Android has no full view.
- **Waiting.** When the agent waits on a draft that is off screen, a pill above
  the composer goes to it, and the sessions list says so. The catalog carries
  `waitingDraft` for this. A card less than 120 points past the edge still
  counts as on screen, because its row is kept that far out. The wait holds a
  lease, so the person can answer here while the desktop shows no card.
- **Mono chats.** A Mono's chat is not among the sessions, so `draft.*` calls
  name it by Mono and the desktop finds the drafts in its conversation. Notes
  sent there go to that chat.

A phone from before this reads the same rows as the plain line "Pricing · Open on
desktop to view"; the card rides on that notice as `draft`.

To check on a real phone paired to a desktop built from this source:

- Scroll past a closed draft and confirm it stays closed on the desktop.
- Move a control on the phone and see it move on the desktop, then the reverse.
- Press a button in the page that calls `mc.send`, and see the turn start.
- Leave a note on the phone, press **Send**, and see it reach the agent.
- With the agent in `draft_wait`, scroll away and follow the pill back.
- With the desktop on another conversation, answer a `draft_wait` on the phone:
  the agent gets the answer, not an error.
- Drag a slider in a Mono's chat: the chat must not swipe back.
- On Android, repeat these with a card in a long thread.

## Verifying

```sh
cd crates/monocode-gpui && cargo build --profile native-dev
python3 scripts/verify-native-draft.py
```

The script opens a self-test window with the draft inside a thread, drives `docs/native/examples/draft-spike.html`
through snapshot, click, type, takeover, resume, resize and close, and
checks what the agent would receive. For ad hoc pages set
`MONOCODE_DRAFT_SELFTEST=page.html` (debug builds, add
`MONOCODE_DRAFT_SELFTEST_THREAD=1` to draw it in a thread); `MONOCODE_DRAFT_ACTIONS`
takes a JSON array of further `draft_preview` inputs, and it writes
`page.html.N.png` and `.json` for each step.
