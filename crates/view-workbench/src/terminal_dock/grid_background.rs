//! Port of src/features/terminal/ui/TerminalGridBackground.tsx: the grid of
//! squares behind the empty-session composer, where pac-man and snake play
//! themselves until someone takes control.
//!
//! The TypeScript drew one canvas per game on `requestAnimationFrame`. Here a
//! 33 ms timer steps the boards and each frame paints the grid as GPUI
//! quads: one 6px square per cell with a faint 1px border, filled where the
//! game stamped it. Sprites, the logo pickup, and the speech bubble draw on
//! top.
//!
//! Idle, the view is a 192px band across the top of its parent, faded out
//! toward the bottom. `mask-image` has no GPUI equivalent, so each cell,
//! sprite, logo, and bubble takes the linear mask's alpha at its own height.
//! Playing, the view covers its parent and draws over later siblings
//! through `deferred`, as `z-20` did.
//!
//! Reduced motion: the slide jumps instead of easing and the cursor block
//! stops pulsing, as the CSS `motion-reduce` and `motion-safe` variants did.
//! The idle boards also hold one still frame instead of playing themselves.
//! A game someone takes control of still runs.

use std::cell::Cell;
use std::f32::consts::PI;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, BorderStyle, Bounds, Context, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement as _,
    PathBuilder, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    Window, canvas, deferred, div, img, point, px, quad, relative, size, svg,
};
use monocode_ui::styled::glass_backdrop;
use monocode_ui::theme::CubicBezier;
use monocode_ui::{ProviderLogo, Theme, UiStyled as _, u};

use super::arcade::{
    ARCADE_MODES, ArcadeMode, ArcadeRng, ArcadeSprite, GRID_GAMES, GridArcade, GridGame,
    LogoPickup, SLIDE_HOLD_MS, SpriteFrame, SpriteKind, step_slider,
};
use crate::panes::pixel_art::{MASCOT_GRID, PROJECT_MASCOTS, pixel_rects};
use crate::panes::speech_bubble::{BubbleTheme, draw_speech_bubble};

/// One grid square, in CSS px.
pub const CELL: f32 = 6.0;
pub const GAP: f32 = 1.0;
/// Cell plus gap: the grid's step.
pub const PITCH: f32 = CELL + GAP;
const BORDER_OPACITY: f32 = 0.06;
const PEAK_OPACITY: f32 = 0.72;
const LOGO_OPACITY: f32 = 0.7;
const BUBBLE_OPACITY: f32 = 0.9;
const PAC_OPACITY: f32 = 0.92;
const GHOST_OPACITY: f32 = 0.8;
/// Sprites sit inside their corridor rather than spilling over the walls.
const SPRITE_SCALE: f32 = 0.8;
/// How hard the bubble chases its speaker. Sprites move cell by cell.
const BUBBLE_EASE: f32 = 0.2;
/// The frame step, in ms.
pub const FRAME_MS: f64 = 33.0;

/// The idle band's height: `h-48`.
pub const BAND_HEIGHT: f32 = 192.0;
/// The idle mask: solid to 65% of the band, transparent at the bottom.
const MASK_SOLID: f32 = 0.65;
/// `duration-700 ease-in-out` on the slider track.
const SLIDE_MS: f32 = 700.0;
/// `transition-opacity duration-200` on the take-control button.
const HOVER_FADE_MS: f32 = 200.0;
/// Tailwind's `ease-in-out`, also its default transition curve.
const EASE_IN_OUT: CubicBezier = CubicBezier(0.4, 0.0, 0.2, 1.0);
/// `animate-pulse`: 2s, opacity down to 0.5 at the halfway mark.
const PULSE_MS: f32 = 2000.0;
const PULSE_EASE: CubicBezier = CubicBezier(0.4, 0.0, 0.6, 1.0);

/// `HEADING`: arrow keys and WASD.
fn heading(key: &str) -> Option<(i32, i32)> {
    match key {
        "up" | "w" => Some((0, -1)),
        "down" | "s" => Some((0, 1)),
        "left" | "a" => Some((-1, 0)),
        "right" | "d" => Some((1, 0)),
        _ => None,
    }
}

