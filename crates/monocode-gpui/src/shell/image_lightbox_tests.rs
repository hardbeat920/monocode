use super::*;
use base64::Engine;
use gpui::{TestAppContext, VisualTestContext, point};

#[gpui::test]
fn image_lightbox_opens_from_composer_and_sent_images_and_restores_focus(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::input::bind(cx);
        crate::image_lightbox::bind(cx);
        cx.set_reduce_motion(true);
    });
    let (window, _, active) = super::tests::window(cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    let path = std::env::temp_dir().join(format!(
        "monocode-lightbox-{}.png",
        monocode_core::mono::new_id()
    ));
    std::fs::write(&path, base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap()).unwrap();
    let input = window
        .update(visual, |shell, window, cx| {
            let input = shell.sessions[&active].read(cx).input.clone();
            shell.sessions[&active].update(cx, |view, cx| {
                view.input
                    .update(cx, |input, cx| input.set_text("Keep this draft".into(), cx));
                cx.notify();
            });
            window.focus(&input.focus_handle(cx), cx);
            input
        })
        .unwrap();
    visual.update(|_, cx| {
        cx.write_to_clipboard(ClipboardItem {
            entries: vec![gpui::ClipboardEntry::ExternalPaths(gpui::ExternalPaths(
                vec![path.clone()].into(),
            ))],
        })
    });
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    visual.simulate_keystrokes(&format!("{modifier}-v"));
    visual.run_until_parked();
    visual.refresh().unwrap();
    let thumbnail = visual.debug_bounds("composer-image-0").unwrap();
    visual.simulate_click(thumbnail.center(), Default::default());
    assert_eq!(
        visual.debug_bounds("image-lightbox").unwrap().size,
        window.update(visual, |_, w, _| w.viewport_size()).unwrap()
    );
    visual.simulate_keystrokes("escape");
    assert!(visual.debug_bounds("image-lightbox").is_none());
    window
        .update(visual, |shell, w, cx| {
            assert!(input.focus_handle(cx).is_focused(w));
            assert_eq!(input.read(cx).content.as_ref(), "Keep this draft");
            assert!(shell.image_lightbox.is_none());
        })
        .unwrap();

    // The remove button overlaps the thumbnail but must not open its preview.
    let thumbnail = visual.debug_bounds("composer-image-0").unwrap();
    visual.simulate_click(
        point(thumbnail.right() - px(4.), thumbnail.top() + px(4.)),
        Default::default(),
    );
    assert!(visual.debug_bounds("composer-image-0").is_none());
    assert!(visual.debug_bounds("image-lightbox").is_none());

    let data = base64::engine::general_purpose::STANDARD.encode(br##"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="480"><rect width="640" height="480" fill="#4da3f5"/></svg>"##);
    window.update(visual, |shell, _, cx| {
        shell.sessions[&active].update(cx, |view, cx| {
            view.summary.has_messages = true;
            cx.notify();
        });
        let transcript = shell.sessions[&active].read(cx).transcript.clone();
        transcript.update(cx, |view, cx| view.patch(0, vec![
            monocode_core::transcript::Row { role: "user".into(), text: "Look at this".into(), first: true, sent_at: None },
            monocode_core::transcript::Row { role: monocode_core::transcript::attachments::ROLE.into(), text: serde_json::json!([{"kind":"image", "name":"Persisted image", "mimeType":"image/svg+xml", "data":data}]).to_string(), first: true, sent_at: None },
        ], cx));
    }).unwrap();
    visual.refresh().unwrap();
    let sent = visual.debug_bounds("transcript-image-0").unwrap();
    visual.simulate_click(sent.center(), Default::default());
    assert!(visual.debug_bounds("image-lightbox").is_some());
    assert!(visual.debug_bounds("lightbox-image").is_some());
    // A press on the backdrop closes immediately, just like onMouseDown in React.
    visual.simulate_mouse_down(point(px(4.), px(4.)), MouseButton::Left, Default::default());
    visual.simulate_mouse_up(point(px(4.), px(4.)), MouseButton::Left, Default::default());
    assert!(visual.debug_bounds("image-lightbox").is_none());
    let sent = visual.debug_bounds("transcript-image-0").unwrap();
    visual.simulate_click(sent.center(), Default::default());
    let close = visual.debug_bounds("close-image-preview").unwrap();
    visual.simulate_click(close.center(), Default::default());
    assert!(visual.debug_bounds("image-lightbox").is_none());
    std::fs::remove_file(path).unwrap();
}
