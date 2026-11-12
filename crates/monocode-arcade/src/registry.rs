//! The two built-ins, in the legacy catalog order. Not a plugin system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(usize)]
pub enum GameId {
    #[default]
    Pacman,
    Snake,
}
impl GameId {
    pub const fn index(self) -> usize {
        self as usize
    }
    pub fn descriptor(self) -> &'static Descriptor {
        &GAMES[self.index()]
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Descriptor {
    pub id: GameId,
    pub label: &'static str,
    pub play_label: &'static str,
    pub idle_dim: f64,
    pub lives: bool,
}
pub const GAMES: [Descriptor; 2] = [
    Descriptor {
        id: GameId::Pacman,
        label: "pac-man",
        play_label: "Pac-man. Arrow keys or WASD to move. Escape to release.",
        idle_dim: 0.2,
        lives: true,
    },
    Descriptor {
        id: GameId::Snake,
        label: "snake",
        play_label: "Snake. Arrow keys or WASD to move. Escape to release.",
        idle_dim: 0.65,
        lives: false,
    },
];
pub const SLIDE_HOLD_MS: u64 = 16000;
pub fn step_slider(index: usize, dir: i32, count: usize) -> (usize, i32) {
    if count <= 1 {
        return (0, 1);
    }
    let next = index as i64 + i64::from(dir);
    if next >= count as i64 {
        return (count - 2, -1);
    }
    if next < 0 {
        return (1, 1);
    }
    (next as usize, dir)
}
