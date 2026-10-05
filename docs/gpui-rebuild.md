# GPUI rebuild plan

Branch: `gpui-native`, cut from `monocode-connect` at `0dd3b29`.

The goal is to replace the Tauri webview app (React, TypeScript, Vite) with a native Rust app drawn by GPUI, and to remove every line of TypeScript, including the Node remote host in `host/`.

## Decisions

| Topic | Decision |
| --- | --- |
| Language | Everything moves to Rust. No TypeScript remains: the harness, the reducer, the engine logic in `App.tsx`, and `host/` all get ported. |
| GPUI base | zui (`zeronsh/zui`) at `667d0aa`, plus `zeronsh/gpui-component` at `2f73e5c`. This is the pair that comet pins. |
| Transition | The Tauri app keeps building and passing its tests until the GPUI app reaches parity. Tauri-free Rust moves into shared crates first, and `src-tauri` becomes thin command wrappers over them. |
| Design | Match the current look: the HSL theme tokens, glass and opacity settings, spacing, type scale, and motion timings. |
| User data | The new app opens existing data without conversion. It uses the same identifier (`com.monocode.desktop`), the same `monocode.db` with migrations continuing from v18, and the same `blocks_json` shape. |

## Verified on this machine (2026-10-02)

- Rust 1.98.1 on an M3 Pro with CommandLineTools only, so there is no `metal` compiler. GPUI's `runtime_shaders` feature compiles the shaders at launch instead.
- A scratch app on zui `667d0aa` with `gpui-component` `2f73e5c` builds in 3m44s from a cold cache with `opt-level = 2` for dependencies. It opens a blurred window wrapped in `gpui_component::Root`.
- That dependency graph does not contain zui's `path` crate, which is the only GPL-3.0 crate in zui. Every gpui crate we link is Apache-2.0.
- `gpui-kit 0.7.0` (crates.io `gpui-pre 0.3.7`) also builds and runs here, in 75s. We can fall back to it if zui stops being maintained.

## What has to move

| Area | Today | Size (non-test lines) |
| --- | --- | --- |
| UI views | React components in `src/app` and `src/features` | 83k TSX plus 3.8k CSS |
| Agent harness | `src/integrations/harness`: framing, 11 provider adapters, reducer, previews | 30.5k TS |
| Engine logic in the UI | `App.tsx`: submit pipeline, event batching, queues, handoffs, side threads, orchestration, control executor, automations timer | most of 11.4k TS |
| Feature models | `src/features/*/model`: orchestration (2.2k), settings (1.4k), sessions (models, transcript activity), inbox providers, automations triggers | 38k TS |
| Remote host | `host/`: Node server, sync, pairing, TLS, and its own SQLite store. It bundles the harness above rather than its own copy. | 7.6k TS |
| Native backend | `src-tauri/src`: 269 commands, of which about two thirds are plain Rust | 46.7k Rust |

There are also 89k lines of TypeScript tests. The protocol tests (`*Protocol.test.ts`, `*Live.test.ts`, `apply.test.ts`) become Rust golden tests before their TypeScript sources go away.

## Target layout

```
Cargo.toml                  workspace
crates/
  monocode-core             Session, Block, HarnessEvent, settings, model catalog types.
                            Serde shapes round-trip today's blocks_json and settings JSON.
  monocode-store            SQLite: sessions, notes, reminders, automations, orchestration runs,
                            workspace snapshot, checkpoints.
  monocode-git              Files, git, worktrees, project search.
  monocode-integrations     GitHub (gh), GitLab, Jira, Linear, Azure DevOps, link previews, inbox media.
  monocode-process          Agent CLI process supervisor, binary resolution, control server.
  monocode-harness          JSON-RPC and ACP framing, one module per provider, tool previews,
                            shell intent. The transcript reducer lives in monocode-core.
  monocode-engine           Everything in App.tsx that is not drawing: submit pipeline, batching,
                            queues, orchestration, automations scheduler, control server, agent app API.
  monocode-remote           Host protocol client and server, SSH bootstrap, TLS pinning.
  monocode-terminal         PTY (vendored portable-pty) and the terminal model (alacritty_terminal).
  monocode-platform         macOS, Windows, and Linux native code: notifications, pasteboard, dock badge,
                            window blur, global hotkey, tray.
  monocode-ui               Theme, icons, popover, modal, menus, toasts, and composer input widgets.
  monocode-markdown         Streaming markdown renderer with code highlighting.
  monocode-terminal-view    Terminal element on alacritty_terminal.
  monocode-editor           Code editor and diff views on gpui-component's editor.
apps/
  monocode-app              GPUI binary. `monocode-app host` runs headless and replaces the Node host.
                            `app` and `control` subcommands keep the agent CLI wire format.
src-tauri/                  Thin #[tauri::command] wrappers over the crates until cutover. Its own
                            workspace and lockfile, because upstream GTK crates pin a toml_datetime
                            version that GPUI's build cannot share.
```

The engine is a set of GPUI entities, the same way Zed's remote server runs its models. Views observe engine entities directly. The headless host runs the same entities under `gpui_platform::headless()` with no window, which is how one Rust codebase serves both local and remote sessions. Provider adapters in `monocode-harness` stay free of GPUI and use runtime-agnostic async, so their tests run without a UI.

