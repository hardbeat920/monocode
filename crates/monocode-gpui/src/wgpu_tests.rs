//! Pixel regressions for Linux scroll-edge masks, independent of the desktop compositor.
use gpui::{
    BorderStyle, Bounds, ContentMask, Corners, DevicePixels, EdgeFade, Edges, Hsla,
    PlatformHeadlessRenderer, Point, Quad, Scene, Size,
};
use gpui_wgpu::WgpuHeadlessRenderer;

fn device_size(width: i32, height: i32) -> Size<DevicePixels> {
    Size {
        width: DevicePixels(width),
        height: DevicePixels(height),
    }
}

fn solid_quad(x: f32, y: f32, width: f32, height: f32, color: Hsla) -> Quad {
    let bounds = Bounds {
        origin: Point {
            x: x.into(),
            y: y.into(),
        },
        size: Size {
            width: width.into(),
            height: height.into(),
        },
    };
    Quad {
        order: 0,
        border_style: BorderStyle::Solid,
        bounds,
        content_mask: ContentMask { bounds },
        background: color.into(),
        border_color: color,
        corner_radii: Corners::default(),
        border_widths: Edges::default(),
    }
}

fn assert_pixel(image: &image::RgbaImage, x: u32, y: u32, expected: [u8; 4]) {
    let actual = image.get_pixel(x, y).0;
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.abs_diff(expected) <= 3),
        "pixel ({x}, {y}) was {actual:?}, expected {expected:?}"
    );
}

fn fade_marker(mut quad: Quad, fade: EdgeFade) -> Quad {
    quad.border_color = fade.marker();
    quad
}

fn assert_masked_pixels(
    before: &image::RgbaImage,
    after: &image::RgbaImage,
    masked: &image::RgbaImage,
    coverage: impl Fn(u32, u32) -> f32,
) {
    for (x, y, actual) in masked.enumerate_pixels() {
        let a = before.get_pixel(x, y).0;
        let b = after.get_pixel(x, y).0;
        let expected = std::array::from_fn(|channel| {
            (a[channel] as f32 + (b[channel] as f32 - a[channel] as f32) * coverage(x, y)).round()
                as u8
        });
        assert_pixel(masked, x, y, expected);
        assert!(
            actual.0[3] >= a[3],
            "a mask must preserve the backdrop's alpha"
        );
    }
}

fn edge_coverage(y: f32, height: f32, top: f32, bottom: f32) -> f32 {
    let top = if top > 0. {
        (y / top).clamp(0., 1.)
    } else {
        1.
    };
    let bottom = if bottom > 0. {
        ((height - y) / bottom).clamp(0., 1.)
    } else {
        1.
    };
    top.min(bottom)
}

