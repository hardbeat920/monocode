//! Real Metal regression for the translucent inset shell. No window is opened.
use crate::theme::{content, pane_fill, shell_fill};
use gpui::{Bounds, ContentMask, Corners, DevicePixels, Quad, ScaledPixels, Scene, point, size};
// `backdrop_marker_blurs_only_inside_its_rounded_bounds` also needs Edges, Hsla.
use gpui_apple::metal_renderer::{InstanceBufferPool, MetalRenderer};
use std::sync::Arc;

#[test]
#[ignore = "Requires a Metal device; run explicitly on macOS"]
fn rounded_children_restore_each_translucent_corner_without_touching_the_footer() {
    use gpui::{Hsla, black, white};
    use gpui_apple::metal_renderer::corner_clip_marker;
    let mut renderer = MetalRenderer::new_headless(Arc::new(parking_lot::Mutex::new(
        InstanceBufferPool::default(),
    )));
    renderer.update_transparency(true);
    let full = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(64.), ScaledPixels(48.)),
    );
    let pane = Bounds::new(
        point(ScaledPixels(8.), ScaledPixels(8.)),
        size(ScaledPixels(48.), ScaledPixels(32.)),
    );
    let corners = [(8., 8.), (48., 8.), (48., 32.), (8., 32.)];
    let quad = |bounds, color: Hsla, marker: Hsla| Quad {
        order: 0,
        border_style: Default::default(),
        bounds,
        content_mask: ContentMask { bounds: full },
        background: color.into(),
        border_color: marker,
        corner_radii: Default::default(),
        border_widths: Default::default(),
    };
    let render = |renderer: &mut MetalRenderer, children: bool| {
        let mut scene = Scene::default();
        for x in 0..64u32 {
            scene.insert_primitive(quad(
                Bounds::new(
                    point(ScaledPixels(x as f32), ScaledPixels(0.)),
                    size(ScaledPixels(1.), ScaledPixels(48.)),
                ),
                gpui::rgba(((x * 3) << 24) | 0x00305080).into(),
                Default::default(),
            ));
        }
        if children {
            let outside = Bounds::new(
                point(ScaledPixels(-12.), ScaledPixels(-12.)),
                size(ScaledPixels(8.), ScaledPixels(8.)),
            );
            // Fully clipped corners do not occupy an unallocated scratch slot.
            scene.insert_primitive(quad(outside, black(), corner_clip_marker(true, 0)));
            let marker = |begin, corner: usize| {
                let (x, y) = corners[corner];
                quad(
                    Bounds::new(
                        point(ScaledPixels(x), ScaledPixels(y)),
                        size(ScaledPixels(8.), ScaledPixels(8.)),
                    ),
                    black(),
                    corner_clip_marker(begin, corner as u8),
                )
            };
            for corner in 0..4 {
                scene.insert_primitive(marker(true, corner));
            }
            scene.insert_primitive(quad(pane, white(), Default::default()));
            for corner in 0..4 {
                scene.insert_primitive(marker(false, corner));
            }
            scene.insert_primitive(quad(outside, black(), corner_clip_marker(false, 0)));
        }
        scene.finish();
        renderer
            .render_scene_to_image(&scene, size(DevicePixels(64), DevicePixels(48)))
            .unwrap()
    };
    let background = render(&mut renderer, false);
    let clipped = render(&mut renderer, true);
    for (x, y) in [(8, 8), (55, 8), (55, 39), (8, 39), (32, 44)] {
        assert_eq!(clipped.get_pixel(x, y), background.get_pixel(x, y));
    }
    for (x, y) in [(15, 15), (48, 15), (48, 32), (15, 32), (32, 24)] {
        assert_eq!(clipped.get_pixel(x, y)[3], 255);
    }
}

