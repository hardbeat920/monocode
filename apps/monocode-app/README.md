# monocode-app

The native GPUI app. For now it opens the app shell with mock data, plus gallery views for checking `monocode-ui` by screenshot.

## Run

```sh
cargo run -p monocode-app -j 4
cargo run -p monocode-app -j 4 -- --view widgets --theme light
```

## Screenshots

Agents cannot capture the screen, so the app can capture itself. Build with the `screenshot` feature, which turns on GPUI's `test-support` for `Window::render_to_image`:

```sh
cargo build -p monocode-app --features screenshot -j 4
./target/debug/monocode-app --screenshot /tmp/shell.png
./target/debug/monocode-app --view widgets --size 1280x800 --screenshot /tmp/widgets.png
```

The app opens its window, redraws for 900ms so SVGs and images finish loading, writes the frame as a PNG, and exits with code 0. Open the PNG with the Read tool. A failed capture prints `screenshot failed: ...` and exits with code 1.

The PNG is at the display's pixel density, so a 1280x800 window on a Retina screen gives a 2560x1600 image. One CSS px from the React source is 2 image px.

| Flag | Meaning |
| --- | --- |
| `--screenshot <path>` | Write the settled frame to `path` and exit. Needs `--features screenshot`. |
| `--size WxH` | Window content size in points. Default `1280x800`. The window manager may shrink it to fit the screen. |
| `--view <name>` | Which view fills the window. Default `shell`. `--list-views` prints the names. |
| `--theme dark\|light\|system` | Overrides the color scheme preference. Default `dark`. |
| `--ui-scale <factor>` | Interface scale, 0.5 to 2, like the Appearance setting. |
| `--backdrop <#rrggbb\|none>` | The color the transparent window is composited over in the PNG, standing in for the blurred desktop. Default `#5f5560`. `none` keeps the alpha channel. |

Two things in a capture differ from the window on screen:

- AppKit draws the traffic lights outside GPUI's scene. The capture paints stand-ins at the same position when it composites over a backdrop.
- The real desktop blur is not in the capture. The window glass shows as the backdrop color.

The capture moves GPUI's pointer outside the window before drawing, so no hover state shows wherever the real cursor sits.

## Views

| Name | Shows |
| --- | --- |
| `shell` | The shell: project rail, session sidebar, title bar with workspace tabs, a mock pane, and the usage footer. |
| `shell-compact` | The shell with the 48px compact rail. On macOS the title bar moves above everything. |
| `shell-no-rail` | The shell with the project rail closed. The sidebar takes the traffic lights and the project picker. |
| `shell-menu` | The shell with the session context menu open. |
| `widgets` | Every `monocode-ui` widget and a toast. |
| `modal` | A modal over the shell. |
| `icons` | All chrome icons, provider logos, and a sample of file-type icons. |
| `blank` | An empty themed window. |

To check a new view, add a `ViewEntry` to `VIEWS` in `src/views.rs` with a `build` function that returns an `AnyView`, then pass its name to `--view`.

## Layout

- `src/main.rs` opens the window: transparent and blurred in dark mode on macOS, hidden title bar, traffic lights at (12, 13) so they sit centered in the 40px chrome.
- `src/shell/` is the shell, one module per region: `title_bar.rs`, `project_rail.rs` (also the compact rail), `sidebar.rs`, `main_pane.rs`, `footer.rs`. `mod.rs` holds the layout, resize handles, and window drag regions.
- `src/mock.rs` is the mock data, as plain structs. Replace it region by region with engine entities.
- `src/gallery.rs` holds the `widgets`, `modal`, and `icons` views.
