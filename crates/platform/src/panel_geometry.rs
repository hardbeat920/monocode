//! Frame math for the quick composer panel and its git popup. Port of the
//! sizing and placement in src-tauri/src/quick_composer.rs (`place`,
//! `quick_composer_fit`) and src-tauri/src/quick_composer/git_popup.rs
//! (`popup_frame`, `over_trigger`).
//!
//! Rectangles use AppKit screen coordinates: the origin is the bottom left
//! corner and y grows upward. Anchors come from the panel's content, so they
//! are measured from the panel's top left corner with y growing downward.

/// The panel width, in points.
pub const QUICK_COMPOSER_WIDTH: f64 = 680.0;
/// The panel height before the card reports its own.
pub const QUICK_COMPOSER_INITIAL_HEIGHT: f64 = 128.0;
/// Must match the card's radius so the blur, border, and native shadow share
/// one outline.
pub const QUICK_COMPOSER_CORNER_RADIUS: f64 = 16.0;
/// The tallest the panel grows.
pub const QUICK_COMPOSER_MAX_HEIGHT: f64 = 520.0;
/// How far down the work area the panel's top edge sits, like Spotlight.
pub const QUICK_COMPOSER_TOP_FRACTION: f64 = 0.22;

/// The git popup's width, in points.
pub const GIT_POPUP_WIDTH: f64 = 320.0;
/// The tallest the git popup grows.
pub const GIT_POPUP_MAX_HEIGHT: f64 = 520.0;
/// The git popup's corner radius.
pub const GIT_POPUP_CORNER_RADIUS: f64 = 12.0;
/// The gap between the popup and the control that opened it.
pub const GIT_POPUP_GAP: f64 = 6.0;

/// A point in screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelPoint {
    pub x: f64,
    pub y: f64,
}

impl PanelPoint {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A rectangle in screen coordinates (origin at the bottom left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl PanelRect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The y of the top edge.
    pub fn top(&self) -> f64 {
        self.y + self.height
    }

    pub fn contains(&self, point: PanelPoint) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.width
            && point.y >= self.y
            && point.y <= self.top()
    }
}

/// The control that opened a popup, relative to the panel's top left
/// corner, as the panel's content measures it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelAnchor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl PanelAnchor {
    /// Every field is a finite number, as `quick_git_open` required.
    pub fn is_finite(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|n| n.is_finite())
    }
}

/// `quick_composer_fit`: grow or shrink to the card while the top edge stays
/// where it was, so the prompt does not jump while a list opens. The width
/// is always `width`. A height that is not finite keeps the frame.
pub fn fit_frame(frame: PanelRect, width: f64, height: f64, max_height: f64) -> PanelRect {
    if !height.is_finite() {
        return frame;
    }
    let height = height.clamp(1.0, max_height);
    PanelRect {
        x: frame.x,
        y: frame.y + frame.height - height,
        width,
        height,
    }
}

/// `place`: centered across the work area of the screen under the pointer,
/// with the top edge `top_fraction` of the way down.
pub fn place_frame(work_area: PanelRect, width: f64, height: f64, top_fraction: f64) -> PanelRect {
    let x = work_area.x + ((work_area.width - width) / 2.0).round();
    let top = work_area.top() - (work_area.height * top_fraction).round();
    PanelRect {
        x,
        y: top - height,
        width,
        height,
    }
}

/// `popup_frame`: below the anchor when it fits on screen, else above it,
/// clamped to the screen. The composer's own frame never changes.
pub fn popup_frame(
    parent: PanelRect,
    screen: PanelRect,
    anchor: &PanelAnchor,
    height: f64,
    width: f64,
    max_height: f64,
) -> PanelRect {
    let width = width.min(screen.width);
    let height = height.clamp(1.0, max_height).min(screen.height);
    let x = (parent.x + anchor.x).clamp(screen.x, screen.x + screen.width - width);
    let anchor_top = parent.y + parent.height - anchor.y;
    let below = anchor_top - anchor.height - GIT_POPUP_GAP - height;
    let above = anchor_top + GIT_POPUP_GAP;
    let y = if below >= screen.y { below } else { above };
    let y = y.clamp(screen.y, screen.y + screen.height - height);
    PanelRect::new(x, y, width, height)
}

/// `over_trigger`: the point is over the anchor control of the panel.
pub fn over_trigger(parent: PanelRect, anchor: &PanelAnchor, point: PanelPoint) -> bool {
    let left = parent.x + anchor.x;
    let top = parent.y + parent.height - anchor.y;
    point.x >= left
        && point.x <= left + anchor.width
        && point.y <= top
        && point.y >= top - anchor.height
}

