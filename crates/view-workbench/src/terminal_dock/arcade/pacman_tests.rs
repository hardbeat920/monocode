//! Port of pacmanArcade.test.ts.
//!
//! The maze, the logo drops, the mascot pen, and the chatter all come off
//! the random stream, so a run is only as repeatable as that stream. Each
//! test pins it to the same seed the TypeScript test used, and every arcade
//! in a test shares the one stream, as they shared the mocked
//! `Math.random`. Set `ARCADE_SEED` to sweep other boards.

use std::collections::HashSet;

use super::*;

/// Roughly what a normal window gives us: 6px cells, a 192px-tall band.
const COLS: i32 = 172;
const ROWS: i32 = 28;
const FRAME_MS: f64 = 33.0;
/// Matches the arcade's own logo lifetime.
const LOGO_LIFE: f64 = 12000.0;

fn seed() -> u32 {
    std::env::var("ARCADE_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0x5eed)
}

const HEADINGS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

fn pacman(arcade: &PacmanArcade) -> ArcadeSprite {
    arcade
        .sprites()
        .into_iter()
        .find(|sprite| sprite.kind == SpriteKind::Pacman)
        .expect("no pac-man on the board")
}

fn ghosts(arcade: &PacmanArcade) -> Vec<ArcadeSprite> {
    arcade
        .sprites()
        .into_iter()
        .filter(|sprite| sprite.kind == SpriteKind::Ghost)
        .collect()
}

/// `Math.sign`, as an integer.
fn sign_of(value: f64) -> i32 {
    sign(value)
}

/// Pac-man spawns stopped and only takes a heading that opens onto a
/// corridor, which the maze picks at random, so try each in turn until one
/// lands.
fn start_moving(arcade: &mut PacmanArcade) -> Option<(i32, i32)> {
    for heading in HEADINGS {
        arcade.steer(heading.0, heading.1);
        let before = pacman(arcade);
        arcade.step(40.0);
        let after = pacman(arcade);
        if after.cx != before.cx || after.cy != before.cy {
            return Some(heading);
        }
    }
    None
}

struct IdleRun {
    eaten: usize,
    #[allow(dead_code)]
    expired: usize,
    peak: f32,
    frightened: usize,
    score: i64,
}

fn idle(rng: &ArcadeRng, cols: i32, rows: i32, ms: f64) -> IdleRun {
    let mut arcade = PacmanArcade::new(rng.clone());
    arcade.resize(cols, rows);
    let mut stamp = vec![0f32; (cols * rows) as usize];

    let mut eaten = 0;
    let mut expired = 0;
    let mut on_board_for = 0.0;
    let mut peak = 0f32;
    let mut frightened = 0;

    let mut elapsed = 0.0;
    while elapsed < ms {
        arcade.step(FRAME_MS);
        stamp.fill(0.0);
        arcade.stamp(&mut stamp, cols as usize, rows as usize);
        for value in &stamp {
            peak = peak.max(*value);
        }
        for ghost in ghosts(&arcade) {
            if ghost.alpha < 0.9 || ghost.eyes {
                frightened += 1;
            }
        }

        if let Some(pickup) = arcade.logo_pickup() {
            assert!(HARNESSES.contains(&pickup.harness));
            assert!(pickup.alpha >= 0.0);
            assert!(pickup.alpha <= 1.0);
            assert!(pickup.x >= 0.0);
            assert!(pickup.y >= 0.0);
            assert!(pickup.x + pickup.cells <= f64::from(cols));
            assert!(pickup.y + pickup.cells <= f64::from(rows));
            on_board_for += FRAME_MS;
        } else if on_board_for > 0.0 {
            // Gone well before its lifetime ran out means pac-man got it.
            if on_board_for < LOGO_LIFE - FRAME_MS * 2.0 {
                eaten += 1;
            } else {
                expired += 1;
            }
            on_board_for = 0.0;
        }
        elapsed += FRAME_MS;
    }

    IdleRun {
        eaten,
        expired,
        peak,
        frightened,
        score: arcade.score(),
    }
}

#[test]
fn works_its_way_through_the_pellets_on_its_own() {
    let run = idle(&ArcadeRng::seeded(seed()), COLS, ROWS, 30_000.0);
    assert!(run.score > 500, "score {}", run.score);
}

#[test]
fn detours_to_swallow_the_provider_logos() {
    // Logos arrive every 4-10s, so two minutes should clear a good handful.
    let run = idle(&ArcadeRng::seeded(seed()), COLS, ROWS, 120_000.0);
    assert!(run.eaten >= 4, "eaten {}", run.eaten);
}

#[test]
fn eases_the_pickup_in_rather_than_popping_it_into_place() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.resize(COLS, ROWS);
    let mut key = String::new();
    let mut alphas: Vec<f64> = Vec::new();

    let mut t = 0.0;
    while t < 120_000.0 {
        t += FRAME_MS;
        arcade.step(FRAME_MS);
        let Some(pickup) = arcade.logo_pickup() else {
            key.clear();
            alphas.clear();
            continue;
        };

        let next = format!("{:?}:{},{}", pickup.harness, pickup.x, pickup.y);
        if next != key {
            key = next;
            alphas.clear();
        }
        alphas.push(pickup.alpha);
        if pickup.alpha < 1.0 {
            continue;
        }

        // It starts invisible and walks up a frame at a time. The board
        // pauses on a death, so this counts frames rather than wall time.
        assert!(alphas[0] < 0.2);
        for pair in alphas.windows(2) {
            assert!(pair[1] >= pair[0]);
            assert!(pair[1] - pair[0] < 0.1);
        }
        return;
    }

    panic!("no logo stayed up long enough to finish fading in");
}

