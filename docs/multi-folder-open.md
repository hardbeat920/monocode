# Opening several folders at once (PR #443 follow-up)

Three review findings, all confirmed in the code, all with the same root cause.
Not yet fixed — this is the plan.

## The findings

1. **Only the first folder inherits the active session's provider.**
   `openProjectInOwnTab` seeds from `sessionsRef.current[0]`, while
   `onSelectProject` seeds from the *active* session
   (`current ?? sessionsRef.current[0]`). So later folders can open with a
   different harness than the one in front of the user.

2. **An already-open folder gets a duplicate blank tab.** `onSelectProject`
   consults `planProjectReturn`, which returns `activate` for a project that is
   already open. `openProjectInOwnTab` skips that entirely and always creates.

3. **New tabs appear in reverse order.** `appendTab` →
   `insertBesideActive` → `insertTabBesideActive(prev, tab, activeTabIdRef.current, …)`.
   The ref is not updated between two synchronous calls, so every tab is
   inserted next to the *same* tab, and the last one selected ends up first.

## Root cause

All three come from the same shortcut: only the first folder goes through
`onSelectProject`; the rest take a parallel path that reimplements part of it
and drops the rest.

The parallel path exists for a real reason — the one fixed earlier in this PR.
`onSelectProject` reads refs, React does not render between synchronous calls,
so calling it in a loop planned every folder against stale state and each call
reused the same blank session. Bypassing it fixed that and introduced these
three.

## The fix: one state transition

Do not loop over a per-folder function at all. Build everything from a single
snapshot and commit it in one update, so no step depends on a render that has
not happened:

1. Read the snapshot once: active tab, active session (the provider seed),
   and the set of projects already open.
2. Walk the selected paths, deciding per path against that snapshot **plus what
   earlier paths in this same walk already produced**:
   - already open (in the snapshot or earlier in the walk) → activate it, create nothing
   - otherwise → create a session seeded from the active one, and a tab
3. Apply once: `setSessions(prev => [...prev, ...created])`, and fold the new
   tabs into `setTabs` in selection order — each inserted next to the previous
   one in the run, not next to `activeTabIdRef.current`.
4. Focus the last selected folder, and remember every opened project
   (`rememberProject`) once.

`insertTabBesideActive` already takes the anchor id as a parameter, so step 3
needs no change to it — thread the previously created tab's id through the fold
instead of reading the ref.

This also removes `openProjectInOwnTab`; the ordering, dedup and seeding rules
live in one place again.

## Why not "await a render between calls"

Calling `onSelectProject` in a loop with `await new Promise(r => requestAnimationFrame(r))`
between iterations would make all three symptoms disappear, and it is a much
smaller diff. It is not the right fix: it makes correctness depend on when React
commits and when a ref assignment lands, neither of which is a contract. It
would work until a render is batched differently.

## Tests

The current tests cover `pickFolders` (the dialog wrapper) only — the bug is in
what the app does with the paths, which is untested. Worth adding, against the
selection walk extracted as a pure function so it can be tested without React:

- several folders → one session and tab each, in selection order
- a folder already open → activated, no new session
- provider seed → every created session inherits the active session's harness,
  not the first session in the list
- one folder → unchanged from today's behaviour
- dismissed dialog → nothing happens

Extracting the decision walk is what makes this testable; the current shape
(refs read inside a callback) is not.
