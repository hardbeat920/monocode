//! Archiving a thread: its sidebar row slides off to the left, the rows below
//! close up behind it, and a small burst leaves the archive button.
use super::*;
use crate::reveal::{Disclosure, bezier};
use gpui::{Hsla, point};
use std::time::{Duration, Instant};

/// The row slides and fades, then its height folds away.
const SLIDE: Duration = Duration::from_millis(170);
const COLLAPSE_AT: Duration = Duration::from_millis(110);
const ROW: Duration = Duration::from_millis(300);
const BURST: Duration = Duration::from_millis(640);
const PARTICLES: usize = 10;
const PALETTE: [u32; 5] = [ACCENT, 0xfbbf24, 0x6ee7b7, 0xfda4af, 0xb180d7];

struct Exit {
    start: Instant,
    /// Where the burst leaves from, in window coordinates.
    origin: Point<Pixels>,
    /// The row has been dropped from the list.
    dropped: bool,
}

#[derive(Default)]
pub(crate) struct ArchiveExits {
    exits: HashMap<i64, Exit>,
    frames: Option<gpui::Task<()>>,
}

impl ArchiveExits {
    /// Still drawn by the list, so it can slide and fold instead of vanishing.
    /// Held until the tick drops it, so a frame drawn after the fold but before
    /// the list is rebuilt keeps the row collapsed instead of showing it again.
    pub(super) fn listed(&self, id: i64) -> bool {
        self.exits.get(&id).is_some_and(|exit| !exit.dropped)
    }

    /// How far the row has slid and how far its height has folded, 0 to 1.
    fn row(&self, id: i64) -> Option<(f32, f32)> {
        let exit = self.exits.get(&id).filter(|exit| !exit.dropped)?;
        let elapsed = exit.start.elapsed().min(ROW);
        let slide = (elapsed.as_secs_f32() / SLIDE.as_secs_f32()).min(1.);
        let fold = (elapsed.saturating_sub(COLLAPSE_AT).as_secs_f32()
            / (ROW - COLLAPSE_AT).as_secs_f32())
        .min(1.);
        Some((
            bezier(slide, 0.4, 0., 1., 1.),
            bezier(fold, 0.2, 0., 0., 1.),
        ))
    }

    pub(super) fn exiting(&self, id: i64) -> bool {
        self.row(id).is_some()
    }
}

