//! Native menu presentation. Selectable rows remain ordinary NSMenuItems so
//! AppKit owns keyboard navigation, accessibility and actions. Each carries a
//! row view only so hover draws the translucent pill of the system's status
//! menus instead of the accent-colored selection.

use std::panic::AssertUnwindSafe;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::runtime::{AnyObject, Bool};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSFont, NSFontWeightRegular,
    NSFontWeightSemibold, NSImage, NSImageSymbolConfiguration, NSImageView, NSMenu, NSMenuItem,
    NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use serde::Deserialize;

const WIDTH: f64 = 260.0;
const PORTRAIT_SIZE: f64 = 28.0;
const ROW_HEIGHT: f64 = 34.0;
/// Where AppKit placed the image and title of a plain item, so rows with a
/// view line up with the header the same way.
const IMAGE_X: f64 = 15.0;
const TITLE_X: f64 = 49.0;
const PORTRAIT_PIXELS: usize = 56;
/// Pixels per mascot unit: a 1.5-unit sprite cell lands on exactly 3 pixels,
/// and the 16-unit sprite fills a little over half the circle.
const SPRITE_SCALE: f64 = 2.0;

pub(super) struct Action {
    pub id: &'static str,
    pub title: &'static str,
    symbol: &'static str,
    /// Tinted red, like other menus' destructive items.
    destructive: bool,
}

/// The actions below the Monos, each in its own group. Each gets a circle
/// like a portrait's so every title in the menu shares one column.
pub(super) const ACTIONS: [Action; 2] = [
    Action {
        id: "mono-chat-composer",
        title: "Quick Composer",
        symbol: "square.and.pencil",
        destructive: false,
    },
    Action {
        id: "mono-chat-quit",
        title: "Quit MonoCode",
        symbol: "power",
        destructive: true,
    },
];

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MenuMascot {
    mono_id: String,
    rects: Vec<MascotRect>,
}

#[derive(Clone, Deserialize, PartialEq)]
struct MascotRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: String,
}

pub(super) fn decorate(tray: &tauri::tray::TrayIcon) -> tauri::Result<()> {
    tray.with_inner_tray_icon(move |tray| {
        // Tauri dispatches this closure onto the main thread. Keep foreign
        // exceptions inside this boundary rather than unwinding through Rust.
        objc2::exception::catch(AssertUnwindSafe(|| {
            let mtm = MainThreadMarker::new().expect("native menu runs on the main thread");
            let Some(menu) = tray.ns_status_item().and_then(|status| status.menu(mtm)) else {
                return;
            };
            style_menu(&menu, mtm);
            for item in menu.itemArray() {
                if item.isSeparatorItem() || !item.isEnabled() {
                    continue;
                }
                let title = item.title().to_string();
                if let Some(action) = ACTIONS.iter().find(|a| a.title == title) {
                    let image = action_icon(action);
                    item.setImage(image.as_deref());
                    show_image(&item);
                    let color = if action.destructive {
                        NSColor::systemRedColor()
                    } else {
                        NSColor::labelColor()
                    };
                    item.setView(Some(&row(&title, image.as_deref(), &color, mtm)));
                } else if let Some(image) = item.image() {
                    // Muda stores the portrait in its IconMenuItem model and
                    // initially sizes it to 18pt. Only adjust its display size.
                    image.setSize(NSSize::new(PORTRAIT_SIZE, PORTRAIT_SIZE));
                    item.setImage(Some(&image));
                    show_image(&item);
                    let view = row(&title, Some(&image), &NSColor::labelColor(), mtm);
                    view.setToolTip(Some(&NSString::from_str("Open floating chat")));
                    item.setView(Some(&view));
                }
            }
        }))
        .map_err(|exception| format!("Could not style the Mono menu: {exception:?}"))
    })?
    .map_err(|error| std::io::Error::other(error).into())
}

/// macOS 27 hides menu item images unless the item asks to keep them.
fn show_image(item: &NSMenuItem) {
    // NSMenuItemImageVisibilityVisible; objc2-app-kit predates the property.
    const VISIBLE: isize = 1;
    if item.respondsToSelector(sel!(setPreferredImageVisibility:)) {
        let () = unsafe { msg_send![item, setPreferredImageVisibility: VISIBLE] };
    }
}

