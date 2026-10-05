//! Port of snakeArcade.test.ts. The TypeScript ran on the real
//! `Math.random`; these runs pin a seed so they play out the same way
//! every time.

use std::collections::HashSet;

use super::*;

/// Roughly what a normal window gives us: 6px cells, a 192px-tall band.
const COLS: i32 = 172;
const ROWS: i32 = 28;
const FRAME_MS: f64 = 33.0;
/// Matches the arcade's own logo lifetime.
const LOGO_LIFE: f64 = 12000.0;

fn rng() -> ArcadeRng {
    ArcadeRng::seeded(0x5eed)
}

struct Run {
    eaten: usize,
    expired: usize,
    peak: f32,
}

fn run(cols: i32, rows: i32, ms: f64) -> Run {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(cols, rows);
    let mut stamp = vec![0f32; (cols * rows) as usize];
    let play_rows = ((f64::from(rows) * 0.62).floor() as i32).max(8);

    let mut eaten = 0;
    let mut expired = 0;
    let mut on_board_for = 0.0;
    let mut peak = 0f32;

    let mut elapsed = 0.0;
    while elapsed < ms {
        elapsed += FRAME_MS;
        arcade.step(FRAME_MS);
        stamp.fill(0.0);
        arcade.stamp(&mut stamp, cols as usize, rows as usize);
        for value in &stamp {
            peak = peak.max(*value);
        }

        if let Some(pickup) = arcade.logo_pickup() {
            assert!(HARNESSES.contains(&pickup.harness));
            assert!(pickup.alpha >= 0.0);
            assert!(pickup.alpha <= 1.0);
            assert_eq!(pickup.cells, f64::from(LOGO_CELLS));
            assert!(pickup.x >= 0.0);
            assert!(pickup.y >= 0.0);
            assert!(pickup.x + pickup.cells <= f64::from(cols));
            assert!(pickup.y + pickup.cells <= f64::from(play_rows));
            on_board_for += FRAME_MS;
        } else if on_board_for > 0.0 {
            // Gone well before its lifetime ran out means the snake got it.
            if on_board_for < LOGO_LIFE - FRAME_MS * 2.0 {
                eaten += 1;
            } else {
                expired += 1;
            }
            on_board_for = 0.0;
        }
    }

    Run {
        eaten,
        expired,
        peak,
    }
}

#[test]
fn detours_to_swallow_the_provider_logos() {
    // Logos arrive every 4-10s, so two minutes should clear a good handful.
    let result = run(COLS, ROWS, 120_000.0);
    assert!(result.eaten >= 6, "eaten {}", result.eaten);
    assert_eq!(result.expired, 0);
}

#[test]
fn eases_the_pickup_in_rather_than_popping_it_into_place() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    let mut tracked: Option<(String, f64)> = None;

    let mut t = 0.0;
    while t < 120_000.0 {
        t += FRAME_MS;
        arcade.step(FRAME_MS);
        let Some(pickup) = arcade.logo_pickup() else {
            tracked = None;
            continue;
        };

        let key = format!("{:?}:{},{}", pickup.harness, pickup.x, pickup.y);
        match &mut tracked {
            Some((tracked_key, shown_for)) if *tracked_key == key => {
                *shown_for += FRAME_MS;
                if *shown_for > 1000.0 {
                    assert!((pickup.alpha - 1.0).abs() < 1e-5);
                    return;
                }
            }
            _ => {
                // First frame of this logo: it starts invisible and fades up.
                assert!(pickup.alpha < 0.2);
                tracked = Some((key, 0.0));
            }
        }
    }

    panic!("no logo stayed up long enough to finish fading in");
}

#[test]
fn keeps_every_stamped_cell_inside_the_intensity_range() {
    let result = run(COLS, ROWS, 30_000.0);
    assert!(result.peak > 0.0);
    assert!(result.peak <= 1.0);
}

#[test]
fn skips_the_logo_entirely_on_grids_too_small_to_hold_one() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(LOGO_CELLS + 2, 6);
    let mut t = 0.0;
    while t < 60_000.0 {
        t += FRAME_MS;
        arcade.step(FRAME_MS);
        assert!(arcade.logo_pickup().is_none());
    }
}