/// A stable pseudo-random value in `0..1` for one particle's trait.
fn jitter(id: i64, particle: usize, salt: u64) -> f32 {
    let mut x = (id as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((particle as u64 + 1).wrapping_mul(0xBF58_476D_1CE4_E5B9))
        .wrapping_add(salt.wrapping_mul(0x94D0_49BB_1331_11EB));
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

struct Particle {
    center: Point<Pixels>,
    size: f32,
    color: Hsla,
}

fn particles(id: i64, origin: Point<Pixels>, elapsed: Duration) -> Vec<Particle> {
    let t = (elapsed.as_secs_f32() / BURST.as_secs_f32()).min(1.);
    let travel = bezier(t, 0., 0., 0.2, 1.);
    let fade = 1. - bezier(((t - 0.45) / 0.55).clamp(0., 1.), 0.4, 0., 1., 1.);
    (0..PARTICLES)
        .map(|k| {
            // A fan thrown up and left, towards where the row is heading.
            let angle = std::f32::consts::PI * (0.55 + 0.9 * jitter(id, k, 1));
            let reach = 26. + 34. * jitter(id, k, 2);
            let fall = 18. * t * t * (0.6 + jitter(id, k, 3));
            let mut color: Hsla =
                rgb(PALETTE[(jitter(id, k, 4) * PALETTE.len() as f32) as usize % PALETTE.len()])
                    .into();
            color.a = fade;
            Particle {
                center: point(
                    origin.x + px(angle.cos() * reach * travel),
                    origin.y + px(-angle.sin() * reach * travel + fall),
                ),
                size: 2.5 + 2.5 * jitter(id, k, 5),
                color,
            }
        })
        .collect()
}

impl Shell {
    /// Follows a thread's archived state once it is written: archiving plays
    /// the exit, and a restore that overtakes a running exit ends it so the
    /// row draws as a normal row again.
    pub(super) fn sync_archive_exit(&mut self, id: i64, archived: bool, cx: &mut Context<Self>) {
        if archived {
            self.start_archive_exit(id, cx);
        } else if self.archive_exits.exits.remove(&id).is_some() {
            // The list may still hold the row at its folded height.
            self.sidebar_list.invalidate_structure();
            self.sidebar_list.remeasure();
            self.sidebar_frame.update(cx, |_, cx| cx.notify());
        }
    }

    /// Starts the exit for a thread that was just archived. Rows that are not
    /// on screen, or a reduced-motion setting, simply drop out of the list.
    fn start_archive_exit(&mut self, id: i64, cx: &mut Context<Self>) {
        if cx.reduce_motion() || self.workspace.preferences.show_archived {
            return;
        }
        let Some(bounds) = self.session_row_bounds.get(&id).and_then(AnchorSlot::get) else {
            return;
        };
        self.archive_exits.exits.insert(
            id,
            Exit {
                start: Instant::now(),
                origin: point(bounds.right() - px(24.), bounds.center().y),
                dropped: false,
            },
        );
        self.tick_archive_exits(cx);
    }

    fn tick_archive_exits(&mut self, cx: &mut Context<Self>) {
        if self.archive_exits.frames.is_some() {
            return;
        }
        self.archive_exits.frames = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.archive_exits.frames = None;
                let mut dropped = false;
                for exit in this.archive_exits.exits.values_mut() {
                    if !exit.dropped && exit.start.elapsed() >= ROW {
                        exit.dropped = true;
                        dropped = true;
                    }
                }
                this.archive_exits
                    .exits
                    .retain(|_, exit| exit.start.elapsed() < BURST);
                if dropped {
                    // The list forgets the row once, after it has folded away.
                    this.sidebar_list.invalidate_structure();
                }
                this.sidebar_frame.update(cx, |_, cx| cx.notify());
                if !this.archive_exits.exits.is_empty() {
                    this.tick_archive_exits(cx);
                }
            });
        }));
    }

    /// The row at `ix` as the list shows it, slid and folded while its thread
    /// is on the way out.
    pub(super) fn archive_exit_row(
        &self,
        id: i64,
        row: AnyElement,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((slide, fold)) = self.archive_exits.row(id) else {
            return row;
        };
        let width = px(self.workspace.sidebar_width.max(0.));
        div()
            .overflow_hidden()
            .child(Disclosure::new(
                div()
                    .relative()
                    .left(-width * 0.4 * slide)
                    .opacity(1. - slide)
                    .child(row),
                width,
                1. - fold,
            ))
            .into_any_element()
    }

    /// Paints the bursts above the list, in window coordinates.
    pub(super) fn archive_burst_overlay(&self) -> Option<AnyElement> {
        let bursts: Vec<_> = self
            .archive_exits
            .exits
            .iter()
            .filter(|(_, exit)| exit.start.elapsed() < BURST)
            .flat_map(|(id, exit)| particles(*id, exit.origin, exit.start.elapsed()))
            .collect();
        if bursts.is_empty() {
            return None;
        }
        Some(
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    for particle in &bursts {
                        let size = px(particle.size);
                        window.paint_quad(gpui::quad(
                            Bounds::centered_at(particle.center, gpui::size(size, size)),
                            size / 2.,
                            particle.color,
                            0.,
                            gpui::transparent_black(),
                            gpui::BorderStyle::default(),
                        ));
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exit_at(age: Duration) -> ArchiveExits {
        let mut exits = ArchiveExits::default();
        exits.exits.insert(
            7,
            Exit {
                start: Instant::now() - age,
                origin: point(px(100.), px(50.)),
                dropped: false,
            },
        );
        exits
    }

    #[test]
    fn row_slides_before_it_folds_and_is_unlisted_when_done() {
        let early = exit_at(Duration::from_millis(60)).row(7).unwrap();
        assert!(early.0 > 0. && early.1 == 0.);
        let late = exit_at(Duration::from_millis(250)).row(7).unwrap();
        assert!(late.0 >= 0.99 && late.1 > 0.5);
        // Between the end of the fold and the tick that drops the row, it
        // stays listed and fully collapsed rather than snapping back.
        let held = exit_at(ROW + Duration::from_millis(5));
        assert!(held.listed(7));
        assert_eq!(held.row(7), Some((1., 1.)));
        let mut done = held;
        done.exits.get_mut(&7).unwrap().dropped = true;
        assert!(!done.listed(7) && done.row(7).is_none());
        assert!(!exit_at(Duration::ZERO).listed(8));
    }

    #[test]
    fn burst_is_stable_and_fades_out() {
        let origin = point(px(100.), px(50.));
        let a = particles(7, origin, Duration::from_millis(200));
        let b = particles(7, origin, Duration::from_millis(200));
        assert_eq!(a.len(), PARTICLES);
        assert!(a.iter().zip(&b).all(|(x, y)| x.center == y.center));
        assert!(a.iter().all(|p| p.color.a > 0.));
        assert!(particles(7, origin, BURST).iter().all(|p| p.color.a < 0.01));
    }
}