/// An SF Symbol in a faint circle the size of a Mono portrait. As a template
/// image it follows the menu's text color, including when highlighted; a
/// destructive action keeps its red instead.
fn action_icon(action: &Action) -> Option<Retained<NSImage>> {
    let red = NSColor::systemRedColor();
    let mut config = NSImageSymbolConfiguration::configurationWithPointSize_weight(13.0, unsafe {
        NSFontWeightRegular
    });
    if action.destructive {
        config = config.configurationByApplyingConfiguration(
            &NSImageSymbolConfiguration::configurationWithHierarchicalColor(&red),
        );
    }
    let glyph = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(action.symbol),
        None,
    )?
    .imageWithSymbolConfiguration(&config)?;
    let ink = if action.destructive {
        red.colorWithAlphaComponent(0.15)
    } else {
        NSColor::colorWithWhite_alpha(0.0, 0.12)
    };
    let draw = block2::RcBlock::new(move |rect: NSRect| {
        // Matches the portrait's circle: 1pt inside the image, faint ink.
        ink.setFill();
        NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
            NSPoint::new(rect.origin.x + 1.0, rect.origin.y + 1.0),
            NSSize::new(rect.size.width - 2.0, rect.size.height - 2.0),
        ))
        .fill();
        let size = glyph.size();
        glyph.drawInRect(NSRect::new(
            NSPoint::new(
                rect.origin.x + (rect.size.width - size.width) / 2.0,
                rect.origin.y + (rect.size.height - size.height) / 2.0,
            ),
            size,
        ));
        Bool::YES
    });
    let image = NSImage::imageWithSize_flipped_drawingHandler(
        NSSize::new(PORTRAIT_SIZE, PORTRAIT_SIZE),
        false,
        &draw,
    );
    image.setTemplate(!action.destructive);
    Some(image)
}

define_class!(
    /// A selectable row. A menu item with a view draws nothing itself, so the
    /// row draws the hover pill and forwards clicks to the item's action.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "MonoCodeMenuRow"]
    struct MenuRow;

    impl MenuRow {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if !self.enclosingMenuItem().is_some_and(|item| item.isHighlighted()) {
                return;
            }
            let bounds = self.bounds();
            NSColor::labelColor().colorWithAlphaComponent(0.1).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                NSRect::new(
                    NSPoint::new(bounds.origin.x + 5.0, bounds.origin.y),
                    NSSize::new(bounds.size.width - 10.0, bounds.size.height),
                ),
                10.0,
                10.0,
            )
            .fill();
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: Option<&AnyObject>) {
            let Some(item) = self.enclosingMenuItem() else {
                return;
            };
            // The item is in the menu that is tracking this view.
            let Some(menu) = (unsafe { item.menu() }) else {
                return;
            };
            menu.cancelTracking();
            menu.performActionForItemAtIndex(menu.indexOfItem(&item));
        }
    }
);

fn row(
    title: &str,
    image: Option<&NSImage>,
    color: &NSColor,
    mtm: MainThreadMarker,
) -> Retained<NSView> {
    let frame = NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, ROW_HEIGHT));
    let view: Retained<MenuRow> = unsafe { msg_send![MenuRow::alloc(mtm), initWithFrame: frame] };
    view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    if let Some(image) = image {
        let icon = NSImageView::imageViewWithImage(image, mtm);
        icon.setFrame(NSRect::new(
            NSPoint::new(IMAGE_X, (ROW_HEIGHT - PORTRAIT_SIZE) / 2.0),
            NSSize::new(PORTRAIT_SIZE, PORTRAIT_SIZE),
        ));
        view.addSubview(&icon);
    }
    let label = NSTextField::labelWithString(&NSString::from_str(title), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(14.0)));
    label.setTextColor(Some(color));
    label.sizeToFit();
    let height = label.frame().size.height;
    label.setFrame(NSRect::new(
        NSPoint::new(TITLE_X, ((ROW_HEIGHT - height) / 2.0).round()),
        NSSize::new(WIDTH - TITLE_X - 16.0, height),
    ));
    label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    view.addSubview(&label);
    view.into_super()
}

fn style_menu(menu: &NSMenu, mtm: MainThreadMarker) {
    menu.setMinimumWidth(WIDTH);
    unsafe { menu.setFont(Some(&NSFont::systemFontOfSize(14.0))) };
    let Some(header) = menu.itemAtIndex(0) else {
        return;
    };
    // A static header view gives the menu the hierarchy of macOS's status
    // menus. Only this noninteractive item has a custom view.
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, 30.0)),
    );
    let title = NSTextField::labelWithString(&NSString::from_str("Monos"), mtm);
    title.setFont(Some(&NSFont::systemFontOfSize_weight(13.0, unsafe {
        NSFontWeightSemibold
    })));
    title.setTextColor(Some(&NSColor::labelColor()));
    title.setFrame(NSRect::new(
        NSPoint::new(16.0, 7.0),
        NSSize::new(WIDTH - 32.0, 17.0),
    ));
    title.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    view.addSubview(&title);
    header.setView(Some(&view));
}