#[test]
fn runs_the_maze_off_every_edge_of_the_pane() {
    let rng = ArcadeRng::seeded(seed());
    for control in [false, true] {
        let mut arcade = PacmanArcade::new(rng.clone());
        if control {
            arcade.take_control();
        }
        arcade.resize(COLS, ROWS);
        arcade.step(500.0);

        let mut stamp = vec![0f32; (COLS * ROWS) as usize];
        arcade.stamp(&mut stamp, COLS as usize, ROWS as usize);
        let lit = |x: i32, y: i32| stamp[(y * COLS + x) as usize] > 0.0;
        let column = |x: i32| (0..ROWS).any(|y| lit(x, y));
        let row = |y: i32| (0..COLS).any(|x| lit(x, y));

        // The border ring hangs off the pane, so the edge lands mid-maze. A
        // tile's worth of corridor is as much dead margin as there should be.
        let margin = 4;
        assert!((0..margin).any(column));
        assert!((0..margin).any(|x| column(COLS - 1 - x)));
        assert!((0..margin).any(row));
        assert!((0..margin).any(|y| row(ROWS - 1 - y)));
    }
}

#[test]
fn keeps_every_stamped_cell_inside_the_intensity_range() {
    let run = idle(&ArcadeRng::seeded(seed()), COLS, ROWS, 30_000.0);
    assert!(run.peak > 0.0);
    assert!(run.peak <= 1.0);
}

#[test]
fn puts_four_different_mascots_on_the_board() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.resize(COLS, ROWS);
    let mut most = 0;

    // They leave the pen staggered, and a caught pac-man puts them back.
    let mut t = 0.0;
    while t < 30_000.0 {
        t += FRAME_MS;
        arcade.step(FRAME_MS);
        let names: Vec<&str> = ghosts(&arcade)
            .iter()
            .map(|ghost| ghost.mascot.expect("mascot"))
            .collect();
        for name in &names {
            assert!(!name.is_empty());
        }
        assert_eq!(names.iter().collect::<HashSet<_>>().len(), names.len());
        most = most.max(names.len());
    }

    assert_eq!(most, 4);
}

#[test]
fn turns_the_mascots_edible_once_an_energizer_goes_down() {
    // A short board so the corners, where the energizers sit, come up quickly.
    let run = idle(&ArcadeRng::seeded(seed()), 90, 24, 180_000.0);
    assert!(run.frightened > 0);
}

#[test]
fn sits_the_game_out_on_grids_too_small_to_hold_a_maze() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.resize(9, 6);
    let mut t = 0.0;
    while t < 60_000.0 {
        t += FRAME_MS;
        arcade.step(FRAME_MS);
        assert!(arcade.logo_pickup().is_none());
        assert!(arcade.sprites().is_empty());
    }
}

#[test]
fn survives_small_and_awkward_grids() {
    let rng = ArcadeRng::seeded(seed());
    for (cols, rows) in [(8, 4), (12, 9), (20, 12)] {
        idle(&rng, cols, rows, 10_000.0);
    }
}

#[test]
fn handles_being_stepped_before_it_has_a_size() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.step(FRAME_MS);
    arcade.stamp(&mut [], 0, 0);
    assert!(arcade.logo_pickup().is_none());
    assert!(arcade.speech_bubble().is_none());
}

#[test]
fn pipes_up_over_whoever_is_talking_and_never_twice_with_the_same_line() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.resize(COLS, ROWS);
    let mut said: Vec<&str> = Vec::new();
    let mut spots: HashSet<(i64, i64)> = HashSet::new();
    let mut travelled = 0;

    let mut t = 0.0;
    while t < 600_000.0 {
        t += FRAME_MS;
        arcade.step(FRAME_MS);
        let Some(bubble) = arcade.speech_bubble() else {
            spots.clear();
            continue;
        };
        if said.last() != Some(&bubble.text) {
            said.push(bubble.text);
            spots.clear();
        }
        // A line said on the move follows whoever said it.
        spots.insert((bubble.x as i64, bubble.y as i64));
        travelled = travelled.max(spots.len());
    }

    assert!(said.len() > 20, "said {}", said.len());
    for pair in said.windows(2) {
        assert_ne!(pair[1], pair[0]);
    }
    assert!(travelled > 5, "travelled {travelled}");
}

