//! Port of src/features/sessions/model/composerRunner.ts: the pixel mascot
//! that patrols the composer's top edge while a turn is in flight, hops a
//! jump-to-latest chevron, collects coins, and hops off when the turn ends.
//!
//! Units are CSS px and milliseconds, as in the TypeScript.

/// `RUNNER_SIZE`.
pub const RUNNER_SIZE: f32 = 16.0;
pub const RUNNER_SPEED_PX: f32 = 160.0;
pub const RUNNER_INSET: f32 = 10.0;
/// Start the jump this far before the obstacle, land the same distance after.
pub const JUMP_LEAD: f32 = 18.0;
const JUMP_CLEARANCE: f32 = 10.0;
const JUMP_MIN: f32 = 28.0;

pub const COIN_SIZE: f32 = 12.0;
/// Coin center above the rim, high enough that the mascot jumps into it.
pub const COIN_HOVER: f32 = 42.0;
pub const COIN_WIDTH: f32 = 8.0;
/// Longer than the chevron hop so the takeoff reads instead of twitching.
pub const COIN_JUMP_LEAD: f32 = 34.0;
pub const COIN_GAP_MIN_MS: f32 = 7000.0;
pub const COIN_GAP_MAX_MS: f32 = 18000.0;
pub const COIN_FIRST_MIN_MS: f32 = 3500.0;
pub const COIN_FIRST_MAX_MS: f32 = 9000.0;
pub const COLLECT_X: f32 = 10.0;
pub const COLLECT_POP_MS: f32 = 280.0;
pub const COLLECT_POP_PX: f32 = 16.0;

pub const EXIT_MS: f32 = 560.0;
pub const EXIT_PEAK: f32 = 44.0;
pub const EXIT_SINK: f32 = 20.0;
const EXIT_APEX: f32 = 0.38;

/// First chevron hit this turn: knock-back, stars, then the mascot learns
/// the hop.
pub const CRASH_RECOIL_PX: f32 = 18.0;
pub const CRASH_RECOIL_MS: f32 = 140.0;
pub const CRASH_STUN_MS: f32 = 560.0;
pub const CRASH_SHAKE_MS: f32 = 480.0;
pub const STAR_SIZE: f32 = 8.0;
pub const STAR_COUNT: usize = 3;
pub const STAR_ORBIT: f32 = 11.0;
/// One full star orbit.
pub const STAR_SPIN_MS: f32 = 520.0;

/// Which way the sprite faces.
pub type Facing = i8;

/// A rectangle in window coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub width: Option<f32>,
}

impl Rect {
    pub fn new(left: f32, right: f32, top: f32, bottom: f32) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
            width: None,
        }
    }

    fn width(&self) -> f32 {
        self.width.unwrap_or(self.right - self.left)
    }
}

/// A hurdle in box coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obstacle {
    pub left: f32,
    pub right: f32,
    /// Peak of the jump arc, in px above the top border.
    pub height: f32,
}

/// A coin on the track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coin {
    pub id: u32,
    /// Center X in box coordinates.
    pub x: f32,
    /// How high the mascot must jump to grab it.
    pub height: f32,
}

/// Where the sprite is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunnerPose {
    /// Sprite center X, in box coordinates.
    pub x: f32,
    /// Feet height above the top border. Negative sinks behind the box.
    pub y: f32,
    pub facing: Facing,
    pub airborne: bool,
}

/// `pingPong`.
pub fn ping_pong(distance: f32, length: f32) -> (f32, Facing) {
    if length <= 0.0 {
        return (0.0, 1);
    }
    let cycle = length * 2.0;
    let d = ((distance % cycle) + cycle) % cycle;
    if d <= length { (d, 1) } else { (cycle - d, -1) }
}

fn arc(x: f32, left: f32, right: f32, height: f32, lead: f32) -> f32 {
    let start = left - lead;
    let end = right + lead;
    if end <= start || x <= start || x >= end {
        return 0.0;
    }
    let t = (x - start) / (end - start);
    4.0 * t * (1.0 - t) * height
}

