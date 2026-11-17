# Drafts

For a plain chart, stat row or gauge, the native visual_show is simpler. Reach for a draft when the idea needs freer layout,
type or motion, or when you want to check it in a screenshot.

A draft is a live page you build for the conversation. You build and check it
out of sight; once you publish it, it appears as a card at that point in the
thread, between your messages, and the user can click around in it right there. Use it to design UI, mock a
screen, try an interaction, build a small tool or explainer with full HTML,
CSS and JavaScript, or check a running dev server. There is no side panel.

## The loop

1. draft_preview {html, title} (or {url}) takes the page's HTML inline: do not
   write it to a file first. It opens the page out of sight and
   returns a screenshot, the page text, controls with refs, console output and
   diagnostics.
2. Look at the screenshot. Fix console errors, clipping, overflow and anything
   that reads badly. draft_preview {draft, html} replaces the page.
3. Test it like a person would: {draft, action:{kind:"click", reference:"REF"}},
   then {draft, action:{kind:"type", reference:"REF", text:"hello",
   replace:true}} for a field (without reference it types into the focused
   one). A ref stays valid for as long as its element is on the page, so you
   can act on several controls from one look; a new version of the page
   gives new refs.
4. {draft, width:375, appearance:"dark"} checks another viewport and theme.
   The appearance holds for that one picture only: the card the person sees
   always follows the app's theme, because the page is clear over the thread.
   Color everything that sits on the thread from the theme variables
   (--foreground, --background, --muted-foreground and the rest) so it reads
   on either; a page's own prefers-color-scheme rules follow the same
   appearance. A mock keeps its own look with data-theme (see "Make the page
   look like part of the conversation").
5. When you have looked at it, tested it and are happy with it, {draft,
   publish:true} puts its card in the thread here (publish:true also works
   alongside html or an action). Until then the person sees nothing, so iterate
   as often as you need and publish the version worth their time, not the first
   one. From then on every version you send appears in that card as you send
   it. draft_wait publishes a draft too, since the person needs to
   see it. A draft you never publish is never shown, so do not forget to
   publish the one that is ready. Pass a title again before publishing to
   rename the card.
6. The app saves each draft as it changes, so it comes back live after a
   restart under the same id.
   {draft, action:{kind:"close"}} removes the page for good and leaves a note,
   so close only what the person no longer needs. When the person closes a card
   themselves, the page stays saved and the card offers Open again; you cannot
   bring it back, so a request for it fails and you start a new draft if you
   still need one.

The card is as wide as the conversation column (up to 832 points) and as tall
as the page's content, however tall that is, and it follows the content as it
grows or shrinks; the card never scrolls the page inside itself (past 12000
points it does). Passing height only sets where the card starts. While a new version loads, the
card shows the previous version (or the page's outline) and the page fades in
once it is ready, already at its height, so build pages that are complete on
load rather than assembling themselves in view. width sets the page's layout
width (240 to 1600); wider than the column, it is scaled down to fit, so a
desktop mock stays a desktop mock. Up to three drafts stay live per
conversation; opening a fourth retires the oldest.

The page runs for each of your requests whether or not the person can see its
card: before you publish it, in a conversation in the background, or scrolled
out of view. Start and drive a draft whenever it helps; a published card shows
the latest version when the person gets to it. draft_wait is the exception: it
waits for the person, who has to see the card, here or on a paired phone. A new draft's first call that
fails leaves nothing behind, so just try again. Several screenshots in a row
are fine: they never appear in the thread, only the card does.

## Other widths and the full view

Every new version you send is also laid out, out of sight, at a phone, tablet
and laptop width (375, 768 and 1280). The result carries layouts: the height
each needs and how far it overflows to the right. A horizontalOverflow above 0
at 375 means the page breaks on a phone; fix it unless the page is a fixed
desktop mock. A paired phone runs the page in the thread at its own width, so
mc.state, notes and control work from there too; a page from a dev server shows
there as pictures.

The person can open the draft in a full view over the window. A page you gave
a width keeps it there, up to life size; any other page reflows to fill the
view, so fluid layouts get the room. While it is open, your results describe
the full view's viewport; pass width or height to bring it back to the card.

## Feedback from the person

The card has a Comment button. In comment mode the person clicks anywhere on the
page and writes a note; it is pinned there with a number. Send N notes hands
them to you:

- If you called draft_wait, it returns with event "feedback" and the notes. Use
  it after you ask the person to try the page: say what to look at, then wait.
  A wait lasts up to 30 seconds (event "timeout" if nothing came); call it
  again to keep waiting. "handed back" means they pressed Hand back with no
  notes.
- If you were busy, the notes arrive as a message in the conversation.

Each note has the element it sits on (tag, label or text) and its place as a
percentage of the page, and the pins are numbered in the screenshots, so look
at the page again before you change it. userActions lists what the person did
by hand (clicks on named controls) since you last looked. Answer notes by
revising the same draft, then say what you changed.

If the page uses mc.state, every result also carries state: the current values
of its controls, so you can see what the person chose. Lines the page sent with
mc.send arrive in messages.

Every result also carries issues: things the page itself reveals, such as low
contrast, small tap targets, controls without a name, images without alt text
and clipped text. Fix them or know why they are fine. Contrast is checked in
both themes, whichever the app is in, and names the cause when it can: text
with a fixed color on the thread, text that follows the theme inside a box
that does not, or a data-theme box with no background of its own.

## Human takeover

The user can click and type in the draft. When they do, userControl is true:
snapshots still work, but click, type, replace and resize are refused so you do
not fight them. Use {kind:"resume"} once they say you may continue.

Your clicks and typing are sent as script events, not real input. They never take
focus from the user's composer. Pages that depend on trusted events (file
pickers, clipboard, fullscreen) cannot be driven this way.

## Writing the page

Pass {html} with inline <style> and <script>. Plain HTML, CSS, SVG and JavaScript
work; the optional mc kit (visual_docs {topic:"kit"}) adds components and charts.
For UI mockups use the product's real content, visual identity and working
controls. Keep explanation in the conversation, not in the page.

When the page has to match something that exists (a screenshot or mock the
person gave you, a site, the codebase you are in), read visual_docs
{topic:"tokens"} first: it shows how to take the colors, type, spacing and radii
from the image by measuring pixels or from the code by reading its theme, and
how to write them once as variables the page uses.

## Design for the person

The page is yours, and so are its controls. Start from what the person has to
judge or decide, then invent the controls that let them do it: which states the
subject moves between and what the trigger is called ("Collapse", "Send
message", "Lose connection"), whether motion needs slow motion, a scrubber,
step-by-step frames or a replay, which values are worth a slider, what a
comparison needs to sit side by side. Name everything in the subject's own words.
A draft sits in the thread with no frame of its own, so any control it needs is
part of the page. Some shapes that often fit:
- Compare versions of one component: side by side in one row, driven by one
  shared trigger, each titled in a line. Tabs would hide the difference.
- Review a page or a flow: one page per tab or step.
- Judge motion: a trigger that flips between the states, plus whatever timing
  control the motion needs. Without motion, none of these.
- Tune values: controls bound to the thing they change, close to it.
- Approve a look: a single static page with no controls.
These are defaults, not rules: build what the intent calls for, and leave out
what it does not. The kit (visual_docs {topic:"kit"}) supplies the parts that
are hard to make yourself, such as a clock that slows every animation on the
page and state the agent can read and set; its ready-made bars are examples.

Make the page look like part of the conversation:
- Leave html, body and the outermost element without a background; the page
  sits on the thread's own background. The app clears the page's own html and
  body background, and removes a full-width outermost fill in --background or
  --popover and reports it as an issue.
- No outer card, border or banner title around everything: the card already
  carries the title, and the page is part of your reply.
- Fluid width with no side padding on the outermost element.
- Theme the frame, not the subject. Headings, prose, labels, axes, legends
  and controls sit on the thread and follow the app's theme. A mock of a
  screen, a site or a brand keeps its own colors in both themes: put
  data-theme="light" (or "dark") on its box and give the box a background.
  Inside it the theme variables are that theme's, and text that you do not
  color is that theme's ink, so nothing turns white on a light mock when the
  app is dark. Your own rules for the box still win, and data-theme on html
  or body does nothing. Use it also to show MonoCode in a set theme, or both
  side by side.
- A box that needs its own background (a mock of a screen, a control panel)
  gets at least 16px of padding and var(--radius) corners. That box may paint
  --background, --sidebar and the other tokens itself: only a fill on the
  outermost element is removed.
- When the draft mocks MonoCode itself, or the user's own software, read
  visual_docs {topic:"app"} first, use its design language, and draw the real
  window, sidebar and chrome inside such a box so the mock looks like the app.
  For anything else, be as creative as the intent calls for.
- Revising a draft with the same title in the same turn replaces that draft
  rather than adding a second card; pass a different title for a variant that
  should stay beside it.
After showing a draft, don't describe it again in your reply: say only what the
page doesn't.

## The page talking back

Two ways for the page and you to work together, both optional:
- mc.state({key: value}) holds what the page's controls change. You read it as
  `state` in every result, and you can change it yourself with
  {draft, action:{kind:"set", values:{key: value}}}, which updates the page the
  way its own controls would. Put what you will want to drive by name there:
  the current state, speed, the chosen variant.
- mc.send(text) sends a line to you as from the person, for a control you offer
  such as "Use this one" or "Looks right". It works only straight after the
  person's own click or key press. It arrives in `messages` (a wait returns
  with event "message"), or as a message in the conversation when you are busy.

The app supplies CSS variables for both themes: --background, --foreground,
--muted, --muted-foreground, --card, --popover, --secondary, --border, --input,
--ring, --primary, --accent, --accent-surface, --destructive, --warning,
--success, --info, --code-background, --chart-1 through --chart-6, --radius,
--font-sans and --font-mono. Text sizes are the thread's own: --text-xs (11px,
captions), --text-sm (12px, labels and controls), --text-base (13px, body),
--text-lg (15px, section titles), --text-xl and --text-2xl (18 and 24px, big
numbers), with --leading for line height. --chart-* colors are for marks
(bars, lines, dots); labels and values stay in --foreground or
--muted-foreground, because the bright chart colors do not read as text on a
light thread. A depicted product screen may use its own, inside a data-theme
box.

