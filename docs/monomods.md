# MonoMods

MonoMods lets anyone extend Monocode with a folder that holds a `mod.json` and a `register(on, $)` entry. A mod listens to session and tool events, calls host capabilities through `$`, and returns element trees that Monocode renders with its own components, so the result looks native in both themes.

The public contract is [`src/features/mods/api/types.ts`](../src/features/mods/api/types.ts) (API version 1). Names follow Claude Code Mods wherever the concept exists, so ports stay mechanical.

## Goals

- **Native look.** Mods return data (element trees), never HTML or CSS. Colours are semantic tones mapped to theme tokens.
- **Isolated by default.** Mod code never runs in the main webview. Every privileged call is checked in Rust against the manifest.
- **Small v1.** Only what real mods needed while building the API, plus the obvious render sites. Everything else is marked *later*.

Non-goals for v1: a marketplace, signing, mods that rewrite or deny tool calls, custom client components (`Client`), raw HTML.

## Mod folder and discovery

```
my-mod/
  mod.json          # ModManifest
  src/register.tsx  # entry, exports `register`
  src/...           # any relative imports
```

```json
{
  "name": "pr-stacks",
  "version": "0.1.0",
  "apiVersion": 1,
  "description": "Your PR stacks for this repo",
  "entry": "src/register.tsx",
  "permissions": { "process": ["git", "gh"], "fs": { "read": ["$PROJECT/**"] } },
  "userConfig": { "trunk": { "type": "string", "title": "Default trunk", "default": "main" } }
}
```

