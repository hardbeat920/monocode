//! editorScrollbar.ts's geometry, shared by native file and review surfaces.
pub(crate) const WIDTH: f32 = 18.;
pub(crate) const MIN_THUMB: f32 = 32.;

pub(crate) fn diff_colors(cx: &gpui::App) -> (u32, u32) {
    let palette = cx
        .try_global::<crate::settings::Preferences>()
        .map(|p| p.diff_palette.as_str())
        .unwrap_or("default");
    let light = crate::theme::content(1.).l < 0.5;
    match (palette, light) {
        ("colorblind", false) => (0x388bfd, 0xdb6d28),
        ("colorblind", true) => (0x0969da, 0xbc4c00),
        ("high-contrast", false) => (0x58a6ff, 0xf0883e),
        ("high-contrast", true) => (0x0550ae, 0x953800),
        _ => (0x10b981, 0xf43f5e),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Metrics {
    pub scrollable: bool,
    pub max_scroll: f32,
    pub max_thumb_top: f32,
    pub thumb_height: f32,
    pub thumb_top: f32,
}
impl Metrics {
    pub fn new(scroll: f32, content: f32, viewport: f32, track: f32) -> Self {
        let viewport = viewport.max(0.);
        let content = content.max(viewport);
        let track = track.max(0.);
        let max_scroll = (content - viewport).max(0.);
        if track == 0. || max_scroll == 0. {
            return Self {
                scrollable: false,
                max_scroll,
                max_thumb_top: 0.,
                thumb_height: track,
                thumb_top: 0.,
            };
        }
        let thumb_height = (track * viewport / content).max(MIN_THUMB).min(track);
        let max_thumb_top = (track - thumb_height).max(0.);
        Self {
            scrollable: true,
            max_scroll,
            max_thumb_top,
            thumb_height,
            thumb_top: (scroll / max_scroll).clamp(0., 1.) * max_thumb_top,
        }
    }
    pub fn scroll_from_pointer(self, y: f32, drag_offset: f32) -> f32 {
        if self.max_thumb_top == 0. {
            return 0.;
        }
        ((y - drag_offset) / self.max_thumb_top).clamp(0., 1.) * self.max_scroll
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metrics_match_frontend_fixtures_and_drag_endpoints() {
        let m = Metrics::new(450., 1000., 100., 200.);
        assert_eq!(
            m,
            Metrics {
                scrollable: true,
                max_scroll: 900.,
                max_thumb_top: 168.,
                thumb_height: 32.,
                thumb_top: 84.
            }
        );
        assert_eq!(Metrics::new(0., 100., 100., 200.).thumb_height, 200.);
        assert!(!Metrics::new(0., 100., 100., 200.).scrollable);
        assert_eq!(Metrics::new(2000., 1000., 200., 100.).thumb_top, 68.);
        assert_eq!(m.scroll_from_pointer(200., 16.), 900.);
        assert_eq!(m.scroll_from_pointer(0., 16.), 0.);
    }
}