#[test]
#[ignore = "Requires a Metal device; run explicitly on macOS"]
fn title_particle_reveal_matches_opposed_css_masks_over_a_translucent_backdrop() {
    use gpui::{Hsla, black, white};
    use gpui_apple::metal_renderer::text_reveal_marker;
    let mut renderer = MetalRenderer::new_headless(Arc::new(parking_lot::Mutex::new(
        InstanceBufferPool::default(),
    )));
    renderer.update_transparency(true);
    let full = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(120.), ScaledPixels(40.)),
    );
    let quad = |bounds, color: Hsla, marker: Hsla| Quad {
        order: 0,
        border_style: Default::default(),
        bounds,
        content_mask: ContentMask { bounds: full },
        background: color.into(),
        border_color: marker,
        corner_radii: Default::default(),
        border_widths: Default::default(),
    };
    for (start, end) in [(-36., -18.), (32., 50.), (59., 41.), (100., 118.)] {
        let label = Bounds::new(
            point(ScaledPixels(10.), ScaledPixels(5.)),
            size(ScaledPixels(100.), ScaledPixels(30.)),
        );
        let mut scene = Scene::default();
        scene.insert_primitive(quad(
            full,
            gpui::rgba(0x20408080).into(),
            Default::default(),
        ));
        scene.insert_primitive(quad(label, black(), text_reveal_marker(true, start, end)));
        scene.insert_primitive(quad(label, white(), Default::default()));
        scene.insert_primitive(quad(label, black(), text_reveal_marker(false, start, end)));
        scene.finish();
        let image = renderer
            .render_scene_to_image(&scene, size(DevicePixels(120), DevicePixels(40)))
            .unwrap();
        assert_eq!(
            image.get_pixel(4, 20)[3],
            128,
            "mask leaves the backdrop outside the label alone"
        );
        for x in 10..110 {
            let alpha = ((end - (x as f32 + 0.5 - 10.)) / (end - start)).clamp(0., 1.);
            let expected = 128. + 127. * alpha;
            assert!(
                (f32::from(image.get_pixel(x, 20)[3]) - expected).abs() < 2.,
                "CSS mask at {x}, stops {start}..{end}"
            );
        }
        assert_eq!(image.get_pixel(50, 0)[3], 128);
    }
}

#[test]
#[ignore = "Requires a Metal device; run explicitly on macOS"]
fn background_artwork_mask_rounds_fades_and_blurs_without_touching_the_backdrop() {
    use gpui::{Edges, Hsla, black, white};
    use gpui_apple::metal_renderer::background_mask_marker;
    let mut renderer = MetalRenderer::new_headless(Arc::new(parking_lot::Mutex::new(
        InstanceBufferPool::default(),
    )));
    renderer.update_transparency(true);
    let full = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(128.), ScaledPixels(200.)),
    );
    let layer = Bounds::new(
        point(ScaledPixels(8.), ScaledPixels(8.)),
        size(ScaledPixels(112.), ScaledPixels(184.)),
    );
    let quad = |bounds, color: Hsla| Quad {
        order: 0,
        border_style: Default::default(),
        bounds,
        content_mask: ContentMask { bounds: full },
        background: color.into(),
        border_color: Default::default(),
        corner_radii: Default::default(),
        border_widths: Default::default(),
    };
    let render = |renderer: &mut MetalRenderer, haze, empty, alpha| {
        let mut scene = Scene::default();
        scene.insert_primitive(quad(full, gpui::rgba(0x20408080).into()));
        let marker = |begin| {
            let mut q = quad(layer, black());
            q.border_color = background_mask_marker(begin, empty, haze, false, 0.65);
            q.border_widths = Edges {
                left: ScaledPixels(1.),
                ..Default::default()
            };
            q.corner_radii = Corners::all(ScaledPixels(12.));
            q
        };
        scene.insert_primitive(marker(true));
        for x in 8..120 {
            let stripe = Bounds::new(
                point(ScaledPixels(x as f32), ScaledPixels(8.)),
                size(ScaledPixels(1.), ScaledPixels(184.)),
            );
            scene.insert_primitive(quad(
                stripe,
                if x % 2 == 0 {
                    black().opacity(alpha)
                } else {
                    white().opacity(alpha)
                },
            ));
        }
        scene.insert_primitive(marker(false));
        scene.finish();
        renderer
            .render_scene_to_image(&scene, size(DevicePixels(128), DevicePixels(200)))
            .unwrap()
    };
    let plain = render(&mut renderer, false, true, 1.);
    let base = plain.get_pixel(0, 0);
    assert_eq!(
        plain.get_pixel(8, 8),
        base,
        "the rounded top corner reveals the original backdrop"
    );
    assert_eq!(
        plain.get_pixel(119, 8),
        base,
        "both top corners are clipped"
    );
    assert!(
        (208..=213).contains(&plain.get_pixel(35, 30)[3]),
        "65% opacity over the translucent native backdrop"
    );
    for percent in [30., 42., 54., 64., 72., 90.] {
        let y = (8. + 184. * percent / 100.) as u32;
        let fraction = (y as f32 + 0.5 - 8.) / 184.;
        let fade = monocode_core::project_appearance::background_fade(fraction, true, false);
        let expected = 128. + 127. * 0.65 * fade;
        assert!(
            (plain.get_pixel(35, y)[3] as f32 - expected).abs() < 3.,
            "React fade at {percent}%"
        );
    }
    let translucent = render(&mut renderer, false, true, 0.5);
    assert!(
        (167..=173).contains(&translucent.get_pixel(35, 30)[3]),
        "the artwork's alpha survives isolation and masking"
    );
    let haze = render(&mut renderer, true, true, 1.);
    assert_eq!(haze.get_pixel(8, 8), base);
    assert_eq!(
        haze.get_pixel(35, 30),
        plain.get_pixel(35, 30),
        "Haze keeps the top of the artwork sharp"
    );
    let plain_contrast =
        (plain.get_pixel(35, 100)[0] as i32 - plain.get_pixel(34, 100)[0] as i32).abs();
    let haze_contrast =
        (haze.get_pixel(35, 100)[0] as i32 - haze.get_pixel(34, 100)[0] as i32).abs();
    assert!(
        haze_contrast < plain_contrast / 2,
        "Haze blends real Gaussian layers below the sharp top"
    );
    assert_eq!(
        haze.get_pixel(35, 190),
        base,
        "the fade preserves the actual native backdrop"
    );
    let session = render(&mut renderer, false, false, 1.);
    assert!(session.get_pixel(35, 70)[3] < plain.get_pixel(35, 70)[3]);
}