struct Board {
    game: GridGame,
    arcade: Box<dyn GridArcade>,
    /// Where the bubble is drawn, in CSS px, easing after its speaker.
    bubble_at: Option<(f32, f32)>,
}

#[derive(Clone, Copy)]
struct Slide {
    index: usize,
    dir: i32,
}

/// The slide in flight: where the track started and when.
#[derive(Clone, Copy)]
struct SlideMotion {
    from: f32,
    started: Instant,
}

/// The empty-session arcade.
pub struct TerminalGridBackground {
    boards: Vec<Board>,
    cols: i32,
    rows: i32,
    /// The view's size in CSS px, as the measuring canvas last saw it.
    measured: Rc<Cell<Option<(f32, f32)>>>,
    size: Option<(f32, f32)>,
    playing: bool,
    score: i64,
    lives: i64,
    mode: ArcadeMode,
    slide: Slide,
    motion: Option<SlideMotion>,
    hovered: bool,
    hover_changed: Option<Instant>,
    /// ms since the slider last moved or its interval restarted.
    slide_clock: f64,
    last_frame: Option<Instant>,
    /// Reduced motion stepped the idle boards to their still frame.
    still: bool,
    epoch: Instant,
    focus: FocusHandle,
    _ticker: Task<()>,
}

impl Focusable for TerminalGridBackground {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TerminalGridBackground {
    /// A background whose games draw from the clock-seeded stream.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::with_rng(ArcadeRng::from_time(), cx)
    }

