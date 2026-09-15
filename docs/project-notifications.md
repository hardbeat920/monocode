# Project notifications

Project notification preferences apply to Inbox sounds, linked issue/PR activity,
agent completion sounds, approval/question popups, and reminder delivery. They
also cover desktop banners for agent completion, approvals and questions.
They do not hide activity in sessions or the sidebar, change read/unread state,
stop polling, or change the user's global sound and desktop-notification settings.

Category switches and project mute both govern blue dots beside Inbox in the
sidebar and beside sessions linked to updated work items. A disabled category or
muted project cannot show these indicators. Changes apply immediately, including
automatic mute expiry. Items stay unread and visible; linked update details remain
available. After resume or category re-enabling, unread activity in enabled
categories can show indicators again. This does not replay old sounds or banners.

Unread markers on individual items inside Inbox are independent of notification
preferences. They stay visible while a category is off or the project is muted.
Opening or reopening Inbox does not mark items read; selecting an item or using
"Mark all as read" does. Mute expiry and category changes do not alter read state.

## User controls

The project context menu offers a duration submenu and project settings.
Muted projects show a bell-off indicator in the project rail, including after
restarting the app. Its tooltip and the context menu show the local expiry time
or "Muted until resumed". The indicator moves aside for project options on hover
and visible keyboard focus, and disappears on resume or automatic expiry.
Restoring focus after a pointer action does not keep the hover controls visible.
The Inbox context menu offers timed mute actions for all known projects,
resuming muted projects and opening Settings → Inbox → Project notifications.
Bulk actions include cataloged Inbox-only projects such as Linear. These actions
apply to the catalog and rail projects shown by the menu when the action is
chosen. Bulk actions wait for any unknown local paths to resolve so they cannot
silently skip a project.
Settings shows projects as expandable rows with notification categories in one
column. "Select projects" reveals checkboxes to mute or resume several projects
together; "Done" clears the selection and hides the checkboxes. Preset durations
and the custom-date control are shared between menus and Settings.
Choosing a custom date opens the calendar and local-time field directly, without
repeating duration presets. `DateTimePicker.tsx` supplies the calendar, month and
keyboard navigation, and strict local date/time parsing. The notification form
validates that the selected time is still in the future when it is submitted.
Cancel leaves preferences unchanged; a failed save keeps the chosen date and time
available for retry.

Categories are pull requests/merge requests, issues/Linear tasks, agent finished,
agent approvals/questions, and reminders. Linked work-item activity inherits its
issue or PR category. A Linear project exposes the issues category. Existing app-wide
cues, such as update availability and explicit copy/switch actions, remain global.

Mute presets are one, four and eight hours, until resumed, or a specified local
date and time. A mute override leaves category preferences intact. After expiry
or manual resume, only the selected categories are enabled. UI writes surface
storage failures rather than displaying a successful save that was not persisted.

Category switches are ongoing preferences, not exceptions to a project mute.
For example, turning Issues off and leaving Pull requests on allows PR notifications
while the project is unmuted. Muting the project pauses all delivery; resuming
restores PR delivery while Issues stays off. Settings shows "All notifications
paused" during a mute instead of an enabled-category count. The expanded row
explains that category choices apply after resume. Switches remain editable while
muted, and edits persist without cancelling or extending the mute. The row returns
to its category count automatically at expiry or on manual resume.

## Module responsibilities

- `notificationProjects.ts` resolves and catalogs project identities. Its native
  adapter, `git_notification_context`, only reads local Git metadata. It prefers
  the GitHub CLI's configured `gh-resolved` remote, then the normal Git remote.
  It does not fetch or authenticate with a provider.
- The identity catalog is shared in memory and persisted as a startup cache.
  Known identities are available synchronously, including while Git refreshes.
  Ordinary lookups revalidate in the background after 60 seconds; app focus and
  visibility changes also check freshness. Local Git-change events force a
  refresh. Concurrent discovery is shared, and a Git change during discovery
  schedules a follow-up. A failed background refresh retains known data and
  backs off for five seconds. Unknown paths expose discovery failures for retry.
  Native discovery updates checkout-to-repository associations without moving
  notification preferences from the old repository to the new one.
  Unavailable paths and Git metadata failures reject discovery instead of
  replacing an established repository identity with a local-folder identity.
- `useNotificationProjects.ts` supplies the same catalog to the project rail,
  Inbox actions, and Settings. Opening a menu never clears known identities.
- `notificationPreferences.ts` owns validated, versioned persistence and the
  shared `allowsProjectNotification` decision. Category enable times and mute
  resume times also live here. Subscribers follow writes, other-window storage
  events, focus changes and the nearest mute expiry.
  `allowsProjectNotificationIndicator` applies current category and mute choices
  to unread indicators without consuming unread history. Event delivery also
  checks occurrence-time cutoffs to prevent replaying old sounds and banners.
