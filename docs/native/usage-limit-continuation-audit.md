# Usage limit continuation audit

Audit date: 2026-10-25. Scope: the supplied desktop screenshot and current GPUI source. This is a proposal; no application behavior was changed or run against user data.

## Findings

1. **P1: Scheduled continuation is absent from the thread list.** `shell/rails.rs::classic_session_status` handles approval, working, done, draft, and age, but does not inspect usage limits. A blocked or scheduled thread can therefore look finished or inactive. The provider icon identifies the provider, not whether the thread will continue.
2. **P1: The banner does not clearly confirm the user's choice.** `conversation/accounts.rs::usage_notice` changes "Resume at reset" to "Resuming at reset" when armed. The same control both enables and cancels continuation. The screenshot shows "Resume at reset", which in the current code means auto-resume is not armed when the reset is in the future. A known reset time alone does not schedule continuation.
3. **P1: Dismiss also cancels continuation.** The banner's close action clears `settings.usage_limit`, including `resume_at_reset`. Its tooltip says only "Dismiss usage limit notice". Users cannot distinguish hiding a notice from cancelling scheduled work.
4. **P1 verification gap: The automatic continuation promise needs a lifecycle check.** The 30-second timer lives in `Conversation`; `shell/session_loading.rs` explicitly keeps restored background tabs metadata-only. Settings persist the limit, but this audit has not established that every armed thread is loaded and scheduled after restart. Do not promise closed-app execution without an independently running scheduler.
5. **P2: Reset and execution are presented as the same time.** `usage::Limit::resume_due` waits until 30 seconds after reset, and the conversation polls every 30 seconds. "Resumes in" should refer to the retry schedule, not imply exact execution at the provider reset instant. A retry may encounter another limit.

The existing persisted `Limit` fields are sufficient to distinguish blocked from armed. The banner already supplies a reset time, countdown, status role, and account/model recovery controls. Keep those foundations.

## Proposed experience

Keep threads in their existing positions. Replace the sidebar's age/status slot with a compact text status; preserve title, branch, and provider/model information. Do not add another sidebar section in the first pass.

| Condition | Sidebar | Conversation notice | Action |
| --- | --- | --- | --- |
| Limited, auto-resume off | Warning icon + Usage limit | Usage limit reached. Auto-resume is off. Show reset time if known. | Resume at reset when a time is known; preserve manual retry otherwise. |
| Armed, future retry known | Clock + Resumes in 28m | Will continue automatically after the limit resets at 12:10 AM. | Cancel auto-resume |
| Armed, time unavailable | Clock + Waiting for reset | Auto-resume is on. Checking when the limit resets. | Cancel auto-resume |
| Retry due, launch pending | Clock + Resume pending | Waiting to retry. | Cancel auto-resume |
| Turn actually starts | Existing Working status | Remove waiting notice. | Existing stop control |
| Retry hits another limit | Usage limit, or a new schedule only if still armed | Explain the new limit and actual retry state. | Explicit recovery controls |

Use the existing amber warning treatment and clock asset. Always include text; color, provider icons, and tooltips must not carry the state alone. Keep waiting static rather than using a working spinner. Include the full local date and time in the tooltip when the compact countdown is shown. At narrow widths, fall back to "Scheduled" with the full time available on inspection.

Approval and active execution retain precedence. A limit or scheduled retry must precede Done, Draft, and age. Clear stale Done indications when a limit stops the turn. Apply the same meaning to other thread-list layouts.

Make the notice's heading and accessible status describe the current state. Keep model/account switching available. Remove the close control for the scheduled state in the initial version; "Cancel auto-resume" is the explicit way to stop it. If hiding is later needed, model visibility separately from scheduling.

Continuation is on by default: a limited thread is armed as soon as the limit is detected, and "Cancel auto-resume" turns it off for that thread. Prefer "Resumes" over generic "Snoozed": it tells users what will happen and distinguishes this feature from existing notification reminders.

## Implementation order

1. Verify scheduling for an unselected thread and after restart, including an overdue schedule. If scheduling depends on opening the thread, fix restoration of armed work before displaying a reliable continuation promise. Establish and disclose the app-running requirement.
2. Expose limit state through sidebar metadata for live and unloaded threads. Do not instantiate every conversation or read storage during row rendering. Reuse persisted settings and update the metadata when arming, cancelling, retrying, or switching providers/accounts.
3. Add the sidebar status and explicit banner states. Ensure the arm/cancel handler invalidates the sidebar as well as the conversation. Reuse the existing countdown formatting and shared refresh cadence.
4. Verify state transitions and inspect the finished native UI at normal and narrow sidebar widths, in both themes.

## Acceptance checks

- Enable auto-resume, select another thread, and see a persistent scheduled label with a decreasing countdown. Cancel it and see Usage limit immediately.
- A limited turn never appears as Done. A running turn never appears as scheduled.
- Multiple threads sharing a reset remain individually discoverable and resume according to their own armed state. A timer tick cannot launch the same thread twice.
- Restart with a future and overdue armed thread. Verify continuation without opening either thread, or explicitly resolve the scheduling limitation before shipping the promise.
- Unknown reset time, a refreshed reset time, account/model switching, and a second limit all show the actual state. Cancelling during history restoration must prevent a deferred automatic launch.
- Verify what closing the app, system sleep, archiving, and deleting a thread do to scheduled continuation. Never leave a visible promise for work the scheduler will not perform.
- Keyboard and accessibility inspection exposes the current schedule and a named cancellation action. Long titles and narrow widths preserve readable status.

These are proposed behavioral checks, not completed tests. No database access or changes were performed during this audit.
