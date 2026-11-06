# Sonar inbox

In the native GPUI app, open **Settings → Inbox → Sonar → Connect**, enter the
server URL and a personal access token, then select **Connect**. Choose an
organization (SonarQube Cloud) and check the projects to watch. SonarQube Server
lists accessible projects without an organization. MonoCode validates the token
before saving it locally. The connection can also be managed from Inbox’s
**Add connection** menu.

Select **Sonar** in Inbox, then open the filter menu to select one watched
**Sonar project** at a time. Organization, watched projects and the selected
project persist across restarts. Changing organization clears the watched
projects. Project keys correspond to `sonar.projectKey` in scanner configuration.

Cloud project discovery requires an organization; it never searches the global
public-project catalog. If organization discovery fails or returns no memberships,
enter the organization key from your SonarCloud organization URL and choose
**Use key**. The error stays visible and **Reload discovery** retries discovery.

Once connected, the dialog prioritizes project selection, with six project rows
visible at its normal size. **Edit connection** opens the credentials and
organization controls; **Back to projects** returns to the project list.

The dialog shows token validation, organization loading and project-page loading
separately. **Close** stays available during requests, cancels unfinished token
validation, and discards late discovery replies. Project results show names and
keys on separate lines in a virtualized list. Discovery loads 100 projects at a
time; **Load next 100 projects** explicitly requests the next page. The search
field filters loaded results and previously watched projects, including saved
selections absent from the current page. **Select all** and **Clear selection**
apply to that matching list, preserving selections outside the search; load more
pages first to include additional projects. Watching or selecting projects saves
the choice without repeating discovery. After connecting, **Done** closes the dialog;
enter a replacement token to enable **Update token**.

Use `https://sonarcloud.io` for the EU SonarQube Cloud instance or
`https://sonarqube.us` for the US instance. Self-hosted SonarQube URLs, including
server context paths, are supported when the branch and issue APIs are available.
Use a personal token with **Browse** permission on every configured project.
Scoped organization tokens that grant only Execute analysis cannot read issues.

Select **Sonar** in Inbox. The filter menu shows **Scanned branches**, grouped by
repository name, with the main branch marked. Names come from the SCM link,
falling back to the Sonar display name or project key when unavailable.
This roster comes from Sonar's branch API
and includes branches with an analysis date even when they have zero issues.
Branch selections persist locally and filter the displayed findings. Hidden
branches remain in the menu, and assignee, status, time and search filters do not
remove branch options.

Findings show severity, rule, status, branch, file and line. Open the finding in
Sonar, discuss it, or start a session in a local project with that context.
Opening a finding loads its analyzed source with the reported lines marked with `>`,
related code locations, rule rationale, fix guidance, code examples, comments,
effort and quality metadata when Sonar provides them. Source access requires
**See Source Code** permission in addition to Browse.

When both branches have analyzed source, the pane compares the affected file
against the project's main branch. This diff compares the latest analyzed source
on each branch, rather than the local working tree or an exact PR merge base.
Rule documentation with paired noncompliant/compliant examples also shows an
example diff, clearly distinguished from changes to your file.

Rule descriptions come from the connected instance's API. When that response
omits descriptions for C# or VB.NET rules, MonoCode falls back to the official
SonarSource analyzer documentation on GitHub. The pane labels this as the latest
reference documentation because it may differ from your installed analyzer.
Other rules without API descriptions retain a link to their documentation.
Source and documentation load failures preserve the other available content.
Starting a session or discussing a finding includes its source excerpt and loaded
rule guidance. The local project selector first matches names from configured
Git remotes, read locally without contacting the Git server. It then tries the
project display name or folder name, and finally falls back to the current
project, the first project, or the current working directory. Manual selections
are kept when issue details finish loading.

Severity, type and status use friendly labels such as Critical, Code smell and
False positive while retaining Sonar's original API codes. The status control
shows only transitions available to the current account.
Select a transition, optionally enter a comment below the finding, then choose
**Apply**. Sharing the comment with Sonar as analyzer feedback is off until
explicitly selected. Available transitions are rechecked before submission,
and successful changes update the inbox and reload issue details. Accept and
False positive require **Administer Issues** permission.

Issue reads are paginated, up to Sonar's 10,000-result search limit per branch.
If a branch exceeds that limit or a request fails, its findings cannot be fully
loaded and Inbox reports the error while retaining the scanned branch roster and
findings from successful branches. Refresh after waiting if Sonar rate-limits
requests. Only branches still retained by Sonar appear; deleted branch history
and pull-request analyses are not branch options.

**Disconnect** removes the saved credentials and clears cached Sonar content.
Credentials stay in `sonar-config.json` in the app’s local data directory, with
owner-only file permissions on Unix. Native release builds reuse the installed
desktop app’s credential file. Native development builds use their separate data
directory, as described in [native development](native/README.md).

API references: [Sonar Web API](https://docs.sonarsource.com/sonarqube-cloud/appendices/web-api),
[issue search](https://sonarcloud.io/web_api/api/issues/search), and
[project branches](https://sonarcloud.io/web_api/api/project_branches/list).

Refreshes read at most four branches concurrently. A 60-second budget stops
starting additional requests, including pagination, while an active request can
still finish within its 20-second timeout. Partial results include a warning;
scanned branches already discovered remain available in the filter.