    /// A background whose games share `rng`, for tests and screenshots.
    pub fn with_rng(rng: ArcadeRng, cx: &mut Context<Self>) -> Self {
        let boards = GRID_GAMES
            .iter()
            .map(|game| Board {
                game: *game,
                arcade: (game.create)(rng.clone()),
                bubble_at: None,
            })
            .collect();
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(FRAME_MS as u64))
                    .await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        });
        let now = cx.background_executor().now();
        Self {
            boards,
            cols: 0,
            rows: 0,
            measured: Rc::default(),
            size: None,
            playing: false,
            score: 0,
            lives: 3,
            mode: ArcadeMode::Mid,
            slide: Slide { index: 0, dir: 1 },
            motion: None,
            hovered: false,
            hover_changed: None,
            slide_clock: 0.0,
            last_frame: None,
            still: false,
            epoch: now,
            focus: cx.focus_handle(),
            _ticker: ticker,
        }
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    /// The game on show.
    pub fn slide_index(&self) -> usize {
        self.slide.index
    }

    pub fn score(&self) -> i64 {
        self.score
    }

    pub fn lives(&self) -> i64 {
        self.lives
    }

    pub fn mode(&self) -> ArcadeMode {
        self.mode
    }

    /// The grid in cells, once the view has a size.
    pub fn grid(&self) -> (i32, i32) {
        (self.cols, self.rows)
    }

    fn game(&self) -> GridGame {
        GRID_GAMES
            .get(self.slide.index)
            .copied()
            .unwrap_or(GRID_GAMES[0])
    }

    fn now(&self, cx: &App) -> Instant {
        cx.background_executor().now()
    }

    /// `layout`: size every board to the view.
    fn layout(&mut self, width: f32, height: f32) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let next_cols = (width / PITCH).ceil() as i32;
        let next_rows = (height / PITCH).ceil() as i32;
        for board in &mut self.boards {
            board.arcade.resize(next_cols, next_rows);
        }
        self.cols = next_cols;
        self.rows = next_rows;
        self.size = Some((width, height));
        self.still = false;
    }

    /// One frame: size, slide, step, and the HUD numbers.
    fn tick(&mut self, cx: &mut Context<Self>) {
        let now = self.now(cx);
        let dt = match self.last_frame {
            Some(last) => now.saturating_duration_since(last).as_secs_f64() * 1000.0,
            None => FRAME_MS,
        };
        self.last_frame = Some(now);
        if self.advance(dt, cx.reduce_motion(), now) {
            cx.notify();
        }
    }

    /// Returns whether anything on screen changed.
    fn advance(&mut self, dt: f64, reduce_motion: bool, now: Instant) -> bool {
        if let Some(measured) = self.measured.get()
            && Some(measured) != self.size
        {
            self.layout(measured.0, measured.1);
        }
        let Some((width, _)) = self.size else {
            return false;
        };
        let slide = self.slide.index;

        // The slider holds each idle board for SLIDE_HOLD_MS, unless a game
        // is on or the pointer rests on the band.
        if !self.playing && !self.hovered && GRID_GAMES.len() >= 2 {
            self.slide_clock += dt;
            if self.slide_clock >= SLIDE_HOLD_MS as f64 {
                let (index, dir) = step_slider(self.slide.index, self.slide.dir, GRID_GAMES.len());
                self.set_slide(index, dir, reduce_motion, now);
            }
        }
        if let Some(motion) = self.motion
            && now.saturating_duration_since(motion.started).as_secs_f32() * 1000.0 >= SLIDE_MS
        {
            self.motion = None;
        }

        let current = self.slide.index;
        let controlled = self.playing;
        if reduce_motion && !controlled {
            // Hold one frame, past the boot fade.
            if self.still {
                return self.slide.index != slide;
            }
            for board in &mut self.boards {
                board.arcade.step(500.0);
                ease_bubble(board, width, true);
            }
            self.still = true;
            return true;
        }
        self.still = false;

        for (index, board) in self.boards.iter_mut().enumerate() {
            // A live game only ticks the board you are on. Idle keeps the
            // neighbors moving so a slide does not reveal a frozen frame.
            if controlled && index != current {
                continue;
            }
            board.arcade.step(dt);
            ease_bubble(board, width, false);
        }

        if controlled && let Some(active) = self.boards.get(current) {
            self.score = active.arcade.score();
            self.lives = active.arcade.lives();
        }
        true
    }

    /// Steps every board `ms` forward in frames, as the timer would.
    /// Screenshots use it to show a game in progress.
    pub fn fast_forward(&mut self, ms: f64, cx: &mut Context<Self>) {
        let mut left = ms;
        let now = self.now(cx);
        while left > 0.0 {
            let dt = left.min(FRAME_MS);
            self.advance(dt, false, now);
            left -= dt;
        }
        cx.notify();
    }

    fn set_slide(&mut self, index: usize, dir: i32, reduce_motion: bool, now: Instant) {
        if !reduce_motion && !self.playing {
            self.motion = Some(SlideMotion {
                from: self.track_position(now),
                started: now,
            });
        }
        self.slide = Slide { index, dir };
        self.slide_clock = 0.0;
    }

    /// Where the track sits, in boards: the slide index, or between two
    /// while a slide eases.
    fn track_position(&self, now: Instant) -> f32 {
        let to = self.slide.index as f32;
        let Some(motion) = self.motion else {
            return to;
        };
        let t = now.saturating_duration_since(motion.started).as_secs_f32() * 1000.0 / SLIDE_MS;
        motion.from + (to - motion.from) * EASE_IN_OUT.ease(t)
    }

    /// `takeControl`: hand the board on show to the keyboard.
    pub fn take_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.mode;
        if let Some(board) = self.boards.get_mut(self.slide.index) {
            board.arcade.set_mode(mode);
            board.arcade.take_control();
            self.lives = board.arcade.lives();
        }
        self.score = 0;
        self.playing = true;
        self.motion = None;
        self.slide_clock = 0.0;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// `pickMode`.
    pub fn pick_mode(&mut self, next: ArcadeMode, cx: &mut Context<Self>) {
        if let Some(board) = self.boards.get_mut(self.slide.index) {
            board.arcade.set_mode(next);
        }
        self.mode = next;
        cx.notify();
    }

    /// `releaseControl`: the idle brain takes the board back.
    pub fn release_control(&mut self, cx: &mut Context<Self>) {
        if let Some(board) = self.boards.get_mut(self.slide.index) {
            board.arcade.release_control();
        }
        self.score = 0;
        self.playing = false;
        self.slide_clock = 0.0;
        self.still = false;
        cx.notify();
    }

    /// `showGame`: slide to another game.
    pub fn show_game(&mut self, next: usize, cx: &mut Context<Self>) {
        if next == self.slide.index || next >= GRID_GAMES.len() {
            return;
        }
        let dir = if next > self.slide.index { 1 } else { -1 };
        let now = self.now(cx);
        self.set_slide(next, dir, cx.reduce_motion(), now);
        cx.notify();
    }

    /// Steer the game under control: what the arrow keys and WASD do.
    pub fn steer(&mut self, x: i32, y: i32) {
        if let Some(board) = self.boards.get_mut(self.slide.index) {
            board.arcade.steer(x, y);
        }
    }

    fn set_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.hovered == hovered {
            return;
        }
        self.hovered = hovered;
        self.hover_changed = Some(self.now(cx));
        self.slide_clock = 0.0;
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if !self.playing {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }
        let key = event.keystroke.key.to_lowercase();
        if key == "escape" {
            cx.stop_propagation();
            self.release_control(cx);
            return;
        }
        let Some((x, y)) = heading(&key) else {
            return;
        };
        cx.stop_propagation();
        self.steer(x, y);
    }

    /// The take-control button's opacity: it fades in over 200 ms on hover.
    fn hover_opacity(&self, now: Instant) -> f32 {
        let t = self.hover_changed.map_or(1.0, |at| {
            (now.saturating_duration_since(at).as_secs_f32() * 1000.0 / HOVER_FADE_MS).min(1.0)
        });
        let eased = EASE_IN_OUT.ease(t);
        if self.hovered { eased } else { 1.0 - eased }
    }

    /// What each visible board paints this frame.
    fn frames(&self, position: f32) -> Vec<(usize, BoardFrame)> {
        let (cols, rows) = (self.cols.max(0) as usize, self.rows.max(0) as usize);
        self.boards
            .iter()
            .enumerate()
            .filter(|(index, _)| (*index as f32 - position).abs() < 1.0)
            .map(|(index, board)| {
                let mut stamp = vec![0f32; cols * rows];
                board.arcade.stamp(&mut stamp, cols, rows);
                let dim = if board.arcade.controlled() {
                    1.0
                } else {
                    board.game.idle_dim as f32
                };
                let bubble = board.arcade.speech_bubble().and_then(|bubble| {
                    let (x, y) = board.bubble_at?;
                    Some(BubbleFrame {
                        text: bubble.text,
                        x,
                        y,
                        alpha: bubble.alpha as f32,
                    })
                });
                (
                    index,
                    BoardFrame {
                        cols,
                        rows,
                        stamp,
                        fade: board.arcade.fade() as f32,
                        dim,
                        sprites: board.arcade.sprites(),
                        bubble,
                        logo: board.arcade.logo_pickup(),
                    },
                )
            })
            .collect()
    }
}

