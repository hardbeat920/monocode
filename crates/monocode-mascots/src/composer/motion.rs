//! Composer motion and effect geometry.
use crate::random::Random;

pub const RUNNER_SIZE: f64 = 16.;
pub const RUNNER_SPEED_PX: f64 = 160.;
pub const RUNNER_INSET: f64 = 10.;
pub const JUMP_LEAD: f64 = 18.;
pub const COIN_SIZE: f64 = 12.;
pub const COIN_HOVER: f64 = 42.;
pub const COIN_WIDTH: f64 = 8.;
pub const COIN_JUMP_LEAD: f64 = 34.;
pub const COIN_GAP_MIN_MS: f64 = 7000.;
pub const COIN_GAP_MAX_MS: f64 = 18000.;
pub const COIN_FIRST_MIN_MS: f64 = 3500.;
pub const COIN_FIRST_MAX_MS: f64 = 9000.;
pub const COLLECT_X: f64 = 10.;
pub const COLLECT_POP_MS: f64 = 280.;
pub const COLLECT_POP_PX: f64 = 16.;
pub const EXIT_MS: f64 = 560.;
pub const EXIT_PEAK: f64 = 44.;
pub const EXIT_SINK: f64 = 20.;
pub const CRASH_RECOIL_PX: f64 = 18.;
pub const CRASH_RECOIL_MS: f64 = 140.;
pub const CRASH_STUN_MS: f64 = 560.;
pub const CRASH_SHAKE_MS: f64 = 480.;
pub const STAR_SIZE: f64 = 8.;
pub const STAR_COUNT: usize = 3;
pub const STAR_ORBIT: f64 = 11.;
pub const STAR_SPIN_MS: f64 = 520.;

pub const COIN_FACE: [&str; 8] = [
    "........", "..####..", ".######.", "########", "########", ".######.", "..####..", "........",
];
pub const STAR_FACE: [&str; 8] = [
    "...##...", "...##...", "..####..", "########", "########", "..####..", "...##...", "...##...",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i8)]
