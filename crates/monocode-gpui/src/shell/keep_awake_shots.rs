//! Renders the footer's keep-awake pill and popover in a real shell with the
//! Metal renderer and writes PNGs of the footer's corner.
//! `KEEP_AWAKE_SHOTS=/tmp/shots cargo test … keep_awake_shots -- --ignored`
use super::*;
use gpui::{HeadlessAppContext, size};
use monocode_core::store::Store;

/// This checkout, so the shell has a real project to open.
const PROJECT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

#[test]
#[ignore = "Requires a Metal device; writes screenshots"]
fn keep_awake_shots() {
    let dir = std::env::var("KEEP_AWAKE_SHOTS").unwrap_or_else(|_| "/tmp/keep-awake-shots".into());
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let mut session = store.create("claude", PROJECT).unwrap();
    store
        .title(session.id, "Refactor provider lifecycle")
        .unwrap();
    session.title = "Refactor provider lifecycle".into();
    session.has_messages = true;
    let id = session.id;
    let mut state = Workspace::default();
    state.add_project(PROJECT);
    state.sidebar_visible = true;
    state.right_panel_open = false;
    // No live harness-update check: its notice would cover the shots.
    state.preferences.hidden_providers = monocode_core::provider::Kind::ALL
        .iter()
        .map(|kind| kind.id().to_string())
        .collect();
    let saved = serde_json::to_string(&state).unwrap();
    let store = Arc::new(Mutex::new(store));
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(gpui_apple::AppleTextSystem::new()),
        Arc::new(crate::assets::Assets),
        || {
            Ok(Some(Box::new(
                gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
            )))
        },
    );
    cx.update(crate::popover::bind);
    let (width, height) = (1100u32, 820u32);
    let window = cx
        .open_window(size(px(width as f32), px(height as f32)), |_, cx| {
            cx.new(|cx| {
                let mut shell = Shell::new(store, vec![session], PROJECT.into(), Some(saved), cx);
                shell.ensure_session(id, cx);
                shell.workspace.active = Some(id);
                shell
            })
        })
        .unwrap();
    let frame = |cx: &mut HeadlessAppContext| {
        for _ in 0..4 {
            std::thread::sleep(std::time::Duration::from_millis(120));
            cx.run_until_parked();
            cx.update_window(window.into(), |_, w, _| w.refresh())
                .unwrap();
            cx.run_until_parked();
        }
    };
    let stage = |cx: &mut HeadlessAppContext, step: &str| {
        window
            .update(cx, |shell, window, cx| {
                shell
                    .keep_awake
                    .update(cx, |keep, cx| keep.stage(step, window, cx));
                cx.notify();
            })
            .unwrap();
    };
    let shot = |cx: &mut HeadlessAppContext, name: &str, crop_height: u32| {
        frame(cx);
        let image = cx.capture_screenshot(window.into()).unwrap();
        let mut flat = image::RgbaImage::from_pixel(
            image.width(),
            image.height(),
            image::Rgba([20, 20, 20, 255]),
        );
        image::imageops::overlay(&mut flat, &image, 0, 0);
        let scale = image.width() / width;
        let (w, h) = (600 * scale, crop_height * scale);
        image::imageops::crop_imm(&flat, flat.width() - w, flat.height() - h, w, h)
            .to_image()
            .save(format!("{dir}/{name}.png"))
            .unwrap();
    };
    shot(&mut cx, "0-off", 80);
    stage(&mut cx, "agents");
    shot(&mut cx, "1-agents", 80);
    stage(&mut cx, "open");
    shot(&mut cx, "2-working", 620);
    stage(&mut cx, "timer");
    shot(&mut cx, "3-timer", 660);
    stage(&mut cx, "lid");
    shot(&mut cx, "4-lid", 760);
    stage(&mut cx, "consent");
    shot(&mut cx, "5-consent", 760);
    stage(&mut cx, "hotspot");
    shot(&mut cx, "6-hotspot", 760);
    stage(&mut cx, "hot");
    shot(&mut cx, "7-hot", 760);
    stage(&mut cx, "close");
    for (name, row) in [
        ("8-settings-lid", "keep-awake-lid-battery"),
        ("9-settings-hotspot", "keep-awake-hotspot-network"),
    ] {
        window
            .update(&mut cx, |shell, window, cx| {
                shell.open_settings(crate::settings::Section::General, window, cx);
                if let Some(settings) = &shell.settings {
                    settings.update(cx, |settings, cx| settings.reveal(row, cx));
                }
            })
            .unwrap();
        frame(&mut cx);
        // Rows have places only after a layout; reveal again to scroll there.
        window
            .update(&mut cx, |shell, _, cx| {
                if let Some(settings) = &shell.settings {
                    settings.update(cx, |settings, cx| settings.reveal(row, cx));
                }
            })
            .unwrap();
        frame(&mut cx);
        let image = cx.capture_screenshot(window.into()).unwrap();
        let mut flat = image::RgbaImage::from_pixel(
            image.width(),
            image.height(),
            image::Rgba([20, 20, 20, 255]),
        );
        image::imageops::overlay(&mut flat, &image, 0, 0);
        flat.save(format!("{dir}/{name}.png")).unwrap();
    }
}