#[test]
#[ignore = "requires a Vulkan or GLES device"]
fn edge_fade_masks_overlapping_content_without_tinting_the_backdrop()
-> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = WgpuHeadlessRenderer::new()?;
    for (width, height) in [(64, 64), (73, 57)] {
        let marker = solid_quad(
            4.,
            4.,
            (width - 8) as f32,
            (height - 8) as f32,
            gpui::black(),
        );
        let background = || {
            let mut scene = Scene::default();
            scene.insert_primitive(solid_quad(
                0.,
                0.,
                width as f32 / 2.,
                height as f32,
                gpui::blue(),
            ));
            scene.insert_primitive(solid_quad(
                width as f32 / 2.,
                0.,
                width as f32,
                height as f32,
                gpui::green(),
            ));
            scene
        };
        let artwork = |scene: &mut Scene| {
            scene.insert_primitive(solid_quad(10., 4., 28., (height - 8) as f32, gpui::red()));
            let mut white = gpui::white();
            white.a = 0.6;
            scene.insert_primitive(solid_quad(18., 4., 32., (height - 8) as f32, white));
        };
        let size = device_size(width, height);
        let mut before = background();
        before.finish();
        let before = renderer.render_scene_to_image(&before, size)?;
        let mut after = background();
        artwork(&mut after);
        after.finish();
        let after = renderer.render_scene_to_image(&after, size)?;
        for (top, bottom) in [(0., 0.), (16., 0.), (0., 16.), (16., 16.)] {
            let mut scene = background();
            scene.insert_primitive(fade_marker(marker, EdgeFade::Begin { top, bottom }));
            artwork(&mut scene);
            scene.insert_primitive(fade_marker(marker, EdgeFade::End { top, bottom }));
            scene.finish();
            let masked = renderer.render_scene_to_image(&scene, size)?;
            assert_masked_pixels(&before, &after, &masked, |_, y| {
                edge_coverage(y as f32 + 0.5 - 4., (height - 8) as f32, top, bottom)
            });
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan or GLES device"]
fn edge_fade_preserves_window_transparency_and_premultiplied_color()
-> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = WgpuHeadlessRenderer::new_transparent()?;
    let size = device_size(32, 32);
    let mut render = |scene: &Scene| renderer.render_scene_to_image(scene, size);
    let before = render(&Scene::default())?;
    let mut after = Scene::default();
    let mut red = gpui::red();
    red.a = 0.75;
    after.insert_primitive(solid_quad(0., 0., 32., 32., red));
    after.finish();
    let after = render(&after)?;
    let marker = solid_quad(0., 0., 32., 32., gpui::black());
    let mut scene = Scene::default();
    scene.insert_primitive(fade_marker(
        marker,
        EdgeFade::Begin {
            top: 12.,
            bottom: 12.,
        },
    ));
    scene.insert_primitive(solid_quad(0., 0., 32., 32., red));
    scene.insert_primitive(fade_marker(
        marker,
        EdgeFade::End {
            top: 12.,
            bottom: 12.,
        },
    ));
    scene.finish();
    let masked = render(&scene)?;
    assert_masked_pixels(&before, &after, &masked, |_, y| {
        edge_coverage(y as f32 + 0.5, 32., 12., 12.)
    });
    assert!(
        masked.get_pixel(16, 0).0[3] < 10,
        "the edge must reveal the desktop instead of an opaque overlay"
    );
    assert!(
        masked.get_pixel(16, 16).0[3] > 180,
        "the center must retain the content's alpha"
    );
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan or GLES device"]
fn edge_fade_composes_nested_and_disjoint_groups() -> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = WgpuHeadlessRenderer::new()?;
    let size = device_size(80, 64);
    let left = solid_quad(0., 0., 32., 64., gpui::black());
    let right = solid_quad(40., 0., 32., 64., gpui::black());
    let outer = solid_quad(0., 0., 80., 64., gpui::black());
    let background = || {
        let mut scene = Scene::default();
        scene.insert_primitive(solid_quad(0., 0., 80., 64., gpui::blue()));
        scene
    };
    let mut before = background();
    before.finish();
    let before = renderer.render_scene_to_image(&before, size)?;
    let mut after = background();
    after.insert_primitive(solid_quad(0., 0., 32., 64., gpui::red()));
    after.insert_primitive(solid_quad(40., 0., 32., 64., gpui::green()));
    after.finish();
    let after = renderer.render_scene_to_image(&after, size)?;
    let mut scene = background();
    scene.insert_primitive(fade_marker(
        outer,
        EdgeFade::Begin {
            top: 8.,
            bottom: 8.,
        },
    ));
    scene.insert_primitive(fade_marker(
        left,
        EdgeFade::Begin {
            top: 24.,
            bottom: 0.,
        },
    ));
    scene.insert_primitive(fade_marker(
        right,
        EdgeFade::Begin {
            top: 0.,
            bottom: 24.,
        },
    ));
    scene.insert_primitive(solid_quad(0., 0., 32., 64., gpui::red()));
    scene.insert_primitive(solid_quad(40., 0., 32., 64., gpui::green()));
    scene.insert_primitive(fade_marker(
        left,
        EdgeFade::End {
            top: 24.,
            bottom: 0.,
        },
    ));
    scene.insert_primitive(fade_marker(
        right,
        EdgeFade::End {
            top: 0.,
            bottom: 24.,
        },
    ));
    scene.insert_primitive(fade_marker(
        outer,
        EdgeFade::End {
            top: 8.,
            bottom: 8.,
        },
    ));
    // A later sibling must draw normally, outside all masked groups.
    scene.insert_primitive(solid_quad(76., 0., 4., 64., gpui::white()));
    scene.finish();
    let masked = renderer.render_scene_to_image(&scene, size)?;
    for y in 0..64 {
        for x in 0..76 {
            let position = y as f32 + 0.5;
            let inner = if x < 32 {
                edge_coverage(position, 64., 24., 0.)
            } else {
                edge_coverage(position, 64., 0., 24.)
            };
            let coverage = inner * edge_coverage(position, 64., 8., 8.);
            let a = before.get_pixel(x, y).0;
            let b = after.get_pixel(x, y).0;
            let expected = std::array::from_fn(|channel| {
                (a[channel] as f32 + (b[channel] as f32 - a[channel] as f32) * coverage).round()
                    as u8
            });
            assert_pixel(&masked, x, y, expected);
        }
        assert_pixel(&masked, 78, y, [255; 4]);
    }
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan or GLES device"]
fn edge_fade_anchors_ramps_to_original_bounds_when_clipped()
-> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = WgpuHeadlessRenderer::new()?;
    let size = device_size(32, 32);
    let mut marker = solid_quad(-4., -8., 40., 48., gpui::black());
    marker.content_mask = solid_quad(4., 3., 24., 24., gpui::black()).content_mask;
    let mut content = solid_quad(0., 0., 32., 32., gpui::red());
    content.content_mask = marker.content_mask;
    let mut before = Scene::default();
    before.insert_primitive(solid_quad(0., 0., 32., 32., gpui::blue()));
    before.finish();
    let before = renderer.render_scene_to_image(&before, size)?;
    let mut after = Scene::default();
    after.insert_primitive(solid_quad(0., 0., 32., 32., gpui::blue()));
    after.insert_primitive(content);
    after.finish();
    let after = renderer.render_scene_to_image(&after, size)?;
    let mut scene = Scene::default();
    scene.insert_primitive(solid_quad(0., 0., 32., 32., gpui::blue()));
    scene.insert_primitive(fade_marker(
        marker,
        EdgeFade::Begin {
            top: 24.,
            bottom: 24.,
        },
    ));
    scene.insert_primitive(content);
    scene.insert_primitive(fade_marker(
        marker,
        EdgeFade::End {
            top: 24.,
            bottom: 24.,
        },
    ));
    scene.finish();
    let masked = renderer.render_scene_to_image(&scene, size)?;
    assert_masked_pixels(&before, &after, &masked, |_, y| {
        edge_coverage(y as f32 + 0.5 + 8., 48., 24., 24.)
    });
    Ok(())
}

#[test]
#[ignore = "requires a Vulkan or GLES device"]
fn edge_fade_captures_pixel_centers_inside_fractional_bounds()
-> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = WgpuHeadlessRenderer::new()?;
    let size = device_size(32, 32);
    let marker = solid_quad(4.3, 4.3, 23.4, 23.4, gpui::black());
    let mut content = solid_quad(0., 0., 32., 32., gpui::red());
    content.content_mask = marker.content_mask;

    let mut before = Scene::default();
    before.insert_primitive(solid_quad(0., 0., 32., 32., gpui::blue()));
    before.finish();
    let before = renderer.render_scene_to_image(&before, size)?;

    let mut after = Scene::default();
    after.insert_primitive(solid_quad(0., 0., 32., 32., gpui::blue()));
    after.insert_primitive(content);
    after.finish();
    let after = renderer.render_scene_to_image(&after, size)?;

    let mut scene = Scene::default();
    scene.insert_primitive(solid_quad(0., 0., 32., 32., gpui::blue()));
    scene.insert_primitive(fade_marker(
        marker,
        EdgeFade::Begin {
            top: 8.,
            bottom: 8.,
        },
    ));
    scene.insert_primitive(content);
    scene.insert_primitive(fade_marker(
        marker,
        EdgeFade::End {
            top: 8.,
            bottom: 8.,
        },
    ));
    scene.finish();
    let masked = renderer.render_scene_to_image(&scene, size)?;
    assert_masked_pixels(&before, &after, &masked, |_, y| {
        edge_coverage(y as f32 + 0.5 - 4.3, 23.4, 8., 8.)
    });
    Ok(())
}