pub enum Facing {
    Left = -1,
    #[default]
    Right = 1,
}
impl Facing {
    pub const fn sign(self) -> f64 {
        self as i8 as f64
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    pub facing: Facing,
    pub airborne: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obstacle {
    pub left: f64,
    pub right: f64,
    pub height: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coin {
    pub x: f64,
    pub height: f64,
    pub collected_at: Option<f64>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Star {
    pub dx: f64,
    pub dy: f64,
    pub opacity: f64,
}

fn arc(x: f64, left: f64, right: f64, height: f64, lead: f64) -> f64 {
    let start = left - lead;
    let end = right + lead;
    if end <= start || x <= start || x >= end {
        return 0.;
    }
    let t = (x - start) / (end - start);
    4. * t * (1. - t) * height
}
pub fn coin_jump_peak(coin: Coin) -> f64 {
    (coin.height - RUNNER_SIZE / 2.).max(0.)
}
pub fn jump_height(x: f64, obstacle: Option<Obstacle>, coins: &[Coin]) -> f64 {
    let mut height = obstacle.map_or(0., |o| arc(x, o.left, o.right, o.height, JUMP_LEAD));
    for coin in coins {
        height = height.max(arc(
            x,
            coin.x - COIN_WIDTH / 2.,
            coin.x + COIN_WIDTH / 2.,
            coin_jump_peak(*coin),
            COIN_JUMP_LEAD,
        ));
    }
    height
}
pub fn scale_track_x(x: f64, from_width: f64, to_width: f64) -> f64 {
    if from_width <= 0. {
        0.
    } else {
        x * (to_width / from_width)
    }
}
pub fn step_along(along: f64, facing: Facing, dt_ms: f64, track: f64, speed: f64) -> (f64, Facing) {
    if track <= 0. {
        return (0., Facing::Right);
    }
    let next = along + facing.sign() * speed * (dt_ms / 1000.);
    if next >= track {
        (track, Facing::Left)
    } else if next <= 0. {
        (0., Facing::Right)
    } else {
        (next, facing)
    }
}
pub fn pose_at(
    along: f64,
    facing: Facing,
    width: f64,
    obstacle: Option<Obstacle>,
    coins: &[Coin],
) -> Pose {
    let track = (width - RUNNER_INSET * 2.).max(0.);
    let x = RUNNER_INSET + track.min(along.max(0.));
    let y = jump_height(x, obstacle, coins);
    Pose {
        x,
        y,
        facing,
        airborne: y > 0.5,
    }
}
pub fn hits_chevron(pose: Pose, obstacle: Option<Obstacle>, learned: bool) -> bool {
    let Some(o) = obstacle else {
        return false;
    };
    if learned || pose.y > 0.5 {
        return false;
    }
    let half = RUNNER_SIZE / 2.;
    match pose.facing {
        Facing::Right => pose.x + half >= o.left && pose.x - half < o.right,
        Facing::Left => pose.x - half <= o.right && pose.x + half > o.left,
    }
}
pub fn recoil_along(hit: f64, facing: Facing, elapsed_ms: f64, track: f64) -> f64 {
    let t = (elapsed_ms / CRASH_RECOIL_MS).clamp(0., 1.);
    let eased = 1. - (1. - t) * (1. - t);
    track.min((hit - facing.sign() * CRASH_RECOIL_PX * eased).max(0.))
}
// Math.round's negative half ties differ from Rust's f64::round.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}
pub fn stun_shake(elapsed_ms: f64) -> (f64, f64) {
    if elapsed_ms <= 0. || elapsed_ms >= CRASH_SHAKE_MS {
        return (0., 0.);
    }
    let decay = 1. - elapsed_ms / CRASH_SHAKE_MS;
    (
        js_round((elapsed_ms / 32.).sin() * 3. * decay),
        js_round((elapsed_ms / 26.).cos() * 2. * decay),
    )
}
pub fn stun_done(elapsed_ms: f64) -> bool {
    elapsed_ms >= CRASH_STUN_MS
}
pub fn write_stun_stars(elapsed_ms: f64, stars: &mut Vec<Star>) {
    stars.clear();
    if elapsed_ms < 0. || elapsed_ms >= CRASH_STUN_MS {
        return;
    }
    let fade_at = CRASH_STUN_MS - 140.;
    let opacity = if elapsed_ms < fade_at {
        1.
    } else {
        (1. - (elapsed_ms - fade_at) / 140.).max(0.)
    };
    let origin_x = (RUNNER_SIZE - STAR_SIZE) / 2.;
    let origin_y = (RUNNER_SIZE - STAR_SIZE) / 2. - 5.;
    let angle = (elapsed_ms / STAR_SPIN_MS) * std::f64::consts::PI * 2.;
    for i in 0..STAR_COUNT {
        let a = angle + (i as f64 * (std::f64::consts::PI * 2.)) / STAR_COUNT as f64;
        stars.push(Star {
            dx: js_round(origin_x + a.cos() * STAR_ORBIT),
            dy: js_round(origin_y + a.sin() * STAR_ORBIT),
            opacity,
        });
    }
}
pub fn coin_collected(pose: Pose, coin: Coin) -> bool {
    if (pose.x - coin.x).abs() > COLLECT_X {
        return false;
    }
    pose.y + RUNNER_SIZE >= coin.height - COIN_SIZE / 2. && pose.y <= coin.height + COIN_SIZE / 2.
}
pub fn next_coin_delay(first: bool, random: &mut dyn Random) -> f64 {
    let (min, max) = if first {
        (COIN_FIRST_MIN_MS, COIN_FIRST_MAX_MS)
    } else {
        (COIN_GAP_MIN_MS, COIN_GAP_MAX_MS)
    };
    min + random.next_f64() * (max - min)
}
pub fn pick_coin_x(
    width: f64,
    runner_x: f64,
    obstacle: Option<Obstacle>,
    random: &mut dyn Random,
) -> Option<f64> {
    let min = RUNNER_INSET + COIN_JUMP_LEAD + 8.;
    let max = width - RUNNER_INSET - COIN_JUMP_LEAD - 8.;
    if max <= min {
        return None;
    }
    for _ in 0..8 {
        let x = min + random.next_f64() * (max - min);
        if (x - runner_x).abs() < 40. {
            continue;
        }
        if obstacle.is_some_and(|o| x >= o.left - 6. && x <= o.right + 6.) {
            continue;
        }
        return Some(x);
    }
    Some(min + random.next_f64() * (max - min))
}
pub fn exit_jump_y(t: f64) -> f64 {
    if t <= 0. {
        return 0.;
    }
    if t >= 1. {
        return -EXIT_SINK;
    }
    if t < 0.38 {
        let u = t / 0.38;
        EXIT_PEAK * (1. - (1. - u) * (1. - u))
    } else {
        let u = (t - 0.38) / (1. - 0.38);
        EXIT_PEAK + (-EXIT_SINK - EXIT_PEAK) * u * u
    }
}
