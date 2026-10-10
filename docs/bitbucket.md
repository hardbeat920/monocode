# Bitbucket Cloud inbox

Open **Settings → Inbox → Bitbucket**, enter your Atlassian account email and a
Bitbucket API token, then select **Connect**. The connection is validated before
credentials are saved. Create a token from
[your Atlassian account](https://id.atlassian.com/manage-profile/security/api-tokens)
with these Bitbucket scopes: `read:user:bitbucket` (used to validate the token
and find your account), `read:repository:bitbucket`, `read:pullrequest:bitbucket`
and `write:pullrequest:bitbucket`. Bitbucket Data Center and Server are not
supported.

Repositories are found from your local projects' git remotes (`bitbucket.org`),
the same way as GitLab and ADO. Select the Bitbucket source in Inbox to browse
pull requests, read descriptions, comments and diffs, post comments,
ask about an item, or start work in a local project.

The **Builds** tab on a pull request lists the builds Bitbucket reports for its
head commit: Pipelines and third-party builds such as SonarCloud. It is called
Builds rather than Checks because Bitbucket's own merge checks (such as "no
commits behind") are a different thing, and they are not available through the
API. Each row shows its result and duration and opens the build on Bitbucket.
This reads commit statuses, so it uses the `read:repository:bitbucket` scope, and
it refreshes every 30 seconds while the tab is open.

Pipelines builds can be expanded to show their steps. That needs one more,
optional, scope: `read:pipeline:bitbucket`. Atlassian API tokens cannot be
changed after they are created, so include it when you create the token. Without
it the build still expands and says which scope is missing; to turn steps on,
create a new token with the scope and reconnect. Builds from other services
(SonarCloud and the like) have no steps and just link out.

**Needs attention** shows open pull requests where you are a reviewer or the
author. Bitbucket has no cross-repository to-do feed, so this is answered per
repository for the projects you have open.

Only pull requests are supported: Atlassian removed the Bitbucket issue tracker,
including its API, in August 2026. Long comment threads show the latest 100
comments, with a link to Bitbucket.

Automations offer **Bitbucket → Pull request opened** after connecting. They run
when a pull request first appears in the polled inbox, after its initial
snapshot. This is polling, not a webhook.

**Disconnect** removes the saved credentials and clears cached Bitbucket content.
Credentials are stored in the app's local data directory; on Unix the file is
created with owner-only permissions.