Discovery paths, scanned in this order (first `name` wins; Settings shows each mod's source and folder, and warns when it hides another mod with the same name):

1. `<project>/.monocode/mods/*/mod.json`: project mods. Off until the user enables them for that project.
2. `~/.monocode/mods/*/mod.json`: user mods.
3. "Load unpacked" in Settings > Mods: any folder, remembered by path. Dev mode: hot reload on.

## Lifecycle

1. **Discover.** Rust scans the paths and returns manifests (`mods_list`). Invalid manifests show up in Settings with the error, never loaded.
2. **Enable.** Settings > Mods shows the permissions as a consent sheet. `process` entries that run any code they are given (shells, `rm`, `env`, `find`, `xargs`, `osascript`, interpreters, `git`, `gh`) show as "full shell access". The sheet also says every mod sees session tool calls and their output. Rust records the consent together with a hash of `mod.json`. A changed manifest asks again.
3. **Bundle.** The host bundles the entry with `esbuild-wasm` and caches it in memory per window, keyed by the `mod.json` hash. Enabling or reloading a mod rebuilds it from the current source.
4. **Load.** The host mounts a hidden `<iframe src="monomod://localhost/<name>">` and posts `{kind:"load", code, config}`. The frame starts a Worker from a blob, the worker runs `register(on, $)`, then replies `ready` with its subscriptions.
5. **Run.** The host routes events into the worker and answers its RPC calls. Each `ui.render` is requested again after `invalidate`, after an atom write, or after a store write made by the mod.
6. **Hot reload.** For unpacked mods, a file-change event or the Reload button sends `unload`, tears down the frame, re-bundles and loads again. In-memory state is lost; `$.store` survives.
7. **Disable / crash.** The frame is removed. A worker that throws in `register` or is not `ready` within 10 s is marked crashed in Settings (the ping after `ready` is *later*). Its views stay open and show an error state; a disabled mod's open views say it is off.

## Loading mechanism

**Chosen: a sandboxed `monomod://` frame that spawns a blob Worker.** This copies the artifact pattern (`src-tauri/src/artifacts.rs:193-249`, registered at `src-tauri/src/lib.rs:256`).

- The scheme serves one fixed bootstrap page. It is GET only, `<name>` is checked against the enabled mods, and the CSP goes out as a response header: `sandbox allow-scripts; default-src 'none'; script-src 'unsafe-inline' blob:; worker-src blob:; connect-src 'none'`.
- `tauri.conf.json:31-32`: add `monomod: http://monomod.localhost` to `frame-src` in both `csp` and `devCsp`. The http form is for Windows.
- The frame has an opaque origin and no Tauri IPC. The worker inherits `connect-src 'none'`, so `fetch` and `WebSocket` are dead. The only way out is `postMessage` to the host, and the host checks `e.source === frame.contentWindow` on every message (same as `artifacts.rs:204`).

Rejected:
- **A blob or Vite worker in the main origin.** `worker-src blob:` on the main CSP would inherit `connect-src asset:` over `$APPDATA/**` (`tauri.conf.json:31,38`). A mod could then read the session store.
- **A Worker on a `monomod:` URL straight from the main window.** That is cross-origin, and the `Worker` constructor rejects it.
- **A Tauri window per mod.** Too heavy, and the `default` capability (`capabilities/default.json:5`) applies to `"windows": ["*"]`.

Spike first: confirm WebKit (macOS), WebView2 and WebKitGTK all apply the frame's CSP to blob workers.

## Transpile

**`esbuild-wasm@0.28.2`** (the same version as the existing `esbuild` dev dependency), lazy-loaded in the main window and run once per content hash. Mods are multi-file TSX with relative imports, so a transpiler alone (Sucrase, `ts.transpileModule`) would still need a module linker. Settings: `format: "iife"`, `jsx: "automatic"` with a tiny host `jsx` runtime that builds `Element` objects, and `external: ["monocode", "claude-code"]`, both resolved by a shim that exposes `atom/read/update` and the `jsx` factory. The bundler reads files through `mod_fs`-style Rust reads limited to the mod folder.

## Security model

- **Trust boundary is Rust.** `mods.rs` re-reads the installed manifest and consent record on every privileged call. The frontend list of permissions is for display only.
- **process.** `argv[0]` must be a bare name listed in `permissions.process`; paths are refused, so a mod cannot run a binary it ships. The name is resolved on the login-shell PATH plus the usual fixed dirs (`/opt/homebrew/bin` and friends). `env` keys that make tools run other code (`GIT_*`, `HOME`, `BASH_ENV`, `NODE_OPTIONS`, pagers, editors, `LD_*`/`DYLD_*`, ...) are refused. Timeout defaults to 30 s, and stdout is capped at 1 MB (`isStdoutTruncated`). Shells and `rm` are all-or-nothing and labelled that way at consent.
- **fs.** Read only in v1. The path is canonicalised (symlinks resolved), then matched against the expanded globs. `$PROJECT` is the active project root at call time.
- **http.** Hostname allowlist. Loopback, link-local and private ranges are refused after DNS resolution.
- **env.** Only names in `permissions.env`.
- **store.** One JSON file per mod folder, `$APPDATA/mods/<name>/store-<folder hash>.json`, 1 MB cap. Saved `userConfig` is keyed by folder too, so a same-named mod elsewhere cannot read either.
- **Rendering.** Trees are data. Unknown element types render nothing. `Markdown` drops raw HTML. `openUrl` takes http(s) only. `openUrl`, `openFile` and `copy` work only within 5 s of a press on one of the mod's views, because any mod can read tool output. Tones are a closed set, so there is no hex and no style injection.
- **Resource limits.** RPC calls in flight per mod are capped at 32. A render that takes over 2 s shows the last tree.

## Events

Events come from one tap: the harness flush callback at `src/app/App.tsx:1611-1625`, which sees each batch with the session state before and after.

| MonoMods event | Derived from |
|---|---|
| `session.start` / `session.end` | the first batch for a new session id / `session.ended` |
| `turn.start` / `turn.complete` | `busy` flipping on any session-state change (an effect on `sessions`, since the composer and the turn's end set `busy` outside the flush); `turn.complete` carries `durationMs` and the turn's `turn.metrics` |
| `context.update` | the harness `context` event (`used` / `window`) |
| `tool.call` | `tool.started` (handler runs) + `tool.updated` with a terminal status (`next` resolves) |
| `workspace.change` | an effect on `sidebarCwd` / `active?.id` (`App.tsx:1884-1918`) |
| `command.run` | the composer slash-command path, for names in `$.command.register` |
| `ui.render` / `ui.focus` | the render sites below |

`tool.call` payload: `tool` maps from the tool kind to a Claude name. `file_path` and `paths` come from `preview.path` / `paths`. `command` comes from the shell preview (`preview.kind === "shell"`). `result.stdout` comes from `preview.output`. A subagent's tool steps (harness `agent.step` with `kind: "tool"`) come through as `tool.call` too, with `agentId` set to the parent Agent call's id, the same id `session.agents` returns. Check each provider fills these. Where one does not, the field is `undefined`, and mods must treat it as optional. Known gaps that skip the tap: btw threads (`App.tsx:9484`), compaction status (`App.tsx:9959-9976`), and cursor subagents (`providers/cursor/cursorSubagents.ts:122`). None of them matter for v1.

## Render sites

| Site | v1 | Where it plugs in |
|---|---|---|
| `SidebarTab` | yes | `SidebarTabId` widened to `` `mod:${name}:${id}` `` (`settings/model/appearance.ts:109`), `isSidebarTabId` (`:807`), and `loadSidebarTabOrder` merges missing ids instead of resetting on a length mismatch (`:832-848`). `projectSidebarTab.ts:6,9-11` accepts `mod:` ids. Labels and icons are looked up with a registry fallback in place of `TAB_LABELS` / `COMPACT_TAB_ICONS` (`app/shell/Sidebar.tsx:216,223`). The strip is at `:1620-1660`, the rail at `:2655-2666`, and the body gets one `tab.startsWith("mod:")` branch after `:2109`. |
| `Pane` | yes | A virtual editor tab, **not** a new `SurfaceKind`. `mod?: {name, viewId}` goes on `FilePaneTab` (`workspace/model/layout.ts:58-83`), with `isModTab` next to `isReleaseNotesTab` (`:411`) and inside `isVirtualDocumentTab` (`:433`). Branch in `files/ui/FilePane.tsx:186-206` beside `ReleaseNotesSurface`, title and icon in `workspace/ui/SurfaceTabs.tsx:117`, dedupe key in `App.tsx:13550`. Live only: dropped by `workspace/model/workspaceSnapshot.ts:562-600`, like agent tabs. Helpers copy `app/model/releaseNotesWorkspace.ts`. |
| `Status` | yes | Chips in `UsageFooter` (`app/shell/UsageFooter.tsx:117-168`) before the right-aligned group at `:135`, styled like `SessionChip` (`:520`). Driven by `$.ui.status`, not by render. |
| `Toast` | yes | One `ModToasts` host portalled at `LAYER.toast` (`shared/lib/layers.ts`), joined to the `topOffset` chain in `App.tsx:13385-13405`. Driven by `$.ui.toast`. |
| `AbovePrompt` | yes (owned by B) | `Composer`'s unused `children` slot (`sessions/ui/Composer.tsx:294,409`, rendered at `:1728`). Passed from `SessionPane.tsx:648-785`. The Mono dock (`:1137-1175`) needs its own slot. |
| `ToolUse` | later | Badges before `ToolCallStatusIcon` in `AgentTranscript.tsx` (`:4468`, `:4763`, `:4774`), keyed on `block.tool.id`, next to `monoCodeToolCall` (`:4412`, `:4698`). |

### Element renderer

| Element | Monocode component |
|---|---|
| `Box`, `Text` | plain elements with token classes (`text-content/55`, `border-stroke`, `bg-selection`) |
| `Button` | `shared/ui/SecondaryButton.tsx`; icon-only uses `IconButton` (moved out of `app/shell/TitleBar.tsx:454`) |
| `Input` | the settings text input style |
| `Badge` | one `Badge` pulled out of `RunStatusPill` (`AutomationsView.tsx:734`) into `shared/ui/Badge.tsx` |
| `Icon` | `shared/ui/icons.tsx`, plus a new string-name map |
| `Spinner` | `sessions/ui/TerminalSpinner.tsx` |
| `Skeleton`, `Dot` | small new `shared/ui` pieces |
| `Markdown` | `MarkdownPreview` (`sessions/ui/AgentMarkdown.tsx:717`), with HTML off |
| `Tree` | a stateless `TreeRow` pulled out of `TreeNode` (`files/ui/FileTree.tsx:1206`, row classes about `:1303`). Rows with `path` get `FileTypeIcon` and git colour from `useGitFileStatuses` (`source-control/hooks/useGitFileStatuses.ts:142`). `FileTree` itself is not reused, because it has editing built in. |

Tones map to tokens in `src/styles/index.css:25-100`: `accent` to `--color-accent`, `success`/`danger` to the diff add/del colours, and `read`/`write`/`git` to purple, orange and green. The categorical tones get one new token each, defined for both themes (dark default, `html.theme-light` at `:147`).

## Rust commands (`src-tauri/src/mods.rs`)

| Command | Notes |
|---|---|
| `mods_list()` | scans the discovery paths and returns manifests, errors, enabled state and consent hash |
| `mods_set_enabled(name, enabled, manifest_hash)` | records consent in `$APPDATA/mods/consent.json` |
| `mods_read_source(name, rel_path)` | for the bundler; confined to the mod folder |
| `mod_exec(name, argv, cwd, timeout_ms, env)` | checks `permissions.process`, reuses `exec_output` (`harness.rs:1486`) |
| `mod_fs(name, op, path)` | `read`/`list`/`stat`; reuses `read_text_file_sync` (`fs.rs:5814`) and `list_dir_sync` (`fs.rs:535`) |
| `mod_http(name, method, url, headers, body)` | ureq like `harness_http` (`harness.rs:1234`), with the allowlist and a private-IP block |
| `mod_store(name, op, key, value)` | `get`/`set`/`delete` |
| `mod_env(name, key)` | allowlisted keys only |
| `monomod` URI scheme | bootstrap page, registered next to `artifacts::HTML_SCHEME` (`lib.rs:256`) |

Hot reload uses the existing file watcher on unpacked folders. If there isn't one for arbitrary paths, Reload stays a button in v1.

## Porting from Claude Code Mods

| Claude Code Mods | MonoMods |
|---|---|
| `plugin.json` + `hooks.json` | `mod.json` (`entry`, `permissions`, `userConfig`) |
| `register(on, options)` | `register(on, $)`; options are `$.config` |
| `import { atom, read, update } from 'claude-code'` | `$.state.atom/read/update` (the `claude-code` import is shimmed too) |
| `$.state.get/set` with `ifVersion` | not needed: the worker is single-threaded, so use `$.state.update` |
| `$.session.cwd()` / `id()` | `(await $.session.current()).cwd` / `.id` |
| `$.session.messages()` replay | `$.session.toolCalls(sessionId)` |
| `$.agent.list()` | `$.session.agents(sessionId)` |
| `$.ui.resolve(e)` → `{Box, Text, Button, Input}` | import the JSX tags from `monocode` |
| `$.ui.copy({text})` → `{isCopied}` | `$.ui.copy(text)` (check `.isCopied`) |
| `process.run(['open', url])` | `$.ui.openUrl(url)`; files: `$.ui.openFile(path)` |
| hex colours, Catppuccin palette, omarchy theme | `Tone` names |
| `` ` LABEL ` `` pills, `▆▃` skeleton, `●` | `Badge`, `Skeleton`, `Dot` |
| `Client` + `rows.tsx`, `ui.message`, `ui.scroll`, scroll maths | `Tree` with `active`, `reveal`, `onPress`, `onDoublePress`, `onContextMenu`, `onKey` |
| braille spinner / per-character shimmer timer | `Spinner`, `Text shimmer` |
| `classic.SessionStart` / `classic.FileChanged` on `.git` | `turn.complete` + a timer (later: `fs.watch`) |
| `prompt.submit` | later |
| `placement`, `presentation`, Nerd Font detection, SSH/herdr checks | delete |
| `tool.call` middleware that rewrites or denies | not supported; observe only |

## Examples

`examples/mods/hello/` is the smallest working mod: a sidebar tab, a status chip, a toast on `turn.complete` and a band above the prompt. `examples/mods/README.md` explains how to load it.

Two larger mods were built against this API to test it and can follow as separate PRs: a port of a Claude Code mod that shows your PR stacks across repos (`process` with `git`/`gh`, `store`, `tool.call`), and a mod with zero permissions that shows turn time, tokens and context fill.

## Later

The `ToolUse` site, `prompt.submit`, `fs.write`, `fs.watch`, `tool.call` rewrite/deny, a `Link` element, signing and a registry, per-mod CPU limits, and a `monocode/testing` mock kit.