/// `coinJumpPeak`: feet peak so the sprite's body meets the coin.
pub fn coin_jump_peak(coin: &Coin) -> f32 {
    (coin.height - RUNNER_SIZE / 2.0).max(0.0)
}

/// `jumpHeight`: 0 at the ends of each arc, the peak in the middle.
pub fn jump_height(x: f32, obstacle: Option<&Obstacle>, coins: &[Coin]) -> f32 {
    let mut height = obstacle
        .map(|o| arc(x, o.left, o.right, o.height, JUMP_LEAD))
        .unwrap_or(0.0);
    for coin in coins {
        height = height.max(arc(
            x,
            coin.x - COIN_WIDTH / 2.0,
            coin.x + COIN_WIDTH / 2.0,
            coin_jump_peak(coin),
            COIN_JUMP_LEAD,
        ));
    }
    height
}

/// `runnerPose`.
pub fn runner_pose(
    distance: f32,
    box_width: f32,
    obstacle: Option<&Obstacle>,
    coins: &[Coin],
) -> RunnerPose {
    let track = (box_width - RUNNER_INSET * 2.0).max(0.0);
    let (t, facing) = ping_pong(distance, track);
    let x = RUNNER_INSET + t;
    let y = jump_height(x, obstacle, coins);
    RunnerPose {
        x,
        y,
        facing,
        airborne: y > 0.5,
    }
}

/// `scaleTrackX`: keep a position in the same relative spot when the width
/// changes.
pub fn scale_track_x(x: f32, from_width: f32, to_width: f32) -> f32 {
    if from_width <= 0.0 {
        return 0.0;
    }
    x * (to_width / from_width)
}

/// `stepAlong`.
pub fn step_along(
    along: f32,
    facing: Facing,
    dt_ms: f32,
    track_width: f32,
    speed: f32,
) -> (f32, Facing) {
    if track_width <= 0.0 {
        return (0.0, 1);
    }
    let next = along + facing as f32 * speed * (dt_ms / 1000.0);
    if next >= track_width {
        (track_width, -1)
    } else if next <= 0.0 {
        (0.0, 1)
    } else {
        (next, facing)
    }
}

/// `poseAt`.
pub fn pose_at(
    along: f32,
    facing: Facing,
    box_width: f32,
    obstacle: Option<&Obstacle>,
    coins: &[Coin],
) -> RunnerPose {
    let track = (box_width - RUNNER_INSET * 2.0).max(0.0);
    let x = RUNNER_INSET + along.clamp(0.0, track);
    let y = jump_height(x, obstacle, coins);
    RunnerPose {
        x,
        y,
        facing,
        airborne: y > 0.5,
    }
}

/// `hitsChevron`: first contact this turn, unless already learned or
/// hopping a coin.
pub fn hits_chevron(
    x: f32,
    y: f32,
    facing: Facing,
    obstacle: Option<&Obstacle>,
    learned: bool,
) -> bool {
    let Some(obstacle) = obstacle else {
        return false;
    };
    if learned || y > 0.5 {
        return false;
    }
    let half = RUNNER_SIZE / 2.0;
    if facing == 1 {
        x + half >= obstacle.left && x - half < obstacle.right
    } else {
        x - half <= obstacle.right && x + half > obstacle.left
    }
}

/// `recoilAlong`: knocked back from the hurdle, easing out.
pub fn recoil_along(hit_along: f32, facing: Facing, elapsed_ms: f32, track_width: f32) -> f32 {
    let t = (elapsed_ms / CRASH_RECOIL_MS).clamp(0.0, 1.0);
    let eased = 1.0 - (1.0 - t) * (1.0 - t);
    let next = hit_along - facing as f32 * CRASH_RECOIL_PX * eased;
    next.clamp(0.0, track_width)
}

/// `stunShake`.
pub fn stun_shake(elapsed_ms: f32) -> (f32, f32) {
    if elapsed_ms <= 0.0 || elapsed_ms >= CRASH_SHAKE_MS {
        return (0.0, 0.0);
    }
    let decay = 1.0 - elapsed_ms / CRASH_SHAKE_MS;
    (
        js_round((elapsed_ms / 32.0).sin() * 3.0 * decay),
        js_round((elapsed_ms / 26.0).cos() * 2.0 * decay),
    )
}