#[test]
#[ignore = "Requires a Metal device; run explicitly on macOS"]
fn tab_title_fade_preserves_the_background_and_only_fades_the_right_edge() {
    use gpui::{Hsla, black, white};
    use gpui_apple::metal_renderer::horizontal_fade_marker;
    let mut renderer = MetalRenderer::new_headless(Arc::new(parking_lot::Mutex::new(
        InstanceBufferPool::default(),
    )));
    renderer.update_transparency(true);
    let full = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(192.), ScaledPixels(32.)),
    );
    let label = Bounds::new(
        point(ScaledPixels(8.), ScaledPixels(4.)),
        size(ScaledPixels(80.), ScaledPixels(24.)),
    );
    let quad = |bounds, color: Hsla, marker: Hsla| Quad {
        order: 0,
        border_style: Default::default(),
        bounds,
        content_mask: ContentMask { bounds: full },
        background: color.into(),
        border_color: marker,
        corner_radii: Default::default(),
        border_widths: Default::default(),
    };
    let mut scene = Scene::default();
    scene.insert_primitive(quad(
        full,
        gpui::rgba(0x20408080).into(),
        Default::default(),
    ));
    // Disjoint labels share draw orders. The renderer must pair their masks
    // even when GPUI batches both begins before both ends.
    for label in [
        label,
        Bounds::new(point(ScaledPixels(104.), ScaledPixels(4.)), label.size),
    ] {
        scene.insert_primitive(quad(label, black(), horizontal_fade_marker(true, 40.)));
        scene.insert_primitive(quad(label, white(), Default::default()));
        scene.insert_primitive(quad(label, black(), horizontal_fade_marker(false, 40.)));
    }
    scene.finish();
    let image = renderer
        .render_scene_to_image(&scene, size(DevicePixels(192), DevicePixels(32)))
        .unwrap();
    assert_eq!(
        image.get_pixel(12, 5)[3],
        255,
        "the top and left edges stay clear"
    );
    assert_eq!(
        image.get_pixel(47, 16)[3],
        255,
        "text before the last 40px stays opaque"
    );
    assert!(
        (188..=194).contains(&image.get_pixel(68, 16)[3]),
        "the ramp is halfway faded at 20px"
    );
    assert!(
        (128..=132).contains(&image.get_pixel(87, 16)[3]),
        "the right edge reveals the translucent background"
    );
    assert_eq!(
        image.get_pixel(90, 16)[3],
        128,
        "the mask stays inside the label"
    );
    assert_eq!(
        image.get_pixel(108, 5)[3],
        255,
        "the next tab keeps its own text"
    );
    assert!(
        (188..=194).contains(&image.get_pixel(164, 16)[3]),
        "the next tab has an independent ramp"
    );
    assert!((128..=132).contains(&image.get_pixel(183, 16)[3]));
}