/// The screen whose frame contains `point`, else the first one.
pub fn screen_at(screens: &[(PanelRect, PanelRect)], point: PanelPoint) -> Option<PanelRect> {
    screens
        .iter()
        .find(|(frame, _)| frame.contains(point))
        .or(screens.first())
        .map(|(_, visible)| *visible)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(parent: PanelRect, screen: PanelRect, anchor: &PanelAnchor, height: f64) -> PanelRect {
        popup_frame(
            parent,
            screen,
            anchor,
            height,
            GIT_POPUP_WIDTH,
            GIT_POPUP_MAX_HEIGHT,
        )
    }

    #[test]
    fn blur_only_marks_a_click_inside_the_current_trigger() {
        let parent = PanelRect::new(-600.0, 400.0, 680.0, 140.0);
        let anchor = PanelAnchor {
            x: 30.0,
            y: 10.0,
            width: 100.0,
            height: 24.0,
        };
        assert!(over_trigger(
            parent,
            &anchor,
            PanelPoint::new(-550.0, 520.0)
        ));
        assert!(!over_trigger(
            parent,
            &anchor,
            PanelPoint::new(-400.0, 520.0)
        ));
        assert!(!over_trigger(
            parent,
            &anchor,
            PanelPoint::new(-550.0, 490.0)
        ));
    }

    #[test]
    fn menu_is_anchored_without_changing_the_composer_frame() {
        let parent = PanelRect::new(200.0, 600.0, 680.0, 140.0);
        let screen = PanelRect::new(0.0, 0.0, 1440.0, 900.0);
        let anchor = PanelAnchor {
            x: 120.0,
            y: 16.0,
            width: 70.0,
            height: 24.0,
        };
        let frame = git(parent, screen, &anchor, 280.0);
        assert_eq!((frame.x, frame.y), (320.0, 414.0));
        assert_eq!((frame.width, frame.height), (320.0, 280.0));
        assert_eq!(parent.height, 140.0);
    }

    #[test]
    fn menu_flips_and_clamps_at_screen_edges() {
        let parent = PanelRect::new(1000.0, 0.0, 680.0, 140.0);
        let screen = PanelRect::new(0.0, 0.0, 1440.0, 900.0);
        let frame = git(
            parent,
            screen,
            &PanelAnchor {
                x: 500.0,
                y: 16.0,
                width: 70.0,
                height: 24.0,
            },
            300.0,
        );
        assert_eq!((frame.x, frame.y), (1120.0, 130.0));
        let small = PanelRect::new(-800.0, -400.0, 250.0, 200.0);
        let frame = git(
            parent,
            small,
            &PanelAnchor {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            700.0,
        );
        assert_eq!(frame, small);
    }

    #[test]
    fn fitting_keeps_the_top_edge_and_clamps_the_height() {
        let frame = PanelRect::new(100.0, 500.0, 680.0, 128.0);
        let grown = fit_frame(
            frame,
            QUICK_COMPOSER_WIDTH,
            300.0,
            QUICK_COMPOSER_MAX_HEIGHT,
        );
        assert_eq!(grown.top(), frame.top());
        assert_eq!(grown.height, 300.0);
        let capped = fit_frame(
            frame,
            QUICK_COMPOSER_WIDTH,
            900.0,
            QUICK_COMPOSER_MAX_HEIGHT,
        );
        assert_eq!(capped.height, QUICK_COMPOSER_MAX_HEIGHT);
        assert_eq!(capped.top(), frame.top());
        assert_eq!(
            fit_frame(frame, QUICK_COMPOSER_WIDTH, f64::NAN, 520.0),
            frame
        );
        assert_eq!(fit_frame(frame, 680.0, -4.0, 520.0).height, 1.0);
    }

    #[test]
    fn placement_centers_on_the_work_area_near_the_top() {
        let area = PanelRect::new(0.0, 40.0, 1440.0, 860.0);
        let frame = place_frame(
            area,
            QUICK_COMPOSER_WIDTH,
            QUICK_COMPOSER_INITIAL_HEIGHT,
            QUICK_COMPOSER_TOP_FRACTION,
        );
        assert_eq!(frame.x, 380.0);
        assert_eq!(frame.top(), 900.0 - (860.0f64 * 0.22).round());
        assert_eq!(frame.height, QUICK_COMPOSER_INITIAL_HEIGHT);
    }

    #[test]
    fn the_pointer_picks_its_screen() {
        let left = (
            PanelRect::new(0.0, 0.0, 1440.0, 900.0),
            PanelRect::new(0.0, 0.0, 1440.0, 875.0),
        );
        let right = (
            PanelRect::new(1440.0, 0.0, 1920.0, 1080.0),
            PanelRect::new(1440.0, 0.0, 1920.0, 1055.0),
        );
        let screens = [left, right];
        assert_eq!(
            screen_at(&screens, PanelPoint::new(2000.0, 500.0)),
            Some(right.1)
        );
        assert_eq!(
            screen_at(&screens, PanelPoint::new(-50.0, 500.0)),
            Some(left.1)
        );
        assert_eq!(screen_at(&[], PanelPoint::new(0.0, 0.0)), None);
    }
}