/// `stunDone`.
pub fn stun_done(elapsed_ms: f32) -> bool {
    elapsed_ms >= CRASH_STUN_MS
}

/// One orbiting star, offset from the sprite's top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Star {
    pub dx: f32,
    pub dy: f32,
    pub opacity: f32,
}

/// `stunStars`.
pub fn stun_stars(elapsed_ms: f32) -> Vec<Star> {
    if !(0.0..CRASH_STUN_MS).contains(&elapsed_ms) {
        return Vec::new();
    }
    let fade_at = CRASH_STUN_MS - 140.0;
    let opacity = if elapsed_ms < fade_at {
        1.0
    } else {
        (1.0 - (elapsed_ms - fade_at) / 140.0).max(0.0)
    };
    let origin_x = (RUNNER_SIZE - STAR_SIZE) / 2.0;
    let origin_y = (RUNNER_SIZE - STAR_SIZE) / 2.0 - 5.0;
    let angle = (elapsed_ms / STAR_SPIN_MS) * std::f32::consts::TAU;
    (0..STAR_COUNT)
        .map(|i| {
            let a = angle + (i as f32 * std::f32::consts::TAU) / STAR_COUNT as f32;
            Star {
                dx: js_round(origin_x + a.cos() * STAR_ORBIT),
                dy: js_round(origin_y + a.sin() * STAR_ORBIT),
                opacity,
            }
        })
        .collect()
}

/// `coinCollected`.
pub fn coin_collected(pose: &RunnerPose, coin: &Coin) -> bool {
    if (pose.x - coin.x).abs() > COLLECT_X {
        return false;
    }
    let mascot_top = pose.y + RUNNER_SIZE;
    let mascot_bottom = pose.y;
    let coin_top = coin.height + COIN_SIZE / 2.0;
    let coin_bottom = coin.height - COIN_SIZE / 2.0;
    mascot_top >= coin_bottom && mascot_bottom <= coin_top
}

/// `nextCoinDelay`. `random` returns a value in `0..1`.
pub fn next_coin_delay(first: bool, random: &mut impl FnMut() -> f32) -> f32 {
    let (min, max) = if first {
        (COIN_FIRST_MIN_MS, COIN_FIRST_MAX_MS)
    } else {
        (COIN_GAP_MIN_MS, COIN_GAP_MAX_MS)
    };
    min + random() * (max - min)
}

/// `pickCoinX`: a spot on the track away from the runner and the chevron.
pub fn pick_coin_x(
    box_width: f32,
    runner_x: f32,
    obstacle: Option<&Obstacle>,
    random: &mut impl FnMut() -> f32,
) -> Option<f32> {
    let min = RUNNER_INSET + COIN_JUMP_LEAD + 8.0;
    let max = box_width - RUNNER_INSET - COIN_JUMP_LEAD - 8.0;
    if max <= min {
        return None;
    }
    for _ in 0..8 {
        let x = min + random() * (max - min);
        if (x - runner_x).abs() < 40.0 {
            continue;
        }
        if let Some(o) = obstacle
            && x >= o.left - 6.0
            && x <= o.right + 6.0
        {
            continue;
        }
        return Some(x);
    }
    Some(min + random() * (max - min))
}

/// `exitJumpY`: a hop that peaks, then drops below the rim.
pub fn exit_jump_y(t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return -EXIT_SINK;
    }
    if t < EXIT_APEX {
        let u = t / EXIT_APEX;
        return EXIT_PEAK * (1.0 - (1.0 - u) * (1.0 - u));
    }
    let u = (t - EXIT_APEX) / (1.0 - EXIT_APEX);
    EXIT_PEAK + (-EXIT_SINK - EXIT_PEAK) * u * u
}

/// `spriteClipBottom`: pixels clipped off the sprite as it sinks.
pub fn sprite_clip_bottom(y: f32) -> f32 {
    if y >= 0.0 {
        return 0.0;
    }
    RUNNER_SIZE.min((-y).ceil())
}