Avoid 100vh, min-height:100vh and height:100% on html/body; the card is as tall as
the content, so a page sized by its own viewport never settles, and the card
stops following it and scrolls. Use
fluid layouts and SVG viewBox. Respect prefers-reduced-motion.

## Limits

The page has no filesystem, storage or app bridge. fetch, XHR and WebSocket work
over https and wss only, from a null origin with no cookies, so only servers that
allow any origin answer. Never put keys or secrets in a page. http and ws stay
blocked, localhost included: open a dev server with {url} for those. A failed
request appears in console. Scripts and styles load over HTTPS only from cdn.jsdelivr.net,
cdnjs.cloudflare.com, unpkg.com, esm.sh, cdn.skypack.dev, ajax.googleapis.com
and cdn.tailwindcss.com; fonts also from fonts.googleapis.com, fonts.gstatic.com,
fonts.bunny.net and fontshare. Pin versions. Anything else is plain HTML, CSS,
SVG, canvas, WebGL, Web Audio, WebAssembly and JavaScript: the page is yours.
eval and new Function are blocked, so use a library's CSP-safe or precompiled build.
Links open in the user's browser.

Images: use an absolute path, ~/path or https URL in img src, CSS url() or a
JavaScript string. They are embedded before the page opens (PNG, JPEG, GIF, WebP,
AVIF, BMP, ICO, SVG; at most 24 images and 6 MiB). HTML is limited to 512 KiB
without images and 10 MiB with them.

## Running apps

draft_preview {url:"http://localhost:5173"} opens a dev server on this machine in
the same card. localhost, 127.0.0.1 and ::1 only, no credentials, an ephemeral
profile. The page keeps its own styles, network and hot reload. The response
includes the current url and title. Pass {draft, url} to navigate and take a new
snapshot after hot reload or user input. Take
pictures of them with {draft}.

This is WebKit, so Chromium-specific behavior still needs a Chromium check.
Popup windows, downloads, cross-origin iframe controls and recording are not
supported.

## Diagnostics

ready: loaded and fonts settled. horizontalOverflow and verticalOverflow: how far
the content exceeds the viewport. failedImages: images that did not load. An
incomplete or overflowing render needs another look even when loaded is true.