#[test]
fn fades_the_board_in_instead_of_painting_at_full_strength() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.resize(COLS, ROWS);
    assert_eq!(arcade.fade(), 0.0);
    arcade.step(FRAME_MS);
    assert!(arcade.fade() > 0.0);
    assert!(arcade.fade() < 1.0);
    arcade.step(500.0);
    assert_eq!(arcade.fade(), 1.0);
}

#[test]
fn reboots_cleanly_when_the_window_is_resized() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    for (cols, rows) in [(COLS, ROWS), (40, 12), (300, 40), (COLS, ROWS)] {
        arcade.resize(cols, rows);
        for _ in 0..400 {
            arcade.step(FRAME_MS);
        }
        let mut stamp = vec![0f32; (cols * rows) as usize];
        arcade.stamp(&mut stamp, cols as usize, rows as usize);
    }
}

#[test]
fn steers_pac_man_instead_of_thinking_for_him() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.resize(COLS, ROWS);
    arcade.take_control();
    assert!(arcade.controlled());

    let before = pacman(&arcade);
    let heading = start_moving(&mut arcade).expect("a heading");

    let after = pacman(&arcade);
    // He goes exactly the way he was pointed, and nowhere else.
    assert_eq!(sign_of(after.cx - before.cx), heading.0);
    assert_eq!(sign_of(after.cy - before.cy), heading.1);
}

#[test]
fn stands_still_until_someone_points_him_somewhere() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.resize(COLS, ROWS);
    arcade.take_control();

    let before = pacman(&arcade);
    for _ in 0..20 {
        arcade.step(FRAME_MS);
    }
    let after = pacman(&arcade);
    assert_eq!(after.cx, before.cx);
    assert_eq!(after.cy, before.cy);
    assert_eq!(arcade.score(), 0);
}

#[test]
fn turns_on_the_spot_when_reversed_mid_corridor() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.resize(COLS, ROWS);
    arcade.take_control();
    let heading = start_moving(&mut arcade).expect("a heading");

    let turn = pacman(&arcade);
    arcade.steer(-heading.0, -heading.1);
    arcade.step(60.0);
    let after = pacman(&arcade);
    assert_eq!(sign_of(after.cx - turn.cx), -heading.0);
    assert_eq!(sign_of(after.cy - turn.cy), -heading.1);
}

#[test]
fn ignores_steering_until_someone_takes_control() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.resize(COLS, ROWS);
    assert!(!arcade.controlled());
    arcade.steer(0, 1);
    assert_eq!(arcade.score(), 0);
}

#[test]
fn starts_on_mid_and_lets_hard_outrun_low() {
    let rng = ArcadeRng::seeded(seed());
    let travel = |mode: ArcadeMode| {
        let mut arcade = PacmanArcade::new(rng.clone());
        arcade.resize(COLS, ROWS);
        arcade.set_mode(mode);
        arcade.take_control();
        start_moving(&mut arcade).expect("a heading");

        let before = pacman(&arcade);
        // Short enough that he cannot reach the next tile and turn.
        arcade.step(60.0);
        let after = pacman(&arcade);
        (after.cx - before.cx).abs() + (after.cy - before.cy).abs()
    };

    let fresh = PacmanArcade::new(rng.clone());
    assert_eq!(fresh.mode(), ArcadeMode::Mid);
    assert!(travel(ArcadeMode::Mid) > travel(ArcadeMode::Low));
    assert!(travel(ArcadeMode::Hard) > travel(ArcadeMode::Mid));
}

#[test]
fn gives_the_player_three_lives() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.resize(COLS, ROWS);
    arcade.take_control();
    assert_eq!(arcade.lives(), 3);
    assert!(!arcade.game_over());
}

#[test]
fn keeps_a_live_game_when_the_pane_is_resized() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.resize(COLS, ROWS);
    arcade.take_control();
    start_moving(&mut arcade);
    for _ in 0..20 {
        arcade.step(FRAME_MS);
    }
    let scored = arcade.score();
    assert!(scored > 0);

    arcade.resize(COLS - 40, ROWS + 12);
    assert!(arcade.controlled());
    assert_eq!(arcade.score(), scored);
    assert_eq!(arcade.mode(), ArcadeMode::Mid);

    let pac = pacman(&arcade);
    assert!(pac.cx >= 0.0);
    assert!(pac.cx < f64::from(COLS - 40));
    assert!(pac.cy >= 0.0);
    assert!(pac.cy < f64::from(ROWS + 12));
}

#[test]
fn hands_the_idle_brain_back_when_control_is_released() {
    let mut arcade = PacmanArcade::new(ArcadeRng::seeded(seed()));
    arcade.resize(COLS, ROWS);
    arcade.take_control();
    start_moving(&mut arcade);
    for _ in 0..20 {
        arcade.step(FRAME_MS);
    }
    assert!(arcade.score() > 0);

    arcade.release_control();
    assert!(!arcade.controlled());
    assert_eq!(arcade.score(), 0);
}
