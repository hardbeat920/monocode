# Jira Cloud inbox

Open **Settings → Inbox → Jira**, enter your Jira site, Atlassian account email,
and an API token **without scopes**, then select **Connect**. The connection is
validated before credentials are saved. Create a token from
[your Atlassian account](https://id.atlassian.com/manage-profile/security/api-tokens).
Scoped tokens and Jira Data Center are not supported by this integration.

Select the Jira source in Inbox to browse issues, read descriptions and comments,
post comments, ask about a ticket, or start work in a local project. Starting work
and asking about a ticket include its description and Jira identifier.

Project selections are shared between Settings and the Inbox filter menu.
Unchecked projects are excluded from fetching and background notifications.
The inbox loads up to 40 open issues or 100 issues when including closed history,
ordered by last update. An unfiltered all-status query covers the last year of
activity. Long comment threads show the latest 50 comments, with a link to Jira.

Automations offer **Jira → Issue appeared** after connecting. They run in the
automation's selected local project when a Jira issue first appears in the polled
inbox, after its initial snapshot. The project-key filter uses values such as
`ENG`. This is polling, not a webhook for every issue created on the site.

**Disconnect** removes the saved credentials and clears cached Jira content.
Credentials are stored in the app's local data directory; on Unix the file is
created with owner-only permissions.
