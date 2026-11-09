# Composer text on both screens

Text typed in a thread's composer and not sent belongs to the thread, not to
the screen it was typed on. Typing in a thread on the desktop and then opening
the same thread on the paired phone shows the same text in the phone's
composer, and the other way round.

## What travels

- **The text.** All of it, as typed, including `/commands`, `$skills` and
  `@file` mentions, which are plain text in both composers.
- **Not attachments.** The desktop's files are paths on the Mac and the phone's
  are files on the phone that are only uploaded when the message is sent.
  Moving them would mean uploading every picked photo while it is still a
  draft. Each screen keeps its own files. When the phone changes or clears the
  text, the desktop composer's files stay where they are.
- **Not the desktop's mode chip** (Plan, Orchestrator) or the model. The
  phone has no modes, and the model is already a thread setting both screens
  share.
- **Threads only.** A new session's composer has no thread yet, a worker
  thread is driven by its lead and has no composer of its own, and a Mono's
  chat is left out for now.

## Where the one copy lives

The desktop store, in the row it already keeps: `composer-draft:<session>` in
`native_settings` (`Store::save_composer_draft` / `composer_draft`). The row
gains a `rev` field: the time in milliseconds the text last changed, always
higher than the last one. A cleared draft is an empty row, which reads as
revision 0. Because revisions are fresh timestamps, a revision never comes
back after a clear, so an old revision held by an offline phone can never
match a newer draft by accident.

`Store::composer_text` reads `{text, rev}`. `Store::write_composer_text` takes
the phone's text and the revision it was based on, and saves only when that
revision is still current; it keeps the row's attachments.

## How the phone reads and writes it

- **`composer.read`** `{conversationId, since}`: a long-poll.
  It answers `{text, rev}` at once without `since`, and otherwise when the
  revision moves past `since`, or after 15 seconds with the same one. The
  desktop checks the stored row every 250 ms while it waits. The phone runs one
  read at a time while a thread's composer is on screen and the app is active.
- **`composer.write`** `{conversationId, text, base}`: goes through
  `mobile_command`, so a retried command ID gets the first answer back. It
  answers `{accepted, text, rev}`, where `text` and `rev` are the desktop's
  copy afterwards. When the thread is open on the desktop, the composer there
  changes too; otherwise only the store does (and the sidebar's Draft mark).
- **When.** The phone writes 700 ms after the last keystroke, and at once when
  the field loses focus, the screen goes away or the app goes to the
  background. The desktop already saves 150 ms after typing stops.

Neither the session catalog nor the transcript watch carries the text. The
catalog goes to every screen of the phone and is rebuilt as a whole, which is
too heavy to resend while someone types, and the watch frames feed the iOS
native transcript, which has no use for composer text. One small long-poll per
open thread keeps it separate.

The catalog does carry a `draft` flag per session, for the Draft mark on the
phone's session rows. It counts a saved draft or composer text, including the
thread open on the desktop, because that thread isn't open on the phone. It
trails a change by up to a publish and a poll, so while text here is unsent,
and for 10 seconds after it last changed or was saved, the row follows the
phone's own text (`localMark` in `composerDraft.ts`). That also marks text
typed while the desktop is away. Like the desktop's mark, it gives way to
Working.

## When both screens edit

- **The person is typing here.** A screen does not replace its text with the
  other's while the person has typed in the last 3 seconds (phone) or 2
  seconds (desktop), or has a change not yet saved. The phone keeps the newer
  revision it heard of and applies it once the person stops, if they have not
  changed the text meanwhile.
- **A write from an old revision.** The desktop refuses a phone write whose
  `base` is not its current revision (unless the text is already the same)
  and answers with its own copy. What the phone does with that:
  - If the person typed on the phone in the last 3 seconds, they are typing
    there now, so the phone's text wins: it takes the new revision as its base
    and writes again on the next pause.
  - Otherwise the write is old, for example made offline and sent once the
    link came back. The desktop's copy wins and the phone shows it. A stale
    phone draft never wipes a newer desktop one.
- **The desktop is typing.** A phone write that arrives while the desktop
  composer was typed in during the last 2 seconds is refused the same way.
- **Offline.** The field stays open and only Send waits for the desktop. The phone keeps its text and unsent change per thread in memory
  while the app runs, so leaving the thread and coming back still shows it and
  retries the write. It is not saved to the phone's disk.

## Sending clears both

- **From the desktop.** Sending empties the composer, which saves an empty row
  (revision 0). The phone's read answers and the phone empties its field,
  unless the person is typing on the phone right then.
- **From the phone.** `conversation.send` carries `draftRev`, the revision the
  phone's text was based on. Before the message goes in, the desktop clears its
  composer's text when its revision is still `draftRev` or its text is the
  sent text, so the phone's message is not queued behind its own copy. Newer
  desktop text stays. The desktop composer's files stay.

## Decisions taken without the person

- Attachments, the mode chip and the model do not travel (above).
- On a real conflict (both edited, and the phone is not being typed in) the
  desktop wins and the phone's text is dropped rather than merged or kept as
  a second copy.
- The phone does not save its unsent text to disk, so an offline draft is lost
  if the app is killed before the link returns.
- When the phone sends, the desktop composer's own files are left in place.

## Where the code is

- Store: `crates/monocode-core/src/store/composer_draft.rs` (revisions, the
  guarded write, the "a phone send uses this text up" rule), and
  `save_composer_draft` in `store.rs`.
- Desktop composer: `mobile_composer_write` and
  `clear_composer_text_sent_from_phone` in
  `crates/monocode-gpui/src/conversation/follow_up.rs`, which also records
  when the person last typed (`ComposerDraft::typed`).
- Phone requests: `crates/monocode-gpui/src/shell/mobile_host/composer.rs`;
  `draftRev` in `mobile_send` in `mobile_host.rs`; both methods are in
  `APP_METHODS` in `crates/monocode-core/src/mobile/host.rs`.
- Phone: the pure rules in `mobile/src/features/sessions/composerDraft.ts`
  (tested in `mobile/tests/composerDraft.test.ts`), the read loop and writes
  in `useComposerDraft.ts`, used by `Composer.tsx`. The composer is the same
  React Native view on iOS and Android.
