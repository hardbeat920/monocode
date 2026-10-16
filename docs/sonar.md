# Sonar inbox

Open **Settings → Inbox → Sonar**, enter the server URL, project keys and a
personal access token, then select **Connect Sonar**. Project keys are the
`sonar.projectKey` values in your scanner configuration; separate multiple keys
with commas. MonoCode checks token validity and Browse access before saving.

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
Opening a finding loads its analyzed source with the reported lines highlighted,
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
rule guidance. The Work in project selector first matches names from configured
Git remotes, read locally without contacting the Git server. It then tries the
project display name or folder name, and finally falls back to the current
project, the first project, or the current working directory. Manual selections
are kept when issue details finish loading.
Severity, type and status use friendly labels such as Critical, Code smell and
False positive while retaining Sonar's original API codes. The status control
shows only transitions available to the current account. A status change can
include an optional comment; sharing it with Sonar as analyzer feedback is off
until explicitly selected. Available transitions are rechecked before submission,
and successful changes update the inbox and reload issue details. Accept and
False positive require **Administer Issues** permission.

Issue reads are paginated, up to Sonar's 10,000-result search limit per branch.
If a branch exceeds that limit or a request fails, its findings cannot be fully
loaded and Inbox reports the error while retaining the scanned branch roster and
findings from successful branches. Refresh after waiting if Sonar rate-limits
requests. Only branches still retained by Sonar appear; deleted branch history
and pull-request analyses are not branch options.

**Disconnect** removes the saved credentials and clears cached Sonar content.
Credentials stay in the app's local data directory, with owner-only file
permissions on Unix.

API references: [Sonar Web API](https://docs.sonarsource.com/sonarqube-cloud/appendices/web-api),
[issue search](https://sonarcloud.io/web_api/api/issues/search), and
[project branches](https://sonarcloud.io/web_api/api/project_branches/list).

Refreshes read at most four branches concurrently. A 60-second budget stops
starting additional requests, including pagination, while an active request can
still finish within its 20-second timeout. Partial results include a warning;
scanned branches already discovered remain available in the filter.