pub(super) fn icon_for(
    mono_id: &str,
    mascots: &[MenuMascot],
) -> Option<tauri::image::Image<'static>> {
    mascots
        .iter()
        .find(|mascot| mascot.mono_id == mono_id)
        .map(|mascot| portrait(&mascot.rects))
}

fn portrait(rects: &[MascotRect]) -> tauri::image::Image<'static> {
    // Feed real RGBA pixels to IconMenuItemBuilder so the native menu model
    // owns the image. Render at 2× for a crisp 28pt Retina portrait, using the
    // same shaded layers and crisp rectangle edges as the in-app mascot.
    let mut rgba = vec![0; PORTRAIT_PIXELS * PORTRAIT_PIXELS * 4];
    let center = PORTRAIT_PIXELS as f64 / 2.0;
    // The sprite sits centred in the circle, like the glyphs in macOS's
    // own status menus, rather than filling it.
    let inset = center - 8.0 * SPRITE_SCALE;
    for y in 0..PORTRAIT_PIXELS {
        for x in 0..PORTRAIT_PIXELS {
            let distance =
                ((x as f64 + 0.5 - center).powi(2) + (y as f64 + 0.5 - center).powi(2)).sqrt();
            let alpha = (center - 2.0 - distance + 0.5).clamp(0.0, 1.0) * 0.12;
            blend(
                &mut rgba[(y * PORTRAIT_PIXELS + x) * 4..][..4],
                [0.5, 0.5, 0.5, alpha],
            );
        }
    }
    for rect in rects {
        let Some(fill) = color(&rect.fill) else {
            continue;
        };
        let edge = |value: f64| {
            (inset + value * SPRITE_SCALE)
                .round()
                .clamp(0.0, PORTRAIT_PIXELS as f64) as usize
        };
        for y in edge(rect.y)..edge(rect.y + rect.h) {
            for x in edge(rect.x)..edge(rect.x + rect.w) {
                blend(&mut rgba[(y * PORTRAIT_PIXELS + x) * 4..][..4], fill);
            }
        }
    }
    tauri::image::Image::new_owned(rgba, PORTRAIT_PIXELS as u32, PORTRAIT_PIXELS as u32)
}

fn blend(pixel: &mut [u8], [r, g, b, a]: [f64; 4]) {
    let previous = f64::from(pixel[3]) / 255.0;
    let alpha = a + previous * (1.0 - a);
    if alpha == 0.0 {
        return;
    }
    for (channel, source) in pixel[..3].iter_mut().zip([r, g, b]) {
        *channel = ((source * a + f64::from(*channel) / 255.0 * previous * (1.0 - a)) / alpha
            * 255.0)
            .round() as u8;
    }
    pixel[3] = (alpha * 255.0).round() as u8;
}

