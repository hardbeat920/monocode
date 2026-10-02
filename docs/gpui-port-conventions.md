# GPUI port conventions

Read this before working on any crate under `crates/` or `apps/`. The plan and milestones are in `docs/gpui-rebuild.md`.

## Crates and what they may depend on

| Crate | Holds | May use |
| --- | --- | --- |
| `monocode-core` | Data types and pure functions: Session, Block, HarnessEvent, model catalog, settings shapes, the transcript reducer. | serde, serde_json. No IO, no threads, no GPUI. |
| `monocode-store` | SQLite storage: sessions, notes, reminders, automations, checkpoints, orchestration runs, workspace snapshot. | core, rusqlite |
| `monocode-git` | Files, git, worktrees, project search, skills and MCP config files, `gh` calls. | core |
| `monocode-integrations` | GitLab, Azure DevOps, Jira, Linear, link previews, inbox media, rate limits and usage, account identity, harness update checks. | core, ureq |
| `monocode-process` | Agent CLI process supervisor: binary resolution, login shell environment, spawn, write, kill, HTTP and SSE helpers. | core, std threads |
| `monocode-terminal` | PTY host: spawn, write, resize, kill, output coalescing. | vendored portable-pty |
| `monocode-remote` | Host protocol client and server, SSH bootstrap, TLS pinning. | core, rustls |
| `monocode-platform` | Native OS code: notifications, pasteboard, dock badge, window blur, global hotkey, tray, Windows job objects. | objc2, windows-sys |
| `monocode-harness` | Provider adapters ported from `src/integrations/harness`: framing, one module per provider, previews, shell intent. | core, process. Runtime-agnostic async only. |
| `monocode-engine` | Everything `App.tsx` and `src/features/*/model` do that is not drawing, as GPUI entities. | gpui (not gpui_platform), all crates above |
| `monocode-ui` | Theme tokens, icons, shared widgets (buttons, popover, modal, menus, toasts). | gpui, gpui-component |
| `monocode-markdown` | Streaming markdown renderer with code highlighting. | gpui |
| `monocode-terminal-view` | Terminal emulator element: alacritty_terminal grid drawn with GPUI, input, selection, scrollback. | gpui, alacritty_terminal |
| `monocode-editor` | Code editor and diff views built on gpui-component's editor: git gutter, find and replace, language detection. | gpui, gpui-component |
| `monocode-app` | The GPUI binary: windows, views per feature, menus, keybindings. `monocode-app host` runs the engine headless. | everything |

`src-tauri` keeps building until cutover. It has its own workspace and lockfile (`src-tauri/Cargo.lock`), because upstream GTK crates pin a `toml_datetime` that GPUI's build cannot share. It may depend on the non-GPUI crates by path. Check it with `cd src-tauri && cargo check`. Both workspaces build into the root `target/` directory (see `.cargo/config.toml`).

## Async and threads

- `monocode-core` is synchronous.
- `monocode-process`, `monocode-store`, `monocode-git`, and `monocode-integrations` are blocking APIs. Callers run them off the UI thread. Long-lived readers use std threads and send over `async-channel` or a callback.
- `monocode-harness` uses `futures`, `async-channel`, and `smol` timers (`smol::Timer`). It never names tokio or GPUI, so the same adapter runs under the GPUI executor, under the headless host, and in tests with `smol::block_on`. An adapter that needs to spawn takes a spawner from its caller.
- `monocode-engine` holds state in GPUI entities (`Entity<T>`, `cx.notify()`, `cx.spawn`, `cx.background_spawn`). Views observe these entities directly. The headless host runs the same entities under `gpui_platform::headless()`.

## Porting rules

- Port behavior one to one from the TypeScript source. Keep the same names in snake_case, the same branch order, and the same edge cases. Do not redesign while porting. When the TypeScript has a bug, port it and leave a `// TODO(port):` note.
- Start each ported module with a doc comment naming its source, for example `//! Port of src/integrations/harness/core/apply.ts.`
- Port the matching `*.test.ts` cases into Rust tests in the same crate. Keep the test names recognizable. Tests that spawn real CLIs (`*Live.test.ts`) become `#[ignore]` tests.
- JSON shapes must match what TypeScript wrote, because the new app reads the same `monocode.db` and the same settings.
  - Use `#[serde(rename_all = "camelCase")]` and `#[serde(skip_serializing_if = "Option::is_none")]` on optional fields.
  - Persisted structs carry `#[serde(flatten)] pub extra: serde_json::Map<String, serde_json::Value>` so fields we did not model survive a round trip.
  - Integer-valued JS numbers (timestamps, counts, sizes, line numbers, request ids) are `i64`. Use `f64` only for values that can be fractional.
  - String unions become enums with `#[serde(rename = "...")]` per variant. Discriminated unions use `#[serde(tag = "type")]`.
- No Tauri types outside `src-tauri`. Where Tauri code used `AppHandle` for paths, take a `Paths` value or a `&Path`. Where it emitted events, take a callback or a channel.
- Match the current design: colors, sizes, radii, and timings come from `src/styles/index.css` and `src/features/settings/model/appearance.ts` through `monocode-ui`'s theme. Do not hard-code colors in views.
- Never copy code from Zed's GPL crates (`editor`, `terminal_view`, `markdown`, `ui`, `theme`, `workspace`, and others marked GPL in their Cargo.toml). Apache-2.0 crates (gpui and its platform crates) and MIT projects (zeronsh/comet at `~/src/zeron-droid-harness`, termy) are fine to learn from.

## Screenshots

Agents cannot capture the screen. `monocode-app` built with `--features screenshot` takes `--screenshot <path.png>` and writes what the window draws through GPUI's `Window::render_to_image`, then exits. Open the PNG with the Read tool to check a view.

## Working in the shared checkout

Several agents work in this checkout at once.

- Edit only the files your task names. Do not edit the root `Cargo.toml`. If your crate needs a dependency that is not in `[workspace.dependencies]`, add it with an explicit version in your crate's own `Cargo.toml`.
- Build and test only your crate: `cargo check -p <crate> -j 4`, `cargo test -p <crate> -j 4`. Do not run workspace-wide `cargo check`, `cargo fmt`, or `cargo clippy`; run `cargo fmt -p <crate>` and `cargo clippy -p <crate> -j 4`.
- Crates that do not depend on GPUI build fast in their own target directory: set `CARGO_TARGET_DIR=target/agent-<crate>` so you are not blocked by another agent's GPUI build lock.
- Run long commands in the background or with a long timeout. A cold GPUI build takes about 4 minutes.
- Do not commit. The lead reviews and commits.
- If another agent's half-finished code breaks your build through a shared dependency, do not fix their files. Note it in your report and work around it, for example by checking only your crate.

## Writing

Comments, docs, and reports follow the writing rules in `AGENTS.md`: plain words, active voice, no em dashes, sentence case headings.