#[test]
#[ignore = "Requires a Metal device; run explicitly on macOS"]
fn tab_strip_fade_ramps_each_requested_side() {
    use gpui::{Hsla, black, white};
    use gpui_apple::metal_renderer::side_fade_marker;
    let mut renderer = MetalRenderer::new_headless(Arc::new(parking_lot::Mutex::new(
        InstanceBufferPool::default(),
    )));
    renderer.update_transparency(true);
    let full = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(200.), ScaledPixels(16.)),
    );
    let quad = |bounds, color: Hsla, marker: Hsla| Quad {
        order: 0,
        border_style: Default::default(),
        bounds,
        content_mask: ContentMask { bounds: full },
        background: color.into(),
        border_color: marker,
        corner_radii: Default::default(),
        border_widths: Default::default(),
    };
    let mut render = |left: f32, right: f32| {
        let mut scene = Scene::default();
        scene.insert_primitive(quad(full, black(), side_fade_marker(true, left, right)));
        scene.insert_primitive(quad(full, white(), Default::default()));
        scene.insert_primitive(quad(full, black(), side_fade_marker(false, left, right)));
        scene.finish();
        renderer
            .render_scene_to_image(&scene, size(DevicePixels(200), DevicePixels(16)))
            .unwrap()
    };
    let both = render(40., 40.);
    assert!(both.get_pixel(0, 8)[3] < 8, "the left edge is transparent");
    assert!((120..=136).contains(&both.get_pixel(20, 8)[3]));
    assert_eq!(both.get_pixel(100, 8)[3], 255, "the middle stays opaque");
    assert!((120..=136).contains(&both.get_pixel(179, 8)[3]));
    assert!(
        both.get_pixel(199, 8)[3] < 8,
        "the right edge is transparent"
    );
    let left = render(40., 0.);
    assert!(left.get_pixel(0, 8)[3] < 8);
    assert_eq!(
        left.get_pixel(199, 8)[3],
        255,
        "no right ramp without a width"
    );
}