/// The three color formats emitted by pixelLayers (not arbitrary user CSS).
fn color(fill: &str) -> Option<[f64; 4]> {
    if let Some(hex) = fill.strip_prefix('#').filter(|hex| hex.len() == 6) {
        let rgb = u32::from_str_radix(hex, 16).ok()?;
        return Some([
            ((rgb >> 16) & 255) as f64 / 255.0,
            ((rgb >> 8) & 255) as f64 / 255.0,
            (rgb & 255) as f64 / 255.0,
            1.0,
        ]);
    }
    let values: Vec<f64> = fill
        .split(['(', ')', ',', ' ', '%'])
        .filter_map(|value| value.parse().ok())
        .collect();
    if fill.starts_with("rgba(") && values.len() == 4 {
        return Some([
            values[0] / 255.0,
            values[1] / 255.0,
            values[2] / 255.0,
            values[3],
        ]);
    }
    if fill.starts_with("hsl(") && values.len() == 3 {
        let [h, s, l] = [values[0] / 30.0, values[1] / 100.0, values[2] / 100.0];
        let amplitude = s * l.min(1.0 - l);
        let channel = |offset: f64| {
            let k = (offset + h).rem_euclid(12.0);
            l - amplitude * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
        };
        return Some([channel(0.0), channel(8.0), channel(4.0), 1.0]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_portrait_preserves_pixel_mascot_colors_and_alpha() {
        assert_eq!(
            color("#263331"),
            Some([38.0 / 255.0, 51.0 / 255.0, 49.0 / 255.0, 1.0])
        );
        assert_eq!(
            color("rgba(255,255,244,0.75)"),
            Some([1.0, 1.0, 244.0 / 255.0, 0.75])
        );
        for (fill, expected) in [
            ("hsl(0 100% 50%)", [1.0, 0.0, 0.0, 1.0]),
            ("hsl(120 100% 50%)", [0.0, 1.0, 0.0, 1.0]),
            ("hsl(240 100% 50%)", [0.0, 0.0, 1.0, 1.0]),
            ("hsl(90 0% 60%)", [0.6, 0.6, 0.6, 1.0]),
        ] {
            assert_eq!(color(fill), Some(expected), "{fill}");
        }
        assert_eq!(color("unknown"), None);
    }

    #[test]
    fn menu_portrait_contains_the_mascot_pixels_and_transparent_corners() {
        let image = portrait(&[
            MascotRect {
                x: 2.0,
                y: 2.0,
                w: 12.0,
                h: 12.0,
                fill: "hsl(0 100% 50%)".into(),
            },
            MascotRect {
                x: 5.0,
                y: 5.0,
                w: 1.5,
                h: 1.5,
                fill: "#263331".into(),
            },
        ]);
        assert_eq!((image.width(), image.height()), (56, 56));
        let pixel = |x: usize, y: usize| &image.rgba()[(y * 56 + x) * 4..][..4];
        assert_eq!(pixel(0, 0), [0, 0, 0, 0]);
        // The 16-unit sprite spans the middle 32 pixels, inset 12 on each side.
        assert_eq!(pixel(18, 18), [255, 0, 0, 255]);
        assert_eq!(pixel(23, 23), [38, 51, 49, 255]);
        assert_eq!(pixel(23, 36), [255, 0, 0, 255], "sprite stays upright");
        assert_ne!(pixel(14, 28), [255, 0, 0, 255], "sprite leaves a margin");
    }

    #[test]
    fn appkit_accepts_the_png_used_by_native_icon_menu_items() {
        use objc2::{rc::Retained, AllocAnyThread};
        use objc2_app_kit::NSImage;
        use objc2_foundation::NSData;

        let image = portrait(&[MascotRect {
            x: 2.0,
            y: 2.0,
            w: 12.0,
            h: 12.0,
            fill: "hsl(211 44% 81%)".into(),
        }]);
        // Muda's IconMenuItem path encodes RGBA as PNG and loads that PNG into
        // NSImage. Exercise that path rather than a separate drawing callback.
        let mut png_bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut png_bytes, image.width(), image.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(image.rgba())
            .unwrap();
        objc2::exception::catch(|| {
            let image: Retained<NSImage> =
                NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&png_bytes)).unwrap();
            image.setSize(NSSize::new(PORTRAIT_SIZE, PORTRAIT_SIZE));
            let raster = unsafe {
                image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None)
            };
            assert!(raster.is_some(), "AppKit did not draw the mascot");
        })
        .unwrap_or_else(|exception| panic!("AppKit rejected the portrait: {exception:?}"));
    }

    #[test]
    fn every_menu_action_draws_a_portrait_sized_symbol() {
        objc2::exception::catch(|| {
            for action in &ACTIONS {
                let title = action.title;
                let image = action_icon(action).unwrap_or_else(|| panic!("{title}"));
                assert_eq!(image.size(), NSSize::new(PORTRAIT_SIZE, PORTRAIT_SIZE));
                assert_eq!(image.isTemplate(), !action.destructive);
                let raster = unsafe {
                    image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None)
                };
                assert!(raster.is_some(), "{title} did not draw");
            }
        })
        .unwrap_or_else(|exception| panic!("AppKit rejected an action icon: {exception:?}"));
    }

    #[test]
    fn each_mono_receives_its_own_portrait_when_the_payload_order_changes() {
        let mascots = [
            MenuMascot {
                mono_id: "second".into(),
                rects: vec![MascotRect {
                    x: 2.0,
                    y: 2.0,
                    w: 12.0,
                    h: 12.0,
                    fill: "#0000ff".into(),
                }],
            },
            MenuMascot {
                mono_id: "first".into(),
                rects: vec![MascotRect {
                    x: 2.0,
                    y: 2.0,
                    w: 12.0,
                    h: 12.0,
                    fill: "#ff0000".into(),
                }],
            },
        ];
        let center = (28 * 56 + 28) * 4;
        assert_eq!(
            &icon_for("first", &mascots).unwrap().rgba()[center..][..4],
            [255, 0, 0, 255]
        );
        assert_eq!(
            &icon_for("second", &mascots).unwrap().rgba()[center..][..4],
            [0, 0, 255, 255]
        );
        assert!(icon_for("missing", &mascots).is_none());
    }
}