- `inboxNotifications.ts` tracks fetched item revisions independently of Inbox
  read/unread state. The existing background hook supplies snapshots and keeps
  the badge calculation separate. `inboxNotificationSubject` shares project and
  category classification between sound delivery and indicators, including
  GitLab merge requests and Linear issues.
- `sounds.ts` applies global sound settings and project policy. Project sound
  cues require a typed notification subject. `playCue` reports whether it
  submitted a cue, allowing an Inbox batch to stop after its first eligible cue.
- `notifications.ts` applies the same policy before the existing desktop
  transport. `announceSessionFinished` owns the desktop-banner/audio-fallback
  choice so a rejected or suppressed banner cannot bypass project muting.
- `getProjectNotificationRule` exposes the decision as `{ enabled, after }` for
  native delivery. `after` is the last suppressed millisecond, so an event
  scheduled exactly at mute expiry is eligible. `useSessionReminders` resolves reminder projects and sends
  those rules to the Rust scheduler. The scheduler waits for identity resolution
  for each project before claiming its due reminders. Saved reminders remain
  listed when a folder is unavailable, and known projects can deliver while
  other lookups fail or remain pending. Failed lookups report an error and retry
  on refresh. The scheduler retains suppressed reminders without replaying
  them, and checks current rules before sending each banner. Expiry works while
  the webview is in the background because `after` is an absolute event cutoff.
- Approval and reminder popup lists use the same project policy. Underlying
  requests and reminders remain accessible in their sessions and sidebar.
  Delivery follows catalog identity changes, including background refreshes
  of a cached checkout. Approval requests keep their original observation time.
- `useProjectNotificationPreferences` and the shared UI controls subscribe to
  these stores. React views do not implement notification policy themselves.

## Identity

Hosted repositories use `repository:<host>/<repository>`. Remote transport,
credentials and local directory names are excluded. Checkouts and worktrees of
the same hosted repository therefore share preferences. A changed remote denotes
a different project. Host names keep separate GitLab instances distinct.

Repositories without a hosted remote use the absolute Git common directory;
ordinary folders use their normalized path. Windows paths compare without case.
Linear uses its project UUID. Issues without a Linear project use a separate
unassigned group for each team UUID. Projects can be cataloged from remote Inbox
items without any local folder. Saved catalog entries keep their controls
available after temporary connection failures.

Git host aliases and custom SSH-to-HTTPS host/port mappings are not inferred.
Repositories addressed through different host aliases have different identities.
No heuristic merges unrelated repositories or Linear projects with local folders.

## Time and refresh behavior

Deadlines are absolute Unix milliseconds, so restarts do not extend a mute.
Delivery checks the clock directly; UI timers only refresh displayed state.
An event's occurrence time prevents delayed activity from the muted interval
from being delivered after expiry. Manual resume and category re-enabling keep
  similar cutoffs. Re-enabling global sounds also suppresses older project activity.

The first successful Inbox snapshot establishes a silent baseline for each
provider. A changed fetch scope establishes a new baseline, avoiding alerts for
history revealed by filters or a newly opened project. Failed providers do not
establish a baseline. Item revisions are retained across partial refreshes, and
invalid timestamps are ignored. All fetched revisions are observed before
visibility or notification preferences are applied. Muting, reading an item, or
resuming notifications does not replay a previously observed revision.

One refresh submits at most one Inbox sound. A muted project cannot prevent an
eligible project from sounding, even when the shared unseen indicator is already
on. Detection retains the existing provider polling and pagination limits; it
does not promise delivery of every upstream event between snapshots.

Approval and question popups use their first observation in the popup list
because the session protocol does not retain request occurrence times. That
timestamp lasts for the mounted request lifecycle. Persisting request event
times across restarts would require an addition to the session protocol.

## Extending and validating

Add a category to `NOTIFICATION_CATEGORIES` and classify the event at its source.
Pass its project identity, category and occurrence time through the shared policy
before any new delivery channel. Do not derive sound eligibility from the badge
or implement category checks in a view. New desktop channels should retain their
own global opt-in and platform permission checks.

Focused tests cover policy/expiry, delayed events, persistence validation,
repository and Linear identity, Inbox revisions, desktop suppression and UI
interactions. `npm run check:web` runs the frontend suite and TypeScript check.
`npm run build` builds the production frontend. The native integration test is
`git_notification_context_resolves_local_project_identity`; the repository's
`npm run check:rust` also checks formatting, Clippy and all native tests.