/// Ease the bubble toward its speaker, or snap when the speaker wraps
/// through a tunnel so it does not sail across the board to catch up.
fn ease_bubble(board: &mut Board, width: f32, snap: bool) {
    let Some(bubble) = board.arcade.speech_bubble() else {
        board.bubble_at = None;
        return;
    };
    let target = (bubble.x as f32 * PITCH, bubble.y as f32 * PITCH);
    board.bubble_at = match board.bubble_at {
        Some(at) if !snap && (target.0 - at.0).abs() <= width / 3.0 => Some((
            at.0 + (target.0 - at.0) * BUBBLE_EASE,
            at.1 + (target.1 - at.1) * BUBBLE_EASE,
        )),
        _ => Some(target),
    };
}

struct BubbleFrame {
    text: &'static str,
    x: f32,
    y: f32,
    alpha: f32,
}

/// One board's frame, owned so the paint closures can keep it.
struct BoardFrame {
    cols: usize,
    rows: usize,
    stamp: Vec<f32>,
    fade: f32,
    dim: f32,
    sprites: Vec<ArcadeSprite>,
    bubble: Option<BubbleFrame>,
    logo: Option<LogoPickup>,
}

/// The idle band's mask at `y` CSS px from the top: solid to 65% of the
/// band, then a linear fade to nothing at the bottom edge.
#[derive(Clone, Copy)]
struct Mask {
    height: Option<f32>,
}

impl Mask {
    fn at(self, y: f32) -> f32 {
        let Some(height) = self.height else {
            return 1.0;
        };
        let band = height * (1.0 - MASK_SOLID);
        ((height - y) / band).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Copy)]