## Milestones

Each milestone ends with something runnable and a check that proves it.

### M0. Foundations

- Create the workspace layout. Move Tauri-free code from `src-tauri` into `monocode-store`, `monocode-git`, `monocode-integrations`, `monocode-remote`, and `monocode-terminal`. Leave `#[tauri::command]` wrappers in place.
- Write `monocode-core` types from `session.ts`, `core/types.ts`, and the settings model.
- Bring up `monocode-app` on zui: theme tokens ported from `src/styles/index.css` and `appearance.ts`, transparent window with the blur radius and the sidebar and main pane opacity, title bar with traffic-light inset, project rail, and a read-only session sidebar loaded from the real `monocode.db`.
- Exit check: `npm run check` still passes. A golden test loads every session in a copy of a real `monocode.db`, deserializes it, serializes it again, and gets byte-equivalent JSON. The GPUI app lists real sessions.

### M1. One working provider

- Port the reducer (`core/apply.ts`), framing (`jsonRpc.ts`, `acp.ts`), the process supervisor, and the Claude adapter, with the Claude protocol tests ported as golden tests.
- Build the transcript view with streaming markdown, tool calls, approvals, and questions. Build a basic composer.
- Persist sessions through `monocode-store`.
- Exit check: from the GPUI app, start a Claude session, stream a reply, approve a tool call, quit, relaunch, and resume. The session also opens in the Tauri build.

### M2. All providers

- Codex, then the ACP group (Cursor, Grok, fx, Antigravity, Droid, Hermes), then Pi and omp, then OpenCode with its HTTP and SSE transport.
- Model picker, model settings, permission modes, usage chips, account switching, provider sign-in.
- Exit check: every provider's ported protocol tests pass, and a live smoke turn works for each installed CLI.

### M3. Workspace

- Tabs, split panes with drag-to-edge, tab groups, workspace snapshot restore.
- File tree, file editor (gpui-component editor with git gutter and find and replace), file picker, image and PDF viewers.
- Terminal and terminal dock.
- Git changes panel, unified diff with hunk staging, branch and worktree pickers, history graph.
- Composer at full parity: skills, @mentions, MCP tags, mode commands, attachments, message queue, side threads.

### M4. Secondary views

- Inbox for GitHub, GitLab, Jira, Linear, and Azure DevOps, including PR checks and diffs.
- Notes, search, automations, reminders, notifications, skills, MCP settings.
- All settings sections and keybinding overrides.
- Settings import: on first launch, read the old values from WebKit's `localstorage.sqlite3` under `~/Library/WebKit/com.monocode.desktop` (and the matching Windows and Linux paths), then store them in a settings file.

### M5. Agents driving the app, quick composer, remote

- Orchestrator, control server, the `/operator` agent app API, handoffs, second opinion.
- Quick composer as a non-activating panel with a global hotkey, and the quick git popup.
- Remote connections over the existing host protocol. The headless host mode of `monocode-app` replaces `host/`, and the SSH bootstrap downloads a prebuilt binary instead of running `npx monocode-host`.
- Exit check: an existing pairing works against the Rust host, and the old desktop build can still talk to it during rollout.

### M6. Cutover

- Packaging for macOS (signing, notarization, dmg), Linux (deb, rpm, AppImage), and Windows (NSIS). Updater. Tray on Windows.
- CI on the three platforms runs `cargo fmt`, `clippy`, and `test` only.
- Delete `src/`, `host/`, `index.html`, `quick-composer.html`, the Vite and Vitest config, `package.json`, and `src-tauri`.

## Risks

- Upstream drift. `hardbeat920/monocode` keeps shipping TypeScript fixes, especially for provider protocols. After the port, every upstream fix must be ported by hand. Porting the protocol tests first makes that cheaper, because an upstream test change can be replayed against the Rust adapter.
- zui pin. It is a git-only fork with a small maintainer base. Bumping the pin means following comet's pin pair. `gpui-kit 0.7` on crates.io is the fallback.
- Transcript rendering. Streaming partial markdown, text selection across blocks, code highlighting, and virtualization of long sessions need native behavior checks. Mermaid fences now render SVG through `mermaid-rs-renderer`, coalesce streaming updates, and allow source toggling. Unsupported diagrams retain their source.
- Editor parity. CodeMirror carries 4.4k lines of custom extensions. The gpui-component editor in the fork (0.5.2) is older than the one in gpui-kit 0.7.
- In-window backdrop blur. zui has a BackdropBlur element on Metal, wgpu, and Windows, so popovers can keep their frosted look. Upstream GPUI does not.
- Licensing. Zed's `editor`, `terminal_view`, `markdown`, `ui`, and `theme` crates are GPL-3.0. Copy nothing from them. Comet (MIT), termy (MIT), and tty7 (Apache-2.0) are safe references.

## References

- Comet and Zeron, MIT, GPUI on zui with a Rust harness for Claude Code and Codex: `~/src/zeron-droid-harness`, https://github.com/zeronsh/comet
- zui: https://github.com/zeronsh/zui
- gpui-component fork: https://github.com/zeronsh/gpui-component
- awesome-gpui: https://github.com/zed-industries/awesome-gpui
