//! Port of src/features/terminal/arcade/gridGames.ts: the games the
//! empty-session slider rotates through.

use super::grid_arcade::{ArcadeRng, GridArcade};
use super::pacman::PacmanArcade;
use super::snake::SnakeArcade;

/// How long an idle board stays up before the slider moves on, in ms.
pub const SLIDE_HOLD_MS: u64 = 16_000;

/// `GridGame`.
#[derive(Clone, Copy)]
pub struct GridGame {
    pub id: &'static str,
    pub label: &'static str,
    pub play_label: &'static str,
    /// Idle-band brightness against a full game. Maze games fill the grid,
    /// so they run quieter than a sparse one like snake.
    pub idle_dim: f64,
    /// Whether the HUD shows lives.
    pub lives: bool,
    pub create: fn(ArcadeRng) -> Box<dyn GridArcade>,
}

impl std::fmt::Debug for GridGame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GridGame").field("id", &self.id).finish()
    }
}

fn create_pacman(rng: ArcadeRng) -> Box<dyn GridArcade> {
    Box::new(PacmanArcade::new(rng))
}

fn create_snake(rng: ArcadeRng) -> Box<dyn GridArcade> {
    Box::new(SnakeArcade::new(rng))
}

/// Games the empty-session slider rotates through. Add an entry to add a
/// game: the track, the dots, and the take-control path all follow this
/// list.
pub const GRID_GAMES: [GridGame; 2] = [
    GridGame {
        id: "pacman",
        label: "pac-man",
        play_label: "Pac-man. Arrow keys or WASD to move. Escape to release.",
        idle_dim: 0.2,
        lives: true,
        create: create_pacman,
    },
    GridGame {
        id: "snake",
        label: "snake",
        play_label: "Snake. Arrow keys or WASD to move. Escape to release.",
        idle_dim: 0.65,
        lives: false,
        create: create_snake,
    },
];

/// `stepSlider`: the next stop on the idle slider. Walks to the end and
/// back so a wrap never jumps the track the long way around. `dir` is 1 or
/// -1.
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

#[cfg(test)]
mod tests {
    //! Port of gridGames.test.ts.

    use std::collections::HashSet;

    use super::*;

    #[test]
    fn lists_each_game_once_under_a_stable_id() {
        let ids: Vec<&str> = GRID_GAMES.iter().map(|game| game.id).collect();
        assert!(ids.len() >= 2);
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), ids.len());
        assert!(ids.contains(&"pacman"));
        assert!(ids.contains(&"snake"));
    }

    #[test]
    fn walks_to_the_end_and_back_instead_of_wrapping() {
        let count = 3;
        let mut steps = vec![0];
        let (mut index, mut dir) = (0, 1);
        for _ in 0..6 {
            (index, dir) = step_slider(index, dir, count);
            steps.push(index);
        }
        assert_eq!(steps, vec![0, 1, 2, 1, 0, 1, 2]);
    }

    #[test]
    fn pings_between_two_games() {
        let (mut index, mut dir) = (0, 1);
        let mut seen = Vec::new();
        for _ in 0..4 {
            (index, dir) = step_slider(index, dir, 2);
            seen.push(index);
        }
        assert_eq!(seen, vec![1, 0, 1, 0]);
    }

    #[test]
    fn stays_put_when_there_is_only_one_game() {
        assert_eq!(step_slider(0, 1, 1), (0, 1));
        assert_eq!(step_slider(0, -1, 0), (0, 1));
    }

    #[test]
    fn boots_every_catalogued_game_onto_a_board() {
        for game in GRID_GAMES {
            let mut arcade = (game.create)(ArcadeRng::seeded(0x5eed));
            arcade.resize(172, 28);
            arcade.step(33.0);
            assert!(arcade.fade() > 0.0);
            assert!(!arcade.controlled());
        }
    }
}