struct Ink {
    content: Hsla,
    surface: Hsla,
    /// CSS px to window px.
    scale: f32,
}

impl Ink {
    fn alpha(&self, alpha: f32) -> Hsla {
        Hsla {
            a: alpha.clamp(0.0, 1.0),
            ..self.content
        }
    }
}

/// The grid: every cell's border, and a fill where the game stamped it.
fn paint_grid(
    frame: &BoardFrame,
    origin: gpui::Point<Pixels>,
    ink: Ink,
    mask: Mask,
    window: &mut Window,
) {
    let s = ink.scale;
    let border = BORDER_OPACITY * frame.fade;
    let peak = PEAK_OPACITY * frame.dim;
    let transparent = Hsla::transparent_black();
    for y in 0..frame.rows {
        let py = y as f32 * PITCH;
        let fade = mask.at(py + CELL / 2.0);
        if fade <= 0.0 {
            continue;
        }
        for x in 0..frame.cols {
            let px_ = x as f32 * PITCH;
            let fill_opacity = frame.stamp.get(y * frame.cols + x).copied().unwrap_or(0.0) * peak;
            let background = if fill_opacity > 0.02 {
                ink.alpha(fill_opacity * fade)
            } else {
                transparent
            };
            window.paint_quad(quad(
                Bounds::new(
                    point(origin.x + px(px_ * s), origin.y + px(py * s)),
                    size(px(CELL * s), px(CELL * s)),
                ),
                px(0.),
                background,
                px(s),
                ink.alpha(border * fade),
                BorderStyle::Solid,
            ));
        }
    }
}

/// `drawPacman`: a wedge-mouthed circle facing its heading. `mouth` 1 closes
/// it out entirely.
fn paint_pacman(
    sprite: &ArcadeSprite,
    origin: gpui::Point<Pixels>,
    fill: Hsla,
    scale: f32,
    window: &mut Window,
) {
    let cx = (sprite.cx as f32 * PITCH + CELL / 2.0) * scale;
    let cy = (sprite.cy as f32 * PITCH + CELL / 2.0) * scale;
    let radius = sprite.size as f32 * PITCH * SPRITE_SCALE / 2.0 * scale;
    let facing = (sprite.dy as f32).atan2(sprite.dx as f32);
    let mouth = sprite.mouth as f32 * PI;
    let center = point(origin.x + px(cx), origin.y + px(cy));

    if mouth <= 0.01 {
        window.paint_quad(quad(
            Bounds::new(
                point(center.x - px(radius), center.y - px(radius)),
                size(px(radius * 2.0), px(radius * 2.0)),
            ),
            px(radius),
            fill,
            px(0.),
            Hsla::transparent_black(),
            BorderStyle::Solid,
        ));
        return;
    }
    let start = facing + mouth;
    let end = facing - mouth + PI * 2.0;
    if end <= start {
        return;
    }
    let steps = ((end - start) / (PI * 2.0) * 48.0).ceil().max(2.0) as usize;
    let mut path = PathBuilder::fill();
    path.move_to(center);
    for step in 0..=steps {
        let angle = start + (end - start) * step as f32 / steps as f32;
        path.line_to(point(
            center.x + px(radius * angle.cos()),
            center.y + px(radius * angle.sin()),
        ));
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, fill);
    }
}