#[test]
fn survives_small_and_awkward_grids() {
    for (cols, rows) in [(8, 4), (12, 9), (20, 12)] {
        run(cols, rows, 10_000.0);
    }
}

#[test]
fn handles_being_stepped_before_it_has_a_size() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.step(FRAME_MS);
    arcade.stamp(&mut [], 0, 0);
    assert!(arcade.logo_pickup().is_none());
}

#[test]
fn only_speaks_up_on_the_frame_it_takes_a_logo() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    let mut on_board_for = 0.0;
    let mut showing = false;
    let mut spoke = 0;

    let mut t = 0.0;
    while t < 120_000.0 {
        t += FRAME_MS;
        arcade.step(FRAME_MS);
        let logo = arcade.logo_pickup();
        let bubble = arcade.speech_bubble();
        let started = bubble.is_some() && !showing;

        if logo.is_some() {
            on_board_for += FRAME_MS;
        } else if on_board_for > 0.0 {
            // Gone well before its lifetime ran out means the snake got it,
            // and that is the only thing that should set a line going.
            let eaten = on_board_for < LOGO_LIFE - FRAME_MS * 2.0;
            assert_eq!(started, eaten);
            if eaten {
                spoke += 1;
            }
            on_board_for = 0.0;
        } else {
            assert!(!started);
        }

        showing = bubble.is_some();
    }

    assert!(spoke >= 6, "spoke {spoke}");
}

#[test]
fn follows_the_head_rather_than_sitting_still() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    let mut spots: HashSet<(i64, i64)> = HashSet::new();

    let mut t = 0.0;
    while t < 120_000.0 {
        t += FRAME_MS;
        arcade.step(FRAME_MS);
        // Follow a single bubble from the moment it appears until it goes.
        let Some(bubble) = arcade.speech_bubble() else {
            if !spots.is_empty() {
                break;
            }
            continue;
        };
        spots.insert((bubble.x as i64, bubble.y as i64));
    }

    // The snake keeps moving for the couple of seconds the line is up.
    assert!(spots.len() > 5);
}

#[test]
fn never_says_the_same_thing_twice_running() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    let mut said: Vec<&str> = Vec::new();

    let mut t = 0.0;
    while t < 600_000.0 {
        t += FRAME_MS;
        arcade.step(FRAME_MS);
        if let Some(bubble) = arcade.speech_bubble()
            && said.last() != Some(&bubble.text)
        {
            said.push(bubble.text);
        }
    }

    assert!(said.len() > 20, "said {}", said.len());
    for pair in said.windows(2) {
        assert_ne!(pair[1], pair[0]);
    }
}

#[test]
fn fades_the_board_in_instead_of_painting_at_full_strength() {
    let mut arcade = SnakeArcade::new(rng());
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
    let mut arcade = SnakeArcade::new(rng());
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
fn has_no_sprites_of_its_own_the_body_is_stamped_onto_the_grid() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    arcade.step(FRAME_MS);
    assert!(arcade.sprites().is_empty());
}

/// Matches mid mode's player tick.
const TICK: f64 = 38.0;
const IDLE_PLAY_ROWS: i32 = 17;
/// Matches the arcade's first pellet when a player takes over.
const PELLET_AHEAD: usize = 8;

struct Head {
    x: i32,
    y: i32,
    value: f32,
}

fn stamp_head(arcade: &SnakeArcade, cols: i32, rows: i32) -> Head {
    let mut stamp = vec![0f32; (cols * rows) as usize];
    arcade.stamp(&mut stamp, cols as usize, rows as usize);
    // The pellet is always the brightest cell and the head is next, even
    // while the boot fade is still scaling both down.
    let mut pellet_at = 0;
    let mut pellet = -1f32;
    for (index, value) in stamp.iter().enumerate() {
        if *value > pellet {
            pellet = *value;
            pellet_at = index;
        }
    }
    let mut best = -1f32;
    let mut at = 0;
    for (index, value) in stamp.iter().enumerate() {
        if index == pellet_at {
            continue;
        }
        if *value > best {
            best = *value;
            at = index;
        }
    }
    Head {
        x: at as i32 % cols,
        y: at as i32 / cols,
        value: best,
    }
}

