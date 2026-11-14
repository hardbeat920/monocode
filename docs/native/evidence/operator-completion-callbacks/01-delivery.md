# Delivery and lifecycle audit

Reviewed the branch against origin/main. Inspected session authorisation, start/send admission, worker outcome classification, store round trips, app restart restoration, composer ownership, asynchronous report preparation and final dispatch. No runtime capabilities were broadened in children and the self-session management check remains.

The initial behavioural review found a callback could be marked delivered before launch_prompt rejected a removed provider account. The regression test reproduced the consumed notification. Delivery now uses ready_for_coordinator_report both before loading and immediately before consumption. It combines existing composer/queue readiness with the same account availability condition as launch_prompt. The test holds a completed subscription pending, restores the default account and confirms one report.

The initial report wording called all undelivered siblings still running, including terminal siblings held behind a busy origin. The new regression test reproduced this. The heading now says Other reports pending, which remains true whether a sibling is active or awaiting delivery.

The thermonuclear audit rejected keeping final race validation, delivery persistence and dispatch buried inside the background completion closure. These now live in deliver_operator_completion. The scheduling method gathers current outcomes and starts report loading; the delivery method independently rechecks parent eligibility and group membership before changing state. This is an ownership boundary rather than a generic wrapper. The persist-before-dispatch order is retained.

I considered replacing this feature with the Orchestrator or Mono queues. Those operate on different ownership and authorisation models. Reusing them would widen this PR, require ordinary project chats to become managed leads or Monos, and risk changing the existing restrictions. The dedicated Operator completion module is the narrower canonical home.

The saved model has a typed turn-or-label group key. Labels are local to the parent, validated at the API boundary and normalised on admission. Existing helper paths still own session creation, prompt submission and project restrictions. The launch payload now uses named fields rather than six positional fields with neighbouring booleans.

Known delivery policy: at most once. Delivery is persisted before dispatch, so a process crash in between can lose the follow-up. This is documented in docs/agent-access.md. It is not a claim of exactly-once or guaranteed delivery. Archived/deleted parents lose their subscriptions. Restart classifies unfinished provider turns as stopped.

Commands used: git diff origin/main...HEAD; git show origin/main:<file>; rg of readiness and launch paths; native callback regression tests; persisted live-demo subscription and transcript inspection. Both actionable behavioural findings failed before their fixes. Final validation is recorded in summary.md.
