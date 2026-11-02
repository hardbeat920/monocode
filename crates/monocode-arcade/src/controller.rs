//! Native wrapper ordering and shared randomness, independent of UI/scheduling.
//! Only the selected board steps; switching first releases the previous board.
use crate::{
    pacman::Pacman,
    random::Mulberry32,
    registry::GameId,
    snake::Snake,
    types::{Bubble, Cell, Mode, Pickup, Sprite},
};

enum Board {
    Pacman(Box<Pacman>),
    Snake(Box<Snake>),
}
impl Board {
    fn resize(&mut self, cols: i32, rows: i32, random: &mut dyn crate::random::Random) {
        match self {
            Self::Pacman(g) => g.resize(cols, rows, random),
            Self::Snake(g) => g.resize(cols, rows, random),
        }
    }
    fn take_control(&mut self, random: &mut dyn crate::random::Random) {
        match self {
            Self::Pacman(g) => g.take_control(random),
            Self::Snake(g) => g.take_control(random),
        }
    }
    fn release_control(&mut self, random: &mut dyn crate::random::Random) {
        match self {
            Self::Pacman(g) => g.release_control(random),
            Self::Snake(g) => g.release_control(random),
        }
    }
    fn steer(&mut self, x: i32, y: i32) {
        match self {
            Self::Pacman(g) => g.steer(x, y),
            Self::Snake(g) => g.steer(x, y),
        }
    }
    fn set_mode(&mut self, mode: Mode) {
        match self {
            Self::Pacman(g) => g.set_mode(mode),
            Self::Snake(g) => g.set_mode(mode),
        }
    }
    fn controlled(&self) -> bool {
        match self {
            Self::Pacman(g) => g.controlled(),
            Self::Snake(g) => g.controlled(),
        }
    }
    fn score(&self) -> u32 {
        match self {
            Self::Pacman(g) => g.score(),
            Self::Snake(g) => g.score(),
        }
    }
    fn lives(&self) -> u32 {
        match self {
            Self::Pacman(g) => g.lives(),
            Self::Snake(g) => g.lives(),
        }
    }
    fn game_over(&self) -> bool {
        match self {
            Self::Pacman(g) => g.game_over(),
            Self::Snake(g) => g.game_over(),
        }
    }
    fn fade(&self) -> f64 {
        match self {
            Self::Pacman(g) => g.fade(),
            Self::Snake(g) => g.fade(),
        }
    }
    fn reveal(&mut self) {
        match self {
            Self::Pacman(g) => g.reveal(),
            Self::Snake(g) => g.reveal(),
        }
    }
    fn step(&mut self, dt: f64, random: &mut dyn crate::random::Random) {
        match self {
            Self::Pacman(g) => g.step(dt, random),
            Self::Snake(g) => g.step(dt, random),
        }
    }
    fn speech_bubble(&self) -> Option<Bubble> {
        match self {
            Self::Pacman(g) => g.speech_bubble(),
            Self::Snake(g) => g.speech_bubble(),
        }
    }
    fn logo_pickup(&self) -> Option<Pickup<'_>> {
        match self {
            Self::Pacman(g) => g.logo_pickup(),
            Self::Snake(g) => g.logo_pickup(),
        }
    }
    fn write_sprites(&self, out: &mut Vec<Sprite>) {
        match self {
            Self::Pacman(g) => g.write_sprites(out),
            Self::Snake(_) => out.clear(),
        }
    }
    fn stamp(&self, out: &mut [f32], cols: usize, rows: usize) {
        match self {
            Self::Pacman(g) => g.stamp(out, cols, rows),
            Self::Snake(g) => g.stamp(out, cols, rows),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Configuration {
    pub cols: usize,
    pub rows: usize,
    pub playing: bool,
    pub game: GameId,
    pub mode: Mode,
}
/// Borrowed presentation data: no JSON, sprite row copies or pickup/speech
/// string allocation at the production frame boundary.
pub struct Frame<'a> {
    pub cols: usize,
    pub rows: usize,
    pub stamp: &'a [f32],
    pub sprites: &'a [Sprite],
    pub pickup: Option<Pickup<'a>>,
    pub bubble: Option<Bubble>,
    pub score: u32,
    pub lives: u32,
    pub dim: f64,
    pub fade: f64,
    pub game_over: bool,
}
pub struct Controller {
    boards: [Board; 2],
    random: Mulberry32,
    configuration: Configuration,
    stamp: Vec<f32>,
    sprites: Vec<Sprite>,
}
impl Controller {
    pub fn new(providers: Vec<String>, seed: u32) -> Self {
        Self {
            boards: [
                Board::Pacman(Box::new(Pacman::new(providers.clone()))),
                Board::Snake(Box::new(Snake::new(providers))),
            ],
            random: Mulberry32::new(seed),
            configuration: Configuration {
                game: GameId::Pacman,
                playing: false,
                cols: 0,
                rows: 0,
                mode: Mode::Mid,
            },
            stamp: vec![],
            sprites: Vec::with_capacity(5),
        }
    }
    /// Apply same-tick configuration without stepping. `step(0)` is NOT a
    /// substitute: zero-dt calls can steer/release ghosts and check collisions.
    pub fn configure(&mut self, input: Configuration) -> Frame<'_> {
        if self.configuration == input {
            return self.frame();
        }
        let cols = i32::try_from(input.cols).expect("columns exceed engine range");
        let rows = i32::try_from(input.rows).expect("rows exceed engine range");
        input
            .cols
            .checked_mul(input.rows)
            .expect("grid area overflow");
        if self.configuration.game != input.game {
            self.boards[self.configuration.game.index()].release_control(&mut self.random);
        }
        let board = &mut self.boards[input.game.index()];
        board.resize(cols, rows, &mut self.random);
        board.set_mode(input.mode);
        if input.playing && !board.controlled() {
            board.take_control(&mut self.random);
        } else if !input.playing && board.controlled() {
            board.release_control(&mut self.random);
        }
        self.configuration = input;
        self.refresh();
        self.frame()
    }
    /// Deliver input without advancing time; each game validates its own turns.
    pub fn steer(&mut self, heading: Cell) {
        if self.configuration.playing {
            let board = &mut self.boards[self.configuration.game.index()];
            board.steer(heading.x, heading.y);
            // Steering only changes direction/pending turns, not the grid.
            board.write_sprites(&mut self.sprites);
        }
    }
    /// Finish the selected board's boot fade without advancing simulation.
    pub fn reveal(&mut self) -> Frame<'_> {
        self.boards[self.configuration.game.index()].reveal();
        self.refresh();
        self.frame()
    }
    pub fn step(&mut self, dt: f64) -> Frame<'_> {
        assert!(
            dt.is_finite() && dt >= 0.,
            "dt must be finite and nonnegative"
        );
        let board = &mut self.boards[self.configuration.game.index()];
        board.step(dt, &mut self.random);
        self.refresh();
        self.frame()
    }
    /// Host suspension releases selected-board control with the same shared RNG
    /// even when no later render/step will occur. The scheduler remains outside.
    pub fn release_control(&mut self) -> Frame<'_> {
        self.boards[self.configuration.game.index()].release_control(&mut self.random);
        self.configuration.playing = false;
        self.refresh();
        self.frame()
    }
    fn refresh(&mut self) {
        let Configuration {
            cols, rows, game, ..
        } = self.configuration;
        let board = &self.boards[game.index()];
        self.stamp.resize(cols * rows, 0.);
        self.stamp.fill(0.);
        board.stamp(&mut self.stamp, cols, rows);
        board.write_sprites(&mut self.sprites);
    }
    pub fn frame(&self) -> Frame<'_> {
        let configuration = self.configuration;
        let board = &self.boards[configuration.game.index()];
        Frame {
            cols: configuration.cols,
            rows: configuration.rows,
            stamp: &self.stamp,
            sprites: &self.sprites,
            pickup: board.logo_pickup(),
            bubble: board.speech_bubble(),
            score: board.score(),
            lives: board.lives(),
            dim: if configuration.playing {
                1.
            } else {
                configuration.game.descriptor().idle_dim
            },
            fade: board.fade(),
            game_over: board.game_over(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_lifecycle() {
        let same_frame = |a: Frame<'_>, b: Frame<'_>| {
            assert_eq!((a.cols, a.rows), (b.cols, b.rows));
            assert_eq!(a.stamp, b.stamp);
            assert_eq!(a.sprites, b.sprites);
            assert_eq!(a.pickup, b.pickup);
            assert_eq!(a.bubble, b.bubble);
            assert_eq!(
                (a.score, a.lives, a.game_over),
                (b.score, b.lives, b.game_over)
            );
            assert_eq!((a.dim, a.fade), (b.dim, b.fade));
        };
        for (game, heading, dt) in [
            (GameId::Pacman, Cell::new(0, 1), 92.5),
            (GameId::Snake, Cell::new(1, 0), 38.),
        ] {
            let mut input = Configuration {
                cols: 80,
                rows: 40,
                playing: false,
                game,
                mode: Mode::Mid,
            };
            let mut controller = Controller::new(vec!["test".into()], 0);
            let mut reference = Controller::new(vec!["test".into()], 0);
            let frame = controller.configure(input);
            assert_eq!(frame.stamp.len(), input.cols * input.rows);
            assert_eq!(frame.fade, 0.);
            if game == GameId::Pacman {
                assert_eq!(frame.sprites[0].dx, 1); // JS fallback for zero dx.
            }
            same_frame(controller.frame(), reference.configure(input));

            let frame = controller.reveal();
            assert_eq!(frame.fade, 1.);
            assert!(frame.stamp.iter().any(|&value| value > 0.));
            assert!(frame.stamp.iter().all(|value| (0. ..=1.).contains(value)));
            assert!(frame.sprites.iter().all(|sprite| sprite.alpha == 1.));
            // Only one board was revealed: matching after the natural fade
            // proves reveal did not move gameplay, advance timers or draw RNG.
            same_frame(controller.step(420.), reference.step(420.));

            input.playing = true;
            for board in [&mut controller, &mut reference] {
                board.configure(input);
                assert!(board.boards[game.index()].controlled());
                board.reveal();
                board.steer(heading);
            }
            same_frame(controller.step(dt), reference.step(dt));
            assert!(controller.frame().stamp.iter().any(|&value| value > 0.));
            same_frame(controller.configure(input), reference.frame());

            input.cols = 72;
            input.rows = 36;
            let frame = controller.configure(input);
            assert_eq!(
                (frame.cols, frame.rows, frame.stamp.len()),
                (72, 36, 72 * 36)
            );
            same_frame(controller.frame(), reference.configure(input));
            assert_eq!(controller.release_control().dim, game.descriptor().idle_dim);
            assert!(!controller.boards[game.index()].controlled());
            assert!(!controller.configuration.playing);
            same_frame(controller.frame(), reference.release_control());
        }
    }
}
