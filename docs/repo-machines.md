# One project, many machines

A project in the rail is a repository, not a folder on one computer. It can have one folder (a "location") on each machine: this computer, and any paired MonoCode Host. Each new session picks which machine it runs on before its first message. After the first message the session stays on that machine.

This follows T3 Code's approach (pingdotgg/t3code, `packages/client-runtime/src/state/projectGrouping.ts` and `BranchToolbarEnvironmentSelector.tsx`). Hosts keep owning their own projects and sessions. The grouping lives only on the desktop, and a repository identity groups folders but never moves work between machines.

## Terms

- **Location**: a project path as the app already stores it. A local location is an absolute path. A remote location is `remote://<environmentId>/<hostPath>`, as built by `remoteProjectKey`.
- **Home**: the location that represents the project in the rail. Per-project settings (logo, groups, pins, rail order, sidebar tab) stay keyed by the home path, so nothing existing needs migration.
- **Member**: any other location linked to a home.

A project has at most one location per machine. Machine identity is `local` for this computer and the environment id for a remote location.

## Storage

One key, `monocode.projectMachines.v1`, in localStorage (Tauri) and in Kv (native):

```json
{
  "links": { "<member path>": "<home path>" },
  "separate": ["<path>", "..."],
  "identities": { "<path>": "<canonical repo key>" }
}
```

- Keys are normalized project paths (`normalizeProjectPath`). Compare them with `pathKey`.
- `links` never chains. Linking a home that already has members moves those members to the new home.
- `separate` lists locations the user unlinked. Automatic linking skips them. Linking one by hand removes it from `separate`.
- `identities` caches the canonical repository key of each location (see below). A missing entry means unknown. An empty string means "not a Git repo or has no remote".
- Changes fire `monocode:project-machines-changed` (Tauri) or bump the Kv revision (native).

Operations:

- `projectHome(path)`: the home for a member, else the path itself.
- `projectLocations(path)`: the home first, then its members. Order the members with the local one first, then remote ones by machine name.
- `locationMachine(path)`: `local` or the environment id.
- `linkProjectLocation(home, member)`: refuses when `home`'s project already has a location on `member`'s machine, and returns false. Otherwise it records the link, moves `member`'s own members to `home`, and drops `member` from `separate`. The rail stops listing `member`.
- `unlinkProjectLocation(member)`: deletes the link, adds the path to `separate`, and remembers it as a recent project so it reappears in the rail. Unlinking a home promotes its first member to home and moves the rest under it.
- `forgetMachineLocation(path)` (`forget_machine_location` in Rust; `forgetProjectLocation` already exists for folder identity): drops a path from links, separate, and identities. Call it when a project is removed from the rail. Removing a home promotes its first member, the same as unlinking.

## Repository identity

`normalizeGitRemoteUrl(url)` must give the same answer in TypeScript and Rust. Shared test vectors are at the end of this file.

