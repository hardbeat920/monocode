//! Plain simulation/presentation values. No GPUI or provider-service types.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    Low,
    #[default]
    Mid,
    Hard,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Cell {
    pub x: i32,
    pub y: i32,
}
impl Cell {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
    pub const fn is_cardinal(self) -> bool {
        matches!((self.x, self.y), (1, 0) | (-1, 0) | (0, 1) | (0, -1))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pickup<'a> {
    pub provider: &'a str,
    pub x: f64,
    pub y: f64,
    pub cells: f64,
    pub alpha: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bubble {
    pub text: &'static str,
    pub x: f64,
    pub y: f64,
    pub alpha: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteKind {
    Pacman,
    Ghost,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    pub kind: SpriteKind,
    pub cx: f64,
    pub cy: f64,
    pub size: f64,
    pub dx: i32,
    pub dy: i32,
    pub alpha: f64,
    pub mouth: f64,
    pub mascot: Option<monocode_mascots::MascotId>,
    pub talk: bool,
    pub eyes: bool,
}
