# Opening several folders at once (PR #443 follow-up)

Three review findings, all with the same root cause. This is what was wrong and
what the code does instead.

## What was wrong

1. **Only the first folder inherited the active session's provider.**
   `openProjectInOwnTab` seeded from `sessionsRef.current[0]`, while
   `onSelectProject` seeded from the *active* session
   (`current ?? sessionsRef.current[0]`). Later folders could open with a
   different harness than the one in front of the user.

2. **An already-open folder got a duplicate blank tab.** `onSelectProject`
   consulted `planProjectReturn`, which returns `activate` for a project that is
   already open. `openProjectInOwnTab` skipped that entirely and always created.

3. **New tabs appeared in reverse order.** `appendTab` →
   `insertBesideActive` → `insertTabBesideActive(prev, tab, activeTabIdRef.current, …)`.
   The ref is not updated between two synchronous calls, so every tab was
   inserted next to the *same* tab, and the last one selected ended up first.

All three came from the same shortcut: only the first folder went through
`onSelectProject`, and the rest took a parallel path that reimplemented part of
it and dropped the rest.

That parallel path existed for a real reason — the one fixed earlier in this PR.
`onSelectProject` reads refs, React does not render between synchronous calls,
so calling it in a loop planned every folder against stale state and each call
reused the same blank session. Bypassing it fixed that and introduced these
three.

## One state transition

Nothing loops over a per-folder function now. `planProjectOpenRun`
(`src/features/projects/model/projectOpenRun.ts`) takes a snapshot — the return
memory, the tabs, the sessions, the active tab id — plus the selected paths, and
answers with the steps to take in selection order. It touches no ref and no
React state, which is also what makes it testable.

The walk decides each path against that snapshot *plus what earlier paths in the
same walk already produced*: a folder open in the snapshot, or opened a step
ago, activates instead of creating, and a created session is visible to the
paths behind it. New sessions are seeded from the active session, once, for the
whole run. Each created tab carries `besideTabId` — the tab created before it in
the run, and the snapshot's active tab for the first — so the run keeps its
order without consulting a ref that has not moved.

`openProjects` in `App.tsx` reads the snapshot once, runs the walk, and applies
it: `setSessions` appends every created session, one `setTabs` folds the created
tabs in with their anchors, the folder chosen last ends up focused, and every
opened project is remembered. `onSelectProject` is now `openProjects([path])`
and `pickProject` is `openProjects(await pickFolders())`, so the ordering, dedup
and seeding rules live in one place.

The walk carries all four decisions `planProjectReturn` returns, not just
activate-vs-create: `keep` and `reuse-blank` are what make one folder behave as
it always did. `reuse-blank` stays a step the caller hands to `onCwdChange`,
because that function owns the retargeting rules — `retargetSessionToProject`,
and `keepSessionChanges` for the project being left. The invariant that matters
is that the walk emits at most one `reuse-blank` per run: there is only one
blank session to take, and the earlier bug was every folder in turn taking it.

`insertBesideActive` split into `insertBeside(prev, tab, anchorId, cwd)` and a
wrapper that passes `activeTabIdRef.current`. The fold needs an explicit anchor;
every other caller wanted the active tab and is unchanged.

## Why not "await a render between calls"

Calling `onSelectProject` in a loop with `await new Promise(r => requestAnimationFrame(r))`
between iterations would make all three symptoms disappear, and it is a much
smaller diff. It is not the right fix: it makes correctness depend on when React
commits and when a ref assignment lands, neither of which is a contract. It
would work until a render is batched differently.

## Tests

`src/features/projects/model/projectOpenRun.test.ts` covers the walk directly,
in seven cases: several folders each get their own session and tab in selection
order, anchored to the one before; a folder already open is activated; a folder
the run itself just opened is activated too; every created session inherits the
active session's harness rather than the first session's; the blank session is
taken by the first folder only, and the rest are created; a single folder still
produces `keep`, `activate`, `create` or `reuse-blank` as it did before; and a
dismissed dialog or a path that is no project produces nothing.

`pickFolders` (the dialog wrapper) keeps its own tests in
`src/platform/tauri/fs.test.ts`.
