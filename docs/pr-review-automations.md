# Re-review changed pull requests

In **Automations**, add **GitHub → Pull request opened** and **GitHub → Pull
request head changed** to the same review automation. Choose its project,
repository filters, conversation reuse, workspace and permission mode as usual.
Head changes apply to open, non-draft PRs. Draft pushes establish a new baseline
without launching a review; add **Pull request ready for review** if desired.

For example, the automation prompt can say:

> Review the linked PR at the supplied current head. Read earlier review findings
> and check which remain valid. Report remaining and newly discovered issues.
> Follow the posting permissions specified in this prompt.

The trigger supplies the repository, PR number/URL, previous observed head and
current head. It does not interpret findings or grant permission to post, edit
code, approve or merge. Those policies belong in the prompt and existing agent
permission settings. A selected workspace is not automatically checked out to
the PR head; the reviewer must inspect the supplied revision.

## Detection and catch-up

- Detection uses the existing shared Inbox refresh (normally every 30 seconds),
  its bounded GitHub list/lookup requests and caches. It adds the head SHA to
  those responses, with no per-agent polling or extra per-PR fetch loop.
- The first observation of each PR in a project establishes a baseline. Enabling
  the trigger does not review every existing PR. Head comparisons use SHA
  equality, so force pushes count and comments, labels and description edits do
  not. Existing opened-event claim keys remain unchanged.
- Baselines and pending events survive restart in local app storage. After a
  disconnect or shutdown, the next successful refresh compares the saved head
  to the current head and can review the latest revision. Intermediate revisions
  while offline are not reconstructed. Unknown PRs still only establish a
  baseline. Clearing/unavailable local storage loses this catch-up history.
- Detection and retry cover PRs returned by the current Inbox query, project
  selection and list limits, plus its existing bounded missing-item lookups.
  A saved event waits when the PR is absent or GitHub refresh fails; it never
  launches using only saved open/draft state. A confirmed closed, merged or draft
  PR drops pending head work. Hidden PRs resume when observed again.

## Deduplication and active reviews

Each automation claims a head SHA once per PR. Repeated refreshes or a force push
back to an already accepted SHA do not repeat that head-change review. The opened
trigger remains an independent event.

While that automation has an active event run for the same PR (including its
opened review), newer heads remain pending. Only the latest observed revision is
kept; intermediate heads are coalesced. The running review is not interrupted.
Its next head review starts on a later successful refresh after settlement.
Separate automations can still run independently.

A rejected launch retains the same durable run for retry on the next refresh;
a newer head can supersede it before launch. An accepted turn that later fails or
is cancelled is not automatically repeated. Dispatch is reserved transactionally
before submission to prevent duplicate concurrent launches. As with existing run
recovery, a process exit during dispatch or an active review marks it interrupted
on restart and does not resubmit that same revision automatically: a crash cannot
prove whether the agent accepted it. Pending, unreserved launch retries still
require fresh GitHub confirmation after restart.
