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