#[test]
fn idle_play_rows_match_the_arcade() {
    assert_eq!(
        IDLE_PLAY_ROWS,
        ((f64::from(ROWS) * 0.62).floor() as i32).max(8)
    );
}

#[test]
fn steers_the_snake_instead_of_thinking_for_itself() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    arcade.take_control();
    assert!(arcade.controlled());

    arcade.steer(0, 1);
    for _ in 0..5 {
        arcade.step(TICK);
    }

    let head = stamp_head(&arcade, COLS, ROWS);
    let spawn_y = (ROWS / 2).max(1);
    let spawn_x = (COLS / 4).max(2);
    assert_eq!(head.x, spawn_x);
    assert_eq!(head.y, spawn_y + 5);
}

#[test]
fn ignores_a_reverse_into_the_body() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    arcade.take_control();
    arcade.steer(-1, 0);
    for _ in 0..5 {
        arcade.step(TICK);
    }

    let head = stamp_head(&arcade, COLS, ROWS);
    let spawn_y = (ROWS / 2).max(1);
    let spawn_x = (COLS / 4).max(2);
    assert_eq!(head.x, spawn_x + 5);
    assert_eq!(head.y, spawn_y);
}

#[test]
fn ignores_steering_until_someone_takes_control() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    assert!(!arcade.controlled());
    arcade.steer(0, 1);
    assert_eq!(arcade.score(), 0);
}

#[test]
fn scores_the_pellet_waiting_in_front_of_a_fresh_player_snake() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    arcade.take_control();
    assert_eq!(arcade.score(), 0);
    for _ in 0..PELLET_AHEAD {
        arcade.step(TICK);
    }
    assert_eq!(arcade.score(), 1);
}

#[test]
fn uses_the_full_board_rather_than_the_faded_idle_strip() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    arcade.take_control();
    arcade.steer(0, 1);
    for _ in 0..6 {
        arcade.step(TICK);
    }

    let head = stamp_head(&arcade, COLS, ROWS);
    assert!(head.y > IDLE_PLAY_ROWS - 1);
}

#[test]
fn hands_the_idle_brain_back_when_control_is_released() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    arcade.take_control();
    for _ in 0..PELLET_AHEAD {
        arcade.step(TICK);
    }
    assert_eq!(arcade.score(), 1);

    arcade.release_control();
    assert!(!arcade.controlled());
    assert_eq!(arcade.score(), 0);
}

#[test]
fn starts_on_mid_and_lets_hard_outrun_low() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    arcade.take_control();
    assert_eq!(arcade.mode(), ArcadeMode::Mid);

    let run_mode = |mode: ArcadeMode| {
        let mut next = SnakeArcade::new(rng());
        next.resize(COLS, ROWS);
        next.set_mode(mode);
        next.take_control();
        next.step(300.0);
        stamp_head(&next, COLS, ROWS).x
    };

    let spawn_x = (COLS / 4).max(2);
    assert!(run_mode(ArcadeMode::Low) > spawn_x);
    assert!(run_mode(ArcadeMode::Mid) > run_mode(ArcadeMode::Low));
    assert!(run_mode(ArcadeMode::Hard) > run_mode(ArcadeMode::Mid));
}

#[test]
fn keeps_a_live_game_when_the_pane_is_resized() {
    let mut arcade = SnakeArcade::new(rng());
    arcade.resize(COLS, ROWS);
    arcade.take_control();
    for _ in 0..PELLET_AHEAD {
        arcade.step(TICK);
    }
    assert_eq!(arcade.score(), 1);

    arcade.resize(COLS - 40, ROWS + 12);
    assert!(arcade.controlled());
    assert_eq!(arcade.score(), 1);
    assert_eq!(arcade.mode(), ArcadeMode::Mid);

    let next = stamp_head(&arcade, COLS - 40, ROWS + 12);
    assert!(next.value > 0.0);
    assert!(next.x >= 0);
    assert!(next.x < COLS - 40);
    assert!(next.y >= 0);
    assert!(next.y < ROWS + 12);
}
