//! Port of src/features/terminal/arcade/gridArcade.ts: the shared types for
//! the empty-session grid games.
//!
//! Pac-man, snake, and anything that joins later all speak [`GridArcade`] so
//! the idle slider can host them. Each game lives in its own module and plugs
//! into `GRID_GAMES`.
//!
//! The TypeScript games read `Math.random`. Here each game takes an
//! [`ArcadeRng`], so tests can pin the stream the way pacmanArcade.test.ts
//! mocked `Math.random`.

use std::cell::Cell;
use std::rc::Rc;

use monocode_core::HarnessId;

/// `ArcadeMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArcadeMode {
    Low,
    Mid,
    Hard,
}

impl ArcadeMode {
    /// The id the HUD prints on the mode buttons.
    pub const fn label(self) -> &'static str {
        match self {
            ArcadeMode::Low => "low",
            ArcadeMode::Mid => "mid",
            ArcadeMode::Hard => "hard",
        }
    }
}

/// `ARCADE_MODES`.
pub const ARCADE_MODES: [ArcadeMode; 3] = [ArcadeMode::Low, ArcadeMode::Mid, ArcadeMode::Hard];

/// A pixel speech bubble trailing whoever is talking.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechBubble {
    pub text: &'static str,
    /// Grid cell the tail points at.
    pub x: f64,
    pub y: f64,
    pub alpha: f64,
}

/// Where to draw a provider logo in place of the grid squares.
#[derive(Debug, Clone, PartialEq)]
pub struct LogoPickup {
    pub harness: HarnessId,
    /// Top-left grid cell of the pickup.
    pub x: f64,
    pub y: f64,
    /// Footprint in grid cells, square.
    pub cells: f64,
    /// Eases in on arrival and out on expiry.
    pub alpha: f64,
}

/// `ArcadeSprite["kind"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteKind {
    Pacman,
    Ghost,
}

/// `ArcadeSprite["frame"]`: which of a mascot's two frames to paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteFrame {
    Rest,
    Talk,
}

/// A drawn piece on top of the stamped grid: pac-man or a mascot.
#[derive(Debug, Clone, PartialEq)]
pub struct ArcadeSprite {
    pub kind: SpriteKind,
    /// Center of the sprite, in grid cells.
    pub cx: f64,
    pub cy: f64,
    /// Side of the sprite, in grid cells.
    pub size: f64,
    /// Heading, for pac-man's mouth and the mascots' eyes.
    pub dx: f64,
    pub dy: f64,
    pub alpha: f64,
    /// Pac-man only: 0 shut, 1 gone. Chewing, or the death spin.
    pub mouth: f64,
    /// Mascots only.
    pub mascot: Option<&'static str>,
    /// Mascots only.
    pub frame: Option<SpriteFrame>,
    /// Mascots only: eaten ones are drawn as a pair of eyes going home.
    pub eyes: bool,
}

/// One idle-or-playable board on the empty-session grid.
pub trait GridArcade {
    fn resize(&mut self, next_cols: i32, next_rows: i32);
    fn take_control(&mut self);
    fn release_control(&mut self);
    fn steer(&mut self, x: i32, y: i32);
    fn set_mode(&mut self, next: ArcadeMode);
    fn mode(&self) -> ArcadeMode;
    fn controlled(&self) -> bool;
    fn score(&self) -> i64;
    fn lives(&self) -> i64;
    fn game_over(&self) -> bool;
    /// Advance by `dt` milliseconds.
    fn step(&mut self, dt: f64);
    fn speech_bubble(&self) -> Option<SpeechBubble>;
    fn logo_pickup(&self) -> Option<LogoPickup>;
    fn sprites(&self) -> Vec<ArcadeSprite>;
    /// 0 to 1 over the first paint after mount or resize.
    fn fade(&self) -> f64;
    /// Light the background cells, one intensity per cell, row major.
    fn stamp(&self, out: &mut [f32], stamp_cols: usize, stamp_rows: usize);
}

/// The games' `Math.random`: mulberry32, the generator
/// pacmanArcade.test.ts mocked `Math.random` with.
///
/// Clones share one stream, as every game in a test shared the one mocked
/// `Math.random`.
#[derive(Debug, Clone)]
pub struct ArcadeRng {
    state: Rc<Cell<u32>>,
}

impl ArcadeRng {
    /// A stream that always plays out the same way.
    pub fn seeded(seed: u32) -> Self {
        Self {
            state: Rc::new(Cell::new(seed)),
        }
    }

    /// A stream seeded from the clock, for the app.
    pub fn from_time() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        Self::seeded((nanos ^ (nanos >> 32)) as u32)
    }

    /// The next number in `[0, 1)`.
    pub fn next(&self) -> f64 {
        let state = self.state.get().wrapping_add(0x6d2b_79f5);
        self.state.set(state);
        let mut t = (state ^ (state >> 15)).wrapping_mul(1 | state);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        (t ^ (t >> 14)) as f64 / 4_294_967_296.0
    }

    /// `pick`: one of `items` at random.
    pub fn pick<T: Copy>(&self, items: &[T]) -> T {
        items[(self.next() * items.len() as f64).floor() as usize]
    }

    /// `shuffled`: a Fisher-Yates copy of `items`.
    pub fn shuffled<T: Clone>(&self, items: &[T]) -> Vec<T> {
        let mut out = items.to_vec();
        for i in (1..out.len()).rev() {
            let j = (self.next() * (i + 1) as f64).floor() as usize;
            out.swap(i, j);
        }
        out
    }
}

/// `Math.min(max, Math.max(min, value))` over grid integers.
pub(crate) fn clamp_i(value: i32, min: i32, max: i32) -> i32 {
    min.max(max.min(value))
}

/// `Math.sign` over a whole number.
pub(crate) fn sign(value: f64) -> i32 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

/// `light` in both games: raise one cell to `value`, never lower it.
pub(crate) fn light(out: &mut [f32], cols: usize, rows: usize, x: i32, y: i32, value: f64) {
    if x < 0 || y < 0 || x as usize >= cols || y as usize >= rows {
        return;
    }
    let index = y as usize * cols + x as usize;
    if let Some(cell) = out.get_mut(index)
        && f64::from(*cell) < value
    {
        *cell = value as f32;
    }
}

/// The boot fade's last step: scale every cell by `alpha` unless it is
/// already full strength.
pub(crate) fn fade_stamp(out: &mut [f32], alpha: f64) {
    if alpha >= 0.999 {
        return;
    }
    for cell in out.iter_mut() {
        *cell = (f64::from(*cell) * alpha) as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mulberry32_matches_the_javascript_stream() {
        // The first draws of the test's seededRandom(0x5eed) in Node.
        let rng = ArcadeRng::seeded(0x5eed);
        let draws: Vec<f64> = (0..3).map(|_| rng.next()).collect();
        assert_eq!(
            draws,
            vec![0.7100320369936526, 0.286336648510769, 0.9519026265479624]
        );
    }

    #[test]
    fn clones_share_one_stream() {
        let rng = ArcadeRng::seeded(7);
        let other = rng.clone();
        let first = rng.next();
        let second = other.next();
        let fresh = ArcadeRng::seeded(7);
        assert_eq!(first, fresh.next());
        assert_eq!(second, fresh.next());
    }
}
