# Asana inbox

Open **Settings → Inbox → Asana**, paste a Personal Access Token, then select
**Connect**. The token is validated before it is saved. Create one from the
[Asana developer console](https://app.asana.com/0/my-apps).

Select the Asana source in Inbox to browse tasks, read descriptions and comments,
post comments, ask about a task, or start work in a local project. Starting work
and asking about a task include its description and Asana task ID.

Project selections are shared between Settings and the Inbox filter menu, where
each project is labeled with its workspace. Unchecked projects are excluded from
fetching and background notifications.

The Asana source mirrors **My Tasks**: it only shows incomplete tasks assigned
to you, up to 500, so the **Assigned to me** and **Status** filters are hidden
for it. Its **Due** filter defaults to **Next 7 days**: tasks due today through
seven days from now, plus overdue ones. Tasks without a due date appear only
with **Any time**.

Link each Asana project to a MonoCode project with the dropdown next to it in
Settings. The Asana source then shows only your tasks from the Asana projects
linked to the selected MonoCode project; a project with no link shows a prompt
to add one. A task in several Asana projects belongs to the first linked one.
Links are stored locally in the app.

Automations offer **Asana → Task appeared** after connecting. They run in the
automation's selected local project when an Asana task first appears in the
polled inbox, after its initial snapshot. This is polling, not a webhook for
every task created in the workspace.

**Disconnect** removes the saved token and clears cached Asana content. The
token is stored in the app's local data directory; on Unix the file is created
with owner-only permissions.
