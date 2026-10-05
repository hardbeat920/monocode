//! Port of src/features/terminal/arcade: the games on the empty-session
//! grid. Pure state machines: the background view steps them on a timer and
//! paints what they stamp.

pub mod grid_arcade;
pub mod grid_games;
pub mod pacman;
pub mod snake;

pub use grid_arcade::{
    ARCADE_MODES, ArcadeMode, ArcadeRng, ArcadeSprite, GridArcade, LogoPickup, SpeechBubble,
    SpriteFrame, SpriteKind,
};
pub use grid_games::{GRID_GAMES, GridGame, SLIDE_HOLD_MS, step_slider};
pub use pacman::PacmanArcade;
pub use snake::SnakeArcade;