/// `Math.sign`.
fn sign(value: f64) -> f32 {
    if value > 0.0 {
        1.0
    } else if value < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// `drawGhost`: the mascot's own pixel art, scaled up out of its 8 by 8 box.
fn paint_ghost(
    sprite: &ArcadeSprite,
    origin: gpui::Point<Pixels>,
    fill: Hsla,
    scale: f32,
    window: &mut Window,
) {
    let span = sprite.size as f32 * PITCH * SPRITE_SCALE;
    let left = sprite.cx as f32 * PITCH + CELL / 2.0 - span / 2.0;
    let top = sprite.cy as f32 * PITCH + CELL / 2.0 - span / 2.0;
    let unit = span / MASCOT_GRID as f32;
    let rect = |x: f32, y: f32, w: f32, h: f32| {
        Bounds::new(
            point(origin.x + px(x * scale), origin.y + px(y * scale)),
            size(px(w * scale), px(h * scale)),
        )
    };

    if sprite.eyes {
        // Eaten: only the eyes float home, leaning the way they are headed.
        let lean = (sign(sprite.dx) * unit * 0.6, sign(sprite.dy) * unit * 0.6);
        for offset in [2.0, 5.0] {
            window.paint_quad(gpui::fill(
                rect(
                    left + offset * unit + lean.0,
                    top + 3.0 * unit + lean.1,
                    unit,
                    unit * 1.5,
                ),
                fill,
            ));
        }
        return;
    }

    let Some(mascot) = sprite
        .mascot
        .and_then(|name| PROJECT_MASCOTS.iter().find(|mascot| mascot.name == name))
    else {
        return;
    };
    let rows = if sprite.frame == Some(SpriteFrame::Talk) {
        &mascot.talk
    } else {
        &mascot.rest
    };
    for (x, y, width) in pixel_rects(rows) {
        window.paint_quad(gpui::fill(
            rect(
                left + x as f32 * unit,
                top + y as f32 * unit,
                width as f32 * unit,
                unit,
            ),
            fill,
        ));
    }
}

fn paint_sprites(
    frame: &BoardFrame,
    bounds: Bounds<Pixels>,
    ink: Ink,
    mask: Mask,
    window: &mut Window,
    cx: &mut App,
) {
    for sprite in &frame.sprites {
        let base = if sprite.kind == SpriteKind::Pacman {
            PAC_OPACITY
        } else {
            GHOST_OPACITY
        };
        let center_y = sprite.cy as f32 * PITCH + CELL / 2.0;
        let opacity = sprite.alpha as f32 * frame.dim * base;
        if opacity <= 0.02 {
            continue;
        }
        let fill = ink.alpha(opacity * mask.at(center_y));
        match sprite.kind {
            SpriteKind::Pacman => paint_pacman(sprite, bounds.origin, fill, ink.scale, window),
            SpriteKind::Ghost => paint_ghost(sprite, bounds.origin, fill, ink.scale, window),
        }
    }

    if let Some(bubble) = &frame.bubble {
        draw_speech_bubble(
            bubble.text,
            bubble.x * ink.scale,
            bubble.y * ink.scale,
            CELL * ink.scale,
            bounds,
            bubble.alpha * BUBBLE_OPACITY * frame.dim * mask.at(bubble.y),
            BubbleTheme {
                fg: Hsla {
                    a: 1.0,
                    ..ink.content
                },
                bg: Hsla {
                    a: 1.0,
                    ..ink.surface
                },
            },
            window,
            cx,
        );
    }
}

/// The provider logo in place of the grid squares under it.
fn logo_element(pickup: &LogoPickup, dim: f32, mask: Mask, content: Hsla) -> Option<AnyElement> {
    let logo = ProviderLogo::from_id(pickup.harness.as_str())?;
    let span = pickup.cells as f32 * PITCH - GAP;
    let left = pickup.x as f32 * PITCH;
    let top = pickup.y as f32 * PITCH;
    let opacity = pickup.alpha as f32 * LOGO_OPACITY * dim * mask.at(top + span / 2.0);
    let mark: AnyElement = if logo.is_monochrome() {
        svg()
            .path(logo.path())
            .size_full()
            .text_color(Hsla { a: 1.0, ..content })
            .into_any_element()
    } else {
        img(logo.path()).size_full().into_any_element()
    };
    Some(
        div()
            .absolute()
            .left(u(left))
            .top(u(top))
            .size(u(span))
            .opacity(opacity)
            .child(mark)
            .into_any_element(),
    )
}

fn board_element(
    index: usize,
    frame: BoardFrame,
    position: f32,
    ink: Ink,
    mask: Mask,
) -> AnyElement {
    let logo = frame
        .logo
        .as_ref()
        .and_then(|pickup| logo_element(pickup, frame.dim, mask, ink.content));
    let frame = Rc::new(frame);
    let grid = frame.clone();
    let sprites = frame;
    div()
        .absolute()
        .top_0()
        .left(relative(index as f32 - position))
        .w_full()
        .h_full()
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| paint_grid(&grid, bounds.origin, ink, mask, window),
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .children(logo)
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, cx| paint_sprites(&sprites, bounds, ink, mask, window, cx),
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .into_any_element()
}

/// Text with CSS `letter-spacing`, which GPUI text lacks: one box per
/// character, each followed by `tracking` px.
fn tracked(text: &str, tracking: f32) -> gpui::Div {
    div().flex().flex_none().children(text.chars().map(|ch| {
        div()
            .flex_none()
            .mr(u(tracking))
            .child(SharedString::from(ch.to_string()))
    }))
}

impl TerminalGridBackground {
    fn render_take_control(
        &self,
        theme: &Theme,
        opacity: f32,
        pulse: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tracking = 11.0 * 0.16;
        let game = self.game();
        let mut button = div()
            .id("arcade-take-control")
            .debug_selector(|| "arcade-take-control".into())
            .relative()
            .flex()
            .items_center()
            .gap(u(8.))
            .border_1()
            .border_color(theme.content(0.25))
            .px(u(12.))
            .py(u(6.))
            .font_family(theme.fonts.mono.clone())
            .text_px(11.)
            .text_color(theme.content(0.85))
            .shadow_lg()
            .child(glass_backdrop(
                0.0,
                4.0,
                Hsla {
                    a: 0.8,
                    ..theme.colors.background_base
                },
            ))
            .child(
                tracked("[", tracking)
                    .relative()
                    .text_color(theme.content(0.40)),
            )
            .child(tracked("take control", tracking).relative())
            .child(
                tracked("·", tracking)
                    .relative()
                    .text_color(theme.content(0.25)),
            )
            .child(tracked(game.label, tracking).relative())
            .child(
                div()
                    .relative()
                    .flex_none()
                    .h(u(12.))
                    .w(u(6.))
                    .bg(theme.content(0.75 * pulse)),
            )
            .child(
                tracked("]", tracking)
                    .relative()
                    .text_color(theme.content(0.40)),
            );
        if self.hovered {
            let hover_fill = theme.content(0.10);
            let hover_border = theme.content(0.45);
            let hover_ink = theme.colors.content;
            button = button
                .cursor_pointer()
                .hover(move |style| {
                    style
                        .border_color(hover_border)
                        .bg(hover_fill)
                        .text_color(hover_ink)
                })
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(|this, _, window, cx| this.take_control(window, cx)));
        }
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .opacity(opacity)
            .child(button)
            .into_any_element()
    }

    fn render_dots(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let dots = GRID_GAMES.iter().enumerate().map(|(index, game)| {
            let on = index == self.slide.index;
            let mut dot = div()
                .size(u(6.))
                .bg(theme.content(if on { 0.45 } else { 0.15 }));
            if !on {
                let hover = theme.content(0.30);
                dot = dot.group_hover("arcade-dot", move |style| style.bg(hover));
            }
            let selector = format!("arcade-dot-{}", game.id);
            div()
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .group("arcade-dot")
                .size(u(16.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(move |this, _, _, cx| this.show_game(index, cx)))
                .child(dot)
        });
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(u(6.))
            .flex()
            .justify_center()
            .children(dots)
            .into_any_element()
    }

    fn render_hud(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let tracking = 11.0 * 0.14;
        let game = self.game();
        let mut score = div()
            .flex_1()
            .flex()
            .items_center()
            .child(tracked(&format!("score {}", self.score), tracking));
        if game.lives {
            let lives = if self.lives > 0 {
                "•".repeat(self.lives as usize)
            } else {
                "game over".to_string()
            };
            score = score.child(
                tracked(&lives, tracking)
                    .ml(u(12.))
                    .text_color(theme.content(0.35)),
            );
        }

        let modes = ARCADE_MODES.iter().map(|&mode| {
            let on = self.mode == mode;
            let selector = format!("arcade-mode-{}", mode.label());
            let mut button = div()
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .cursor_pointer()
                .border_1()
                .px(u(8.))
                .py(u(4.))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(move |this, _, _, cx| this.pick_mode(mode, cx)))
                .child(tracked(mode.label(), tracking));
            if on {
                button = button
                    .border_color(theme.content(0.40))
                    .bg(theme.content(0.10))
                    .text_color(theme.colors.content);
            } else {
                let hover_border = theme.content(0.25);
                let hover_ink = theme.content(0.70);
                button = button
                    .border_color(theme.content(0.10))
                    .text_color(theme.content(0.40))
                    .hover(move |style| style.border_color(hover_border).text_color(hover_ink));
            }
            button
        });

        let hover_border = theme.content(0.40);
        let hover_ink = theme.colors.content;
        let release = div()
            .id("arcade-release")
            .flex()
            .cursor_pointer()
            .border_1()
            .border_color(theme.content(0.20))
            .bg(Hsla {
                a: 0.7,
                ..theme.colors.background_base
            })
            .px(u(8.))
            .py(u(4.))
            .text_color(theme.content(0.70))
            .hover(move |style| style.border_color(hover_border).text_color(hover_ink))
            .on_click(cx.listener(|this, _, _, cx| this.release_control(cx)))
            .child(tracked("[", tracking).text_color(theme.content(0.35)))
            .child(tracked(" release ", tracking))
            .child(tracked("]", tracking).text_color(theme.content(0.35)));

        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .flex()
            .items_center()
            .px(u(12.))
            .py(u(8.))
            .font_family(theme.fonts.mono.clone())
            .text_px(11.)
            .text_color(theme.content(0.50))
            .child(score)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_center()
                    .gap(u(4.))
                    .children(modes),
            )
            .child(div().flex_1().flex().justify_end().child(release))
            .into_any_element()
    }
}