/// `obstacleFromRects`: a control sitting on (or just above) the top border
/// is a hurdle.
pub fn obstacle_from_rects(r#box: &Rect, button: Option<&Rect>) -> Option<Obstacle> {
    let button = button?;
    if button.right <= r#box.left || button.left >= r#box.right {
        return None;
    }
    if button.bottom < r#box.top - 48.0 || button.top > r#box.top + 12.0 {
        return None;
    }
    Some(Obstacle {
        left: button.left - r#box.left,
        right: button.right - r#box.left,
        height: JUMP_MIN.max(r#box.top - button.top + JUMP_CLEARANCE),
    })
}

/// `RunnerTrack`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunnerTrack {
    pub left: f32,
    pub top: f32,
    pub width: f32,
}

/// `runnerTrack`: prefer the top edge of a control stacked on the composer.
pub fn runner_track(r#box: &Rect, ledge: Option<&Rect>) -> RunnerTrack {
    match ledge {
        Some(ledge) if ledge.width() > 0.0 => RunnerTrack {
            left: ledge.left,
            top: ledge.top,
            width: ledge.width(),
        },
        _ => RunnerTrack {
            left: r#box.left,
            top: r#box.top,
            width: r#box.width(),
        },
    }
}

/// `Math.round`: halves round up.
fn js_round(x: f32) -> f32 {
    (x + 0.5).floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed() -> Rect {
        Rect {
            width: Some(400.0),
            ..Rect::new(100.0, 500.0, 200.0, 320.0)
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn runs_right_then_flips_and_runs_back() {
        assert_eq!(ping_pong(0.0, 100.0), (0.0, 1));
        assert_eq!(ping_pong(40.0, 100.0), (40.0, 1));
        assert_eq!(ping_pong(100.0, 100.0), (100.0, 1));
        assert_eq!(ping_pong(140.0, 100.0), (60.0, -1));
        let (t, facing) = ping_pong(199.9, 100.0);
        assert_eq!(facing, -1);
        assert!(close(t, 0.1));
        assert_eq!(ping_pong(200.0, 100.0), (0.0, 1));
        assert_eq!(ping_pong(240.0, 100.0), (40.0, 1));
    }

    #[test]
    fn wraps_negative_distance_so_a_restart_still_faces_forward() {
        assert_eq!(ping_pong(-10.0, 100.0), (10.0, -1));
    }

    #[test]
    fn parks_at_the_left_inset_when_the_track_is_too_short() {
        assert_eq!(
            runner_pose(80.0, 8.0, None, &[]),
            RunnerPose {
                x: RUNNER_INSET,
                y: 0.0,
                facing: 1,
                airborne: false
            }
        );
    }

    #[test]
    fn places_x_in_box_coordinates_inset_from_each_end() {
        let right = runner_pose(0.0, 200.0, None, &[]);
        assert_eq!((right.x, right.facing, right.y), (RUNNER_INSET, 1, 0.0));
        let far = runner_pose(200.0 - RUNNER_INSET * 2.0, 200.0, None, &[]);
        assert_eq!((far.x, far.facing), (200.0 - RUNNER_INSET, 1));
    }

    #[test]
    fn jumps_a_parabola_over_the_hurdle_and_lands_after_it() {
        let obstacle = Obstacle {
            left: 80.0,
            right: 104.0,
            height: 40.0,
        };
        let mid = (obstacle.left + obstacle.right) / 2.0;
        assert_eq!(
            jump_height(obstacle.left - JUMP_LEAD, Some(&obstacle), &[]),
            0.0
        );
        assert_eq!(
            jump_height(obstacle.right + JUMP_LEAD, Some(&obstacle), &[]),
            0.0
        );
        assert!(close(jump_height(mid, Some(&obstacle), &[]), 40.0));
        assert!(
            jump_height(mid, Some(&obstacle), &[])
                > jump_height(obstacle.left, Some(&obstacle), &[])
        );
        let peak = runner_pose(mid - RUNNER_INSET, 400.0, Some(&obstacle), &[]);
        assert!(peak.airborne);
        assert!(close(peak.y, 40.0));
        let before = runner_pose(0.0, 400.0, Some(&obstacle), &[]);
        assert!(!before.airborne);
        assert_eq!(before.y, 0.0);
    }

    #[test]
    fn keeps_the_landing_arc_after_the_coin_is_grabbed() {
        let coins = [Coin {
            id: 1,
            x: 120.0,
            height: COIN_HOVER,
        }];
        let at_coin = runner_pose(120.0 - RUNNER_INSET, 400.0, None, &coins);
        let past = runner_pose(120.0 - RUNNER_INSET + 20.0, 400.0, None, &coins);
        assert!(at_coin.y > past.y);
        assert!(past.y > 0.0);
        assert_eq!(
            runner_pose(120.0 - RUNNER_INSET + 20.0, 400.0, None, &[]).y,
            0.0
        );
    }

    #[test]
    fn collects_only_when_the_sprite_overlaps_the_coin() {
        let coin = Coin {
            id: 1,
            x: 120.0,
            height: COIN_HOVER,
        };
        let peak = runner_pose(120.0 - RUNNER_INSET, 400.0, None, &[coin]);
        assert!(coin_collected(&peak, &coin));
        let low = RunnerPose {
            x: 120.0,
            y: 8.0,
            facing: 1,
            airborne: true,
        };
        assert!(!coin_collected(&low, &coin));
        let away = RunnerPose { x: 40.0, ..peak };
        assert!(!coin_collected(&away, &coin));
    }

    #[test]
    fn spaces_coins_across_the_track_and_away_from_the_runner() {
        let values = [0.5, 0.1];
        let mut i = 0;
        let mut random = || {
            let v = values[i.min(1)];
            i += 1;
            v
        };
        let x = pick_coin_x(400.0, 200.0, None, &mut random).unwrap();
        assert!(x > 50.0 && x < 100.0, "{x}");
        assert_eq!(pick_coin_x(40.0, 10.0, None, &mut || 0.5), None);
    }

    #[test]
    fn waits_several_seconds_between_coins() {
        assert_eq!(next_coin_delay(true, &mut || 0.0), COIN_FIRST_MIN_MS);
        assert_eq!(next_coin_delay(true, &mut || 1.0), COIN_FIRST_MAX_MS);
        assert_eq!(next_coin_delay(false, &mut || 0.0), COIN_GAP_MIN_MS);
        assert_eq!(next_coin_delay(false, &mut || 1.0), COIN_GAP_MAX_MS);
        const { assert!(COIN_GAP_MIN_MS >= 6000.0) };
    }

    #[test]
    fn hops_up_then_drops_behind_the_rim_on_the_way_out() {
        assert_eq!(exit_jump_y(0.0), 0.0);
        assert!(close(exit_jump_y(0.38), EXIT_PEAK));
        assert_eq!(exit_jump_y(1.0), -EXIT_SINK);
        assert!(exit_jump_y(0.2) > 0.0 && exit_jump_y(0.2) < EXIT_PEAK);
        assert!(exit_jump_y(0.9) < 0.0);
        assert_eq!(sprite_clip_bottom(8.0), 0.0);
        assert_eq!(sprite_clip_bottom(-4.0), 4.0);
        assert_eq!(sprite_clip_bottom(-40.0), 16.0);
    }

    #[test]
    fn ignores_a_control_that_is_not_sitting_on_the_top_border() {
        assert_eq!(
            obstacle_from_rects(&boxed(), Some(&Rect::new(250.0, 274.0, 40.0, 64.0))),
            None
        );
        assert_eq!(
            obstacle_from_rects(&boxed(), Some(&Rect::new(10.0, 34.0, 188.0, 212.0))),
            None
        );
    }

    #[test]
    fn reads_the_jump_to_latest_chevron_as_a_hurdle_in_box_space() {
        assert_eq!(
            obstacle_from_rects(&boxed(), Some(&Rect::new(288.0, 312.0, 168.0, 192.0))),
            Some(Obstacle {
                left: 188.0,
                right: 212.0,
                height: 42.0
            })
        );
    }

    #[test]
    fn crashes_into_the_chevron_only_on_the_first_approach() {
        let hurdle = Obstacle {
            left: 80.0,
            right: 104.0,
            height: 40.0,
        };
        let half = RUNNER_SIZE / 2.0;
        let h = Some(&hurdle);
        assert!(!hits_chevron(80.0 - half - 2.0, 0.0, 1, h, false));
        assert!(hits_chevron(80.0 - half, 0.0, 1, h, false));
        assert!(!hits_chevron(120.0, 0.0, 1, h, false));
        assert!(hits_chevron(104.0 + half, 0.0, -1, h, false));
        assert!(!hits_chevron(50.0, 0.0, -1, h, false));
        assert!(!hits_chevron(80.0 - half, 8.0, 1, h, false));
        assert!(!hits_chevron(80.0 - half, 0.0, 1, h, true));
        assert!(!hits_chevron(80.0 - half, 0.0, 1, None, false));
    }

    #[test]
    fn knocks_the_mascot_back_shakes_then_finishes_the_stun() {
        assert_eq!(recoil_along(70.0, 1, 0.0, 200.0), 70.0);
        assert_eq!(
            recoil_along(70.0, 1, CRASH_RECOIL_MS, 200.0),
            70.0 - CRASH_RECOIL_PX
        );
        assert_eq!(
            recoil_along(70.0, -1, CRASH_RECOIL_MS, 200.0),
            70.0 + CRASH_RECOIL_PX
        );
        assert_eq!(recoil_along(4.0, 1, CRASH_RECOIL_MS, 200.0), 0.0);
        assert_eq!(recoil_along(190.0, -1, CRASH_RECOIL_MS, 200.0), 200.0);
        assert_eq!(stun_shake(0.0), (0.0, 0.0));
        let wobble: Vec<(f32, f32)> = [40.0, 80.0, 120.0, 160.0]
            .into_iter()
            .map(stun_shake)
            .collect();
        assert!(wobble.iter().any(|s| s.0.abs() >= 2.0));
        assert!(wobble.iter().any(|s| s.1.abs() >= 1.0));
        assert_eq!(stun_shake(CRASH_SHAKE_MS), (0.0, 0.0));
        assert!(!stun_done(CRASH_STUN_MS - 1.0));
        assert!(stun_done(CRASH_STUN_MS));
    }

    #[test]
    fn orbits_pixel_stars_around_the_sprite_for_the_stun_then_clears_them() {
        let start = stun_stars(0.0);
        assert_eq!(start.len(), STAR_COUNT);
        assert_eq!(start[0].opacity, 1.0);
        let mut spots: Vec<(i32, i32)> = start.iter().map(|s| (s.dx as i32, s.dy as i32)).collect();
        spots.dedup();
        assert_eq!(spots.len(), STAR_COUNT);
        assert_ne!(stun_stars(140.0)[0].dx, start[0].dx);
        assert!(stun_stars(CRASH_STUN_MS - 1.0)[0].opacity < 1.0);
        assert!(stun_stars(CRASH_STUN_MS).is_empty());
    }

    #[test]
    fn runs_on_the_review_bar_when_it_is_sitting_on_the_composer() {
        assert_eq!(
            runner_track(&boxed(), None),
            RunnerTrack {
                left: 100.0,
                top: 200.0,
                width: 400.0
            }
        );
        let bar = Rect {
            width: Some(384.0),
            ..Rect::new(108.0, 492.0, 168.0, 200.0)
        };
        assert_eq!(
            runner_track(&boxed(), Some(&bar)),
            RunnerTrack {
                left: 108.0,
                top: 168.0,
                width: 384.0
            }
        );
    }

    #[test]
    fn keeps_relative_position_when_the_track_width_changes() {
        assert_eq!(scale_track_x(200.0, 400.0, 200.0), 100.0);
        assert_eq!(scale_track_x(0.0, 400.0, 800.0), 0.0);
        assert_eq!(scale_track_x(50.0, 0.0, 400.0), 0.0);
        assert_eq!(step_along(0.0, 1, 1000.0, 100.0, 40.0), (40.0, 1));
        assert_eq!(step_along(100.0, 1, 1000.0, 100.0, 40.0), (100.0, -1));
        let pose = pose_at(50.0, 1, 200.0, None, &[]);
        assert_eq!((pose.x, pose.facing, pose.y), (RUNNER_INSET + 50.0, 1, 0.0));
    }
}