1. Trim. Return an empty string for an empty input.
2. Remove a trailing `/`, then a trailing `.git`, then any trailing `/` again.
3. If it has a scheme (`scheme://`), drop the scheme and any `user@`. Drop a `:port` after the host.
4. Otherwise, if it matches the scp form `[user@]host:path` and `host` contains no `/`, use `host/path`, with any leading `/` of path removed. A single letter before the colon is a Windows drive, not a host.
5. Otherwise it is a local path remote. Return `file:` followed by the path with `\` turned into `/`.
6. Azure DevOps: `ssh.dev.azure.com/v3/<org>/<project>/<repo>` becomes `dev.azure.com/<org>/<project>/_git/<repo>`, and `<org>.visualstudio.com/<project>/_git/<repo>` becomes `dev.azure.com/<org>/<project>/_git/<repo>`.
7. Lowercase the whole result.

The URL comes from the `origin` remote. Without `origin`, use `upstream`. Without either, use the first remote in name order. No remote gives an empty identity.

- Local: a new desktop command, `git_remote_url(cwd) -> Option<String>`, runs `git -C <cwd> remote` and `git remote get-url <name>`.
- Remote: hosts add an optional `remoteUrl` field to the project objects returned by `projects.open` and `projects.list`, using the same remote choice. Older hosts omit it, and their projects link only by hand.

## Automatic linking

`autoLinkProjects()` runs after the rail loads, whenever a remote project is added, and whenever a machine's `projects.list` returns. It works like this:

1. It collects the rail projects: every recent project that is not already a member.
2. It fills missing identities. Local paths use `git_remote_url`. Remote paths use the `remoteUrl` that the host's `projects.list` returned for that machine. Store it on the remote project record as `remoteUrl`, and in `identities`.
3. It groups rail projects that share a non-empty identity and are on different machines. It skips any path in `separate`.
4. For each group, the home is the existing home with the most members. A local path wins a tie, then the path opened first. Every other path links to that home. A machine that already has a location in the group is not linked twice. The most recently opened path wins that machine, and the others stay separate rail projects.

Linking is silent. The user sees one rail entry where there used to be several.

## Rail

- Members are hidden. `collectRailProjects` maps each recent path to `projectHome(path)` before deduplicating.
- The selected project is `projectHome(activeCwd)`.
- A project with more than one location shows its machine count, such as "2 machines", where a remote project shows its machine name today. The tooltip lists each machine with its path.
- The project context menu gets a "Machines" section:
  - "Add on another machine…" opens the existing "Open folder on a machine" dialog in link mode for this project.
  - "Add folder on this computer…" appears only when the project has no local location. It opens the folder picker and links the result.
  - "Unlink from <machine>" appears once for each member.

## Sessions sidebar

The Sessions list shows sessions from every location of the active project, merged by recency:

- Local rows come from the local location's history (`session_list_by_project(localLocation)`).
- Remote rows come from each remote location's `sessions.list` poll.
- Each row carries its location's path as `cwd`, so actions route by row: rename, archive, pin, delete, link work item, and select. Remote rows use their own machine and host project id.
- When the project has more than one location, each row shows a small machine label ("This Mac", "Mini").
- Git, Files and Changes keep following the active session's location, as they do today.

## Run on (machine picker)

The composer's top bar gets a machine picker before the project and branch pickers. It shows in both local and remote composers.

- Label: the current location's machine ("This Mac" on macOS, "This PC" on Windows, "This computer" on Linux, or the host's machine name), with a monitor or globe icon.
- Shown when the project has more than one location, or when at least one machine is paired.
- Enabled only while the session can still change its workspace. That means the same rule as the workspace picker (`draftWorkspace`), which is "before the first message" for a remote session.
- The menu lists each location with a check on the current one. Picking another location moves the blank session to that path through the existing `onCwdChange` retarget path. That clears branch, worktree and workspace mode, so the pane switches between the local and remote session views.
- Below a separator: "Add on another machine…" (link mode dialog) and, if missing, "Add folder on this computer…".
- After the first message the picker is a read-only label.

New sessions start on the active session's machine, as they do today. Opening a project from the rail opens its home.

## Add a folder on a machine in link mode

The existing dialog takes an optional `linkTo` home path:

- The title becomes "Add <project> on a machine".
- Machines that already have a location for this project are disabled, with "Already added".
- After `projects.open` succeeds, the app calls `linkProjectLocation(linkTo, key)` instead of adding a new rail project. If the session that opened the dialog is still blank, the app moves it to the new location.

## Test vectors for `normalizeGitRemoteUrl`

| Input | Output |
| --- | --- |
| `git@github.com:T3Tools/T3Code.git` | `github.com/t3tools/t3code` |
| `https://github.com/T3Tools/T3Code.git` | `github.com/t3tools/t3code` |
| `https://user@github.com/a/b/` | `github.com/a/b` |
| `ssh://git@github.com:22/a/b.git` | `github.com/a/b` |
| `git://example.com/a/b` | `example.com/a/b` |
| `ssh://git@gitlab.example.com:2222/group/sub/repo.git` | `gitlab.example.com/group/sub/repo` |
| `git@ssh.dev.azure.com:v3/Org/Proj/Repo` | `dev.azure.com/org/proj/_git/repo` |
| `https://Org@dev.azure.com/Org/Proj/_git/Repo` | `dev.azure.com/org/proj/_git/repo` |
| `https://org.visualstudio.com/Proj/_git/Repo` | `dev.azure.com/org/proj/_git/repo` |
| `/srv/git/app.git` | `file:/srv/git/app` |
| `C:\repos\app` | `file:c:/repos/app` |
| (empty) | (empty) |