impl Render for TerminalGridBackground {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let now = self.now(cx);
        let reduce_motion = cx.reduce_motion();
        let ink = Ink {
            content: theme.colors.content,
            surface: theme.colors.background_base,
            scale: f32::from(window.rem_size()) / 16.0,
        };
        let position = if self.playing {
            self.slide.index as f32
        } else {
            self.track_position(now)
        };
        let hover_fading = self.hover_changed.is_some_and(|at| {
            now.saturating_duration_since(at).as_secs_f32() * 1000.0 < HOVER_FADE_MS
        });
        if self.motion.is_some() || hover_fading {
            window.request_animation_frame();
        }
        let mask = Mask {
            height: (!self.playing).then_some(BAND_HEIGHT),
        };

        let measured = self.measured.clone();
        let scale = ink.scale;
        let measure = canvas(
            move |bounds, _, _| {
                measured.set(Some((
                    f32::from(bounds.size.width) / scale,
                    f32::from(bounds.size.height) / scale,
                )));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        let track = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .overflow_hidden()
            .children(
                self.frames(position)
                    .into_iter()
                    .map(|(index, frame)| board_element(index, frame, position, ink, mask)),
            );

        let root = div().relative().size_full();
        if self.playing {
            let layer = div()
                .id("arcade-playing")
                .debug_selector(|| "arcade-playing".into())
                .key_context("Arcade")
                .track_focus(&self.focus)
                .on_key_down(
                    cx.listener(|this, event: &KeyDownEvent, _, cx| this.on_key(event, cx)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
                )
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .overflow_hidden()
                .bg(theme.colors.background_base)
                .child(measure)
                .child(track)
                .child(self.render_hud(&theme, cx));
            return root.child(deferred(layer).with_priority(1));
        }

        let pulse = if reduce_motion {
            1.0
        } else {
            let t = (now.saturating_duration_since(self.epoch).as_secs_f32() * 1000.0) % PULSE_MS
                / PULSE_MS;
            // Opacity 1 to 0.5 and back, eased each way.
            let half = if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 };
            1.0 - 0.5 * PULSE_EASE.ease(half)
        };
        let mut band = div()
            .id("arcade-band")
            .debug_selector(|| "arcade-band".into())
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(u(BAND_HEIGHT))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.set_hovered(*hovered, cx)))
            .child(measure)
            .child(track);
        let opacity = self.hover_opacity(now);
        if opacity > 0.0 {
            band = band.child(self.render_take_control(&theme, opacity, pulse, cx));
        }
        if GRID_GAMES.len() > 1 {
            band = band.child(self.render_dots(&theme, cx));
        }
        root.child(band)
    }
}

#[cfg(test)]
#[path = "grid_background_tests.rs"]
mod tests;