#[test]
#[ignore = "Requires a Metal device; run explicitly on macOS"]
fn transcript_fades_mask_the_group_and_preserve_the_native_background() {
    use gpui::{Hsla, black, white};
    use gpui_apple::metal_renderer::edge_fade_marker;
    let mut renderer = MetalRenderer::new_headless(Arc::new(parking_lot::Mutex::new(
        InstanceBufferPool::default(),
    )));
    renderer.update_transparency(true);
    let full = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(32.), ScaledPixels(96.)),
    );
    let mask = Bounds::new(
        point(ScaledPixels(4.), ScaledPixels(8.)),
        size(ScaledPixels(24.), ScaledPixels(80.)),
    );
    let quad = |bounds, color: Hsla, marker: Hsla| Quad {
        order: 0,
        border_style: Default::default(),
        bounds,
        content_mask: ContentMask { bounds: full },
        background: color.into(),
        border_color: marker,
        corner_radii: Default::default(),
        border_widths: Default::default(),
    };
    let mut scene = Scene::default();
    scene.insert_primitive(quad(
        full,
        gpui::rgba(0x20408080).into(),
        Default::default(),
    ));
    scene.insert_primitive(quad(mask, black(), edge_fade_marker(true, 16., 24.)));
    // Overlapping fills must fade as one group, rather than accumulate alpha.
    scene.insert_primitive(quad(mask, white().opacity(0.6), Default::default()));
    scene.insert_primitive(quad(mask, white().opacity(0.6), Default::default()));
    scene.insert_primitive(quad(mask, black(), edge_fade_marker(false, 16., 24.)));
    // A control drawn after the mask must remain fully visible.
    let control = Bounds::new(
        point(ScaledPixels(18.), ScaledPixels(82.)),
        size(ScaledPixels(6.), ScaledPixels(4.)),
    );
    scene.insert_primitive(quad(control, white(), Default::default()));
    scene.finish();
    let image = renderer
        .render_scene_to_image(&scene, size(DevicePixels(32), DevicePixels(96)))
        .unwrap();
    let alpha = |y| image.get_pixel(8, y)[3] as i32;
    assert_eq!(
        image.get_pixel(0, 48)[3],
        128,
        "outside the transcript retains native transparency"
    );
    assert!(
        alpha(8) < 135 && alpha(87) < 135,
        "both edges reveal the original background"
    );
    assert!(
        (232..=237).contains(&alpha(48)),
        "center retains both overlapping fills"
    );
    assert!(
        (alpha(16) - ((128 + alpha(48)) / 2)).abs() < 7,
        "top midpoint fades the composite once"
    );
    assert!(
        (alpha(76) - ((128 + alpha(48)) / 2)).abs() < 7,
        "bottom midpoint uses its own fade height"
    );
    assert_eq!(
        image.get_pixel(20, 83)[3],
        255,
        "floating controls sit above the mask"
    );
}

#[test]
#[ignore = "Requires a Metal device; run explicitly on macOS"]
fn overlapping_shell_pane_and_hover_preserve_source_over_alpha() {
    let mut renderer = MetalRenderer::new_headless(Arc::new(parking_lot::Mutex::new(
        InstanceBufferPool::default(),
    )));
    renderer.update_transparency(true);
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(16.), ScaledPixels(16.)),
    );
    let quad = |background: gpui::Hsla, radius| Quad {
        order: 0,
        border_style: Default::default(),
        bounds,
        content_mask: ContentMask { bounds },
        background: background.into(),
        border_color: Default::default(),
        corner_radii: Corners::all(ScaledPixels(radius)),
        border_widths: Default::default(),
    };
    let mut scene = Scene::default();
    scene.insert_primitive(quad(shell_fill(), 0.));
    scene.insert_primitive(quad(pane_fill(), 6.));
    scene.finish();
    let pixels = renderer
        .render_scene_to_image(&scene, size(DevicePixels(16), DevicePixels(16)))
        .unwrap();
    let center = pixels.get_pixel(8, 8)[3];
    let corner = pixels.get_pixel(0, 0)[3];
    assert!(
        (225..=228).contains(&center),
        "pane must retain transparency, alpha={center}"
    );
    assert!(
        (205..=209).contains(&corner),
        "rounded corner must expose the shell, alpha={corner}"
    );

    let mut scene = Scene::default();
    scene.insert_primitive(quad(shell_fill(), 0.));
    scene.insert_primitive(quad(content(0.1), 0.));
    scene.finish();
    let pixels = renderer
        .render_scene_to_image(&scene, size(DevicePixels(16), DevicePixels(16)))
        .unwrap();
    assert!(
        (210..=213).contains(&pixels.get_pixel(8, 8)[3]),
        "hover fill must not accumulate alpha additively"
    );
}
#[test]
#[ignore = "Requires a Metal device; run explicitly on macOS"]
fn backdrop_marker_blurs_only_inside_its_rounded_bounds() {
    use gpui::{Edges, Hsla, black, white};
    let mut renderer = MetalRenderer::new_headless(Arc::new(parking_lot::Mutex::new(
        InstanceBufferPool::default(),
    )));
    let full = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(64.), ScaledPixels(32.)),
    );
    let quad = |order, bounds: Bounds<ScaledPixels>, background: Hsla, border_color| Quad {
        order,
        border_style: Default::default(),
        bounds,
        content_mask: ContentMask { bounds: full },
        background: background.into(),
        border_color,
        corner_radii: Corners::all(ScaledPixels(0.)),
        border_widths: Edges::default(),
    };
    let mut scene = Scene::default();
    // A hard black|white edge down the middle.
    scene.insert_primitive(quad(0, full, black(), Default::default()));
    scene.insert_primitive(quad(
        1,
        Bounds::new(
            full.center() - point(ScaledPixels(0.), ScaledPixels(16.)),
            size(ScaledPixels(32.), ScaledPixels(32.)),
        ),
        white(),
        Default::default(),
    ));
    // Frosted glass over the left half's right side, with no tint of its own.
    let glass = Bounds::new(
        point(ScaledPixels(16.), ScaledPixels(0.)),
        size(ScaledPixels(32.), ScaledPixels(32.)),
    );
    scene.insert_primitive(quad(
        2,
        glass,
        gpui::transparent_black(),
        gpui_apple::metal_renderer::backdrop_blur_marker(6.),
    ));
    scene.finish();
    let pixels = renderer
        .render_scene_to_image(&scene, size(DevicePixels(64), DevicePixels(32)))
        .unwrap();
    let red = |x| pixels.get_pixel(x, 16)[0];
    // Outside the glass the edge stays hard.
    assert_eq!((red(4), red(60)), (0, 255));
    // Inside, both sides of the edge soften toward each other.
    assert!(
        (20..120).contains(&red(30)),
        "left of edge, red={}",
        red(30)
    );
    assert!(
        (135..236).contains(&red(33)),
        "right of edge, red={}",
        red(33)
    );
}

