# monocode-app

The native GPUI app. It boots the engine on the app data directory (the same `monocode.db` and settings the Tauri app uses), restores the saved workspace, and shows real sessions. Gallery views check `monocode-ui` by screenshot.

## Run

Never point a development run at the real data directory while the Tauri app may be using it. Copy it first:

```sh
mkdir -p /tmp/mc/appdata
sqlite3 ~/Library/Application\ Support/com.monocode.desktop/monocode.db ".backup /tmp/mc/appdata/monocode.db"
MONOCODE_DATA_DIR=/tmp/mc/appdata cargo run -p monocode-app -j 4
cargo run -p monocode-app -j 4 -- --view widgets --theme light
```

The data directory is `--data-dir`, else `MONOCODE_DATA_DIR`, else the Tauri app's (`~/Library/Application Support/com.monocode.desktop` on macOS). The first start copies the Tauri app's WebKit localStorage into `local-storage.json` there; the import only reads the WebKit files.

`monocode-app app ...` and `monocode-app control ...` run the agent CLI (`monocode_process::control_cli`), as the Tauri binary does.

## Live test

`tests/live_engine.rs` runs Claude Code for real: a supervised turn that needs an approval, then a second process that reloads the session from the store and resumes it. It has its own `main`, because GPUI's run loop needs the main thread, and runs only with `--ignored`:

```sh
MONOCODE_DATA_DIR=/tmp/mc/appdata cargo test -p monocode-app --test live_engine -j 4 -- --ignored
```

## Screenshots

Agents cannot capture the screen, so the app can capture itself. Build with the `screenshot` feature, which turns on GPUI's `test-support` for `Window::render_to_image`:

```sh
cargo build -p monocode-app --features screenshot -j 4
./target/debug/monocode-app --screenshot /tmp/shell.png
./target/debug/monocode-app --view widgets --size 1280x800 --screenshot /tmp/widgets.png
```

The app opens its window, redraws for 900ms (2500ms for views that boot the engine) so SVGs, images, and the store finish loading, writes the frame as a PNG, and exits with code 0. Open the PNG with the Read tool. A failed capture prints `screenshot failed: ...` and exits with code 1.

The PNG is at the display's pixel density, so a 1280x800 window on a Retina screen gives a 2560x1600 image. One CSS px from the React source is 2 image px.

| Flag | Meaning |
| --- | --- |
| `--screenshot <path>` | Write the settled frame to `path` and exit. Needs `--features screenshot`. |
| `--size WxH` | Window content size in points. Default `1280x800`. The window manager may shrink it to fit the screen. |
| `--view <name>` | Which view fills the window. Default `shell`. `--list-views` prints the names. |
| `--theme dark\|light\|system` | Overrides the color scheme preference. Default `dark`. |
| `--ui-scale <factor>` | Interface scale, 0.5 to 2, like the Appearance setting. |
| `--backdrop <#rrggbb\|none>` | The color the transparent window is composited over in the PNG, standing in for the blurred desktop. Default `#5f5560`. `none` keeps the alpha channel. |
| `--data-dir <dir>` | The app data directory. |
| `--open-session <id>` | Opens this stored session once the workspace restores. |
| `--settle-ms <ms>` | How long to redraw before the capture. |

Two things in a capture differ from the window on screen:

- AppKit draws the traffic lights outside GPUI's scene. The capture paints stand-ins at the same position when it composites over a backdrop.
- The real desktop blur is not in the capture. The window glass shows as the backdrop color.

The capture moves GPUI's pointer outside the window before drawing, so no hover state shows wherever the real cursor sits.

## Views

| Name | Shows |
| --- | --- |
| `shell` | The app: project rail, session sidebar, title bar with the workspace tabs, the active tab's panes, and the usage footer. |
| `shell-compact` | The shell with the 48px compact rail. On macOS the title bar moves above everything. |
| `shell-no-rail` | The shell with the project rail closed. The sidebar takes the traffic lights and the project picker. |
| `shell-menu` | The shell with the session context menu open. |
| `widgets` | Every `monocode-ui` widget and a toast. |
| `modal` | A modal over the shell. |
| `icons` | All chrome icons, provider logos, and a sample of file-type icons. |
| `blank` | An empty themed window. |

To check a new view, add a `ViewEntry` to `VIEWS` in `src/views.rs` with a `build` function that returns an `AnyView`, then pass its name to `--view`.

## Layout

The library (`src/lib.rs`) is the engine side, shared by the window and the live test:

- `data_dir.rs` resolves the data directory. `boot.rs` opens the settings (`Kv`) and runs the WebKit import, opens `monocode.db`, starts the harness bridge and registers every provider, then initializes attention, submit, side threads, and the workspace, and restores the saved workspace.
- `bridge.rs` is the runtime's `HarnessHooks` and attention's `ApprovalRouter` over the harness registry. `provider_hooks.rs` gives the providers git context, generated images, and Cursor's stores. `session_factory.rs` builds new sessions from the live catalog.
- `projects.rs` (the rail list) and `history.rs` (sidebar rows) are small stand-ins for the engine's projects and history packages. `attention_platform.rs` keeps notification calls out of unbundled builds.

The binary draws:

- `src/main.rs` opens the window: transparent in dark mode with the user's blur radius on macOS (`glass.rs`), hidden title bar, traffic lights at (12, 13) so they sit centered in the 40px chrome.
- `src/shell/` is the shell, one module per region: `title_bar.rs`, `project_rail.rs` (also the compact rail), `sidebar.rs`, `main_pane.rs` (the split tree), `footer.rs`. `mod.rs` holds the layout, the window's `Workspace`, resize handles, and window drag regions. `view_data.rs` collects what the regions draw from the engine.
- `session_pane.rs` is a session's transcript and composer; `composer_host.rs` connects the composer to `Submit`. `file_pane.rs` shows editor surfaces with `monocode-editor`.
- `src/gallery.rs` holds the `widgets`, `modal`, and `icons` views.
