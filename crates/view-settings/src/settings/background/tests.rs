//! Ports of newThreadBackgroundEffects.test.ts and
//! useProjectBackgroundEffect.test.ts.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{AppContext as _, TestAppContext};
use image::{DynamicImage, RgbaImage};
use monocode_core::appearance::NewThreadBackgroundEffect;

use super::*;

/// A reader that counts its calls and fails or returns a small PNG.
fn counting_reader(fail: Option<&'static str>) -> (Arc<AtomicUsize>, SourceReader) {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let reader: SourceReader = Arc::new(move |_path| {
        seen.fetch_add(1, Ordering::SeqCst);
        if let Some(message) = fail {
            return Err(message.to_string());
        }
        Ok(png_bytes())
    });
    (calls, reader)
}

pub(crate) fn png_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    DynamicImage::ImageRgba8(RgbaImage::from_fn(8, 8, |x, y| {
        image::Rgba([(x * 30) as u8, (y * 30) as u8, 120, 255])
    }))
    .write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .unwrap();
    bytes
}

#[gpui::test]
fn uses_the_original_asset_directly_for_none_so_animation_is_preserved(cx: &mut TestAppContext) {
    let (calls, reader) = counting_reader(Some("None must not read the file"));
    cx.update(|cx| {
        BackgroundEffects::set_reader(reader, cx);
        ChatBackground::apply_prepared(
            "/background.gif?v=101",
            "/background.gif",
            NewThreadBackgroundEffect::None,
            false,
            cx,
        );
    });
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    cx.read(|cx| {
        let state = ChatBackground::global(cx).unwrap();
        assert_eq!(
            state.image,
            Some(BackgroundImage::Original("/background.gif".into()))
        );
        assert!(state.effect_ready);
    });
}

#[gpui::test]
fn uses_the_original_asset_for_gradient_blur_without_starting_the_image_worker(
    cx: &mut TestAppContext,
) {
    let (calls, reader) = counting_reader(None);
    cx.update(|cx| {
        BackgroundEffects::set_reader(reader, cx);
        ChatBackground::apply_prepared(
            "/background.png?v=102",
            "/background.png",
            NewThreadBackgroundEffect::GradientBlur,
            false,
            cx,
        );
    });
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    cx.read(|cx| {
        let state = ChatBackground::global(cx).unwrap();
        assert!(state.gradient_blur);
        assert!(state.effect_ready);
        assert_eq!(
            state.image,
            Some(BackgroundImage::Original("/background.png".into()))
        );
    });
    cx.update(|cx| {
        ChatBackground::apply_prepared(
            "/background.png?v=102",
            "/background.png",
            NewThreadBackgroundEffect::None,
            false,
            cx,
        )
    });
    cx.run_until_parked();
    cx.read(|cx| assert!(!ChatBackground::global(cx).unwrap().gradient_blur));
}

#[gpui::test]
fn drops_rejected_source_promises_so_transient_failures_can_retry(cx: &mut TestAppContext) {
    let (calls, reader) = counting_reader(Some("temporary failure"));
    cx.update(|cx| BackgroundEffects::set_reader(reader, cx));
    for _ in 0..2 {
        let prepared = cx.update(|cx| {
            BackgroundEffects::prepare(
                "/background.png?v=5",
                "/background.png",
                NewThreadBackgroundEffect::Dither,
                false,
                cx,
            )
        });
        let result = cx.foreground_executor().block_test(prepared);
        assert_eq!(result.err().as_deref(), Some("temporary failure"));
        cx.run_until_parked();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    cx.read(|cx| assert_eq!(BackgroundEffects::cached(cx), (0, 0)));
}

#[gpui::test]
fn renders_an_effect_once_and_shares_it(cx: &mut TestAppContext) {
    let (calls, reader) = counting_reader(None);
    cx.update(|cx| BackgroundEffects::set_reader(reader, cx));
    let first = cx.update(|cx| {
        BackgroundEffects::prepare(
            "/a.png?v=1",
            "/a.png",
            NewThreadBackgroundEffect::Halftone,
            true,
            cx,
        )
    });
    let second = cx.update(|cx| {
        BackgroundEffects::prepare(
            "/a.png?v=1",
            "/a.png",
            NewThreadBackgroundEffect::Halftone,
            true,
            cx,
        )
    });
    let a = cx.foreground_executor().block_test(first).unwrap();
    let b = cx.foreground_executor().block_test(second).unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // A dither of the same source reuses the decoded image.
    let dither = cx.update(|cx| {
        BackgroundEffects::prepare(
            "/a.png?v=1",
            "/a.png",
            NewThreadBackgroundEffect::Dither,
            false,
            cx,
        )
    });
    cx.foreground_executor().block_test(dither).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[gpui::test]
fn applies_a_prepared_effect_and_falls_back_to_the_original_on_failure(cx: &mut TestAppContext) {
    let (_, reader) = counting_reader(None);
    cx.update(|cx| {
        BackgroundEffects::set_reader(reader, cx);
        ChatBackground::apply_prepared(
            "/a.png?v=1",
            "/a.png",
            NewThreadBackgroundEffect::Scanlines,
            false,
            cx,
        );
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let state = ChatBackground::global(cx).unwrap();
        assert!(matches!(state.image, Some(BackgroundImage::Prepared(_))));
        assert!(state.effect_ready);
    });

    let (_, failing) = counting_reader(Some("gone"));
    cx.update(|cx| {
        BackgroundEffects::set_reader(failing, cx);
        ChatBackground::apply_prepared(
            "/b.png?v=2",
            "/b.png",
            NewThreadBackgroundEffect::Ascii,
            false,
            cx,
        );
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let state = ChatBackground::global(cx).unwrap();
        assert_eq!(
            state.image,
            Some(BackgroundImage::Original("/b.png".into()))
        );
        assert!(state.effect_ready);
    });
}

// useProjectBackgroundEffect.test.ts

#[gpui::test]
fn uses_the_original_for_none_and_a_pane_local_processed_image_for_dither(cx: &mut TestAppContext) {
    let (calls, reader) = counting_reader(None);
    cx.update(|cx| BackgroundEffects::set_reader(reader, cx));
    let effect = cx.new(|_| ProjectBackgroundEffect::new());
    let resolve = |effect_kind, cx: &mut TestAppContext| {
        effect.update(cx, |this, cx| {
            this.resolve(Some("/background.png"), effect_kind, 101, false, cx)
        })
    };
    assert_eq!(
        resolve(NewThreadBackgroundEffect::None, cx),
        Some(BackgroundImage::Original("/background.png".into()))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    assert_eq!(resolve(NewThreadBackgroundEffect::Dither, cx), None);
    cx.run_until_parked();
    let prepared = resolve(NewThreadBackgroundEffect::Dither, cx);
    assert!(matches!(prepared, Some(BackgroundImage::Prepared(_))));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // The global background is untouched.
    cx.read(|cx| assert!(ChatBackground::global(cx).is_none()));

    assert_eq!(
        resolve(NewThreadBackgroundEffect::None, cx),
        Some(BackgroundImage::Original("/background.png".into()))
    );
}