#[test]
#[ignore = "Requires a Metal device; run explicitly on macOS"]
fn drop_shadows_paint_only_outside_their_element() {
    let mut renderer = MetalRenderer::new_headless(Arc::new(parking_lot::Mutex::new(
        InstanceBufferPool::default(),
    )));
    renderer.update_transparency(true);
    let full = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(48.), ScaledPixels(48.)),
    );
    let element = Bounds::new(
        point(ScaledPixels(12.), ScaledPixels(12.)),
        size(ScaledPixels(24.), ScaledPixels(24.)),
    );
    let mut scene = Scene::default();
    // An unfilled box with a dark shadow-sm-like drop beneath it.
    scene.insert_primitive(gpui::Shadow {
        order: 0,
        blur_radius: ScaledPixels(3.),
        bounds: element + point(ScaledPixels(0.), ScaledPixels(2.)),
        corner_radii: Corners::all(ScaledPixels(6.)),
        content_mask: ContentMask { bounds: full },
        color: gpui::black(),
        element_bounds: element,
        element_corner_radii: Corners::all(ScaledPixels(6.)),
        inset: 0,
        pad: 0,
    });
    scene.finish();
    let pixels = renderer
        .render_scene_to_image(&scene, size(DevicePixels(48), DevicePixels(48)))
        .unwrap();
    let alpha = |x, y| pixels.get_pixel(x, y)[3];
    assert_eq!(alpha(24, 24), 0, "the box's own interior stays clear");
    assert!(alpha(24, 37) > 0, "the drop still shows below the box");
}

#[test]
fn system_font_resolves_medium_to_its_own_face() {
    use gpui::{FontRun, FontWeight, PlatformTextSystem, font, px};
    let text = gpui_apple::AppleTextSystem::new();
    let width = |weight| {
        let mut f = font(".AppleSystemUIFont");
        f.weight = weight;
        let id = text.font_id(&f).unwrap();
        (
            id,
            text.layout_line(
                "Search",
                px(14.),
                &[FontRun {
                    len: 6,
                    font_id: id,
                }],
            )
            .width,
        )
    };
    let (regular, regular_width) = width(FontWeight::NORMAL);
    let (medium, medium_width) = width(FontWeight::MEDIUM);
    let (semibold, _) = width(FontWeight::SEMIBOLD);
    // `font-medium` labels (rail rows, search, titles) must not fall back to Regular.
    assert_ne!(medium, regular);
    assert_ne!(medium, semibold);
    assert!(medium_width > regular_width);
}
