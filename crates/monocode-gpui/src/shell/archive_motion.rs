//! Archiving a thread: its sidebar title dissolves from the archive button
//! across, the rest of the row fades, and the rows below close up behind it.
use super::*;
use crate::particle_text::Dissolve;
use crate::reveal::{Disclosure, bezier};
use std::time::{Duration, Instant};

/// The row fades while its title dissolves, then its height folds away.
const FADE: Duration = Duration::from_millis(336);
const COLLAPSE_AT: Duration = Duration::from_millis(400);
const ROW: Duration = Duration::from_millis(600);
/// The sweep plus the longest a particle lives.
const DUST: Duration = Duration::from_millis(1104);
/// How much of the dust stays over the next row, so its title stays readable.
const DUST_OVER_ROWS: f32 = 0.2;

struct Exit {
    start: Instant,
    /// Painted above the list, so the fold doesn't clip it.
    dust: Dissolve,
    /// The list's width, so a truncated title keeps its cut. Read here because
    /// the list can't be asked while it draws its rows.
    width: Pixels,
    /// The row has been dropped from the list.
    dropped: bool,
}

#[derive(Default)]
pub(crate) struct ArchiveExits {
    exits: HashMap<i64, Exit>,
    frames: Option<gpui::Task<()>>,
}

impl ArchiveExits {
    /// Still drawn by the list, so it can fade and fold instead of vanishing.
    /// Held until the tick drops it, so a frame drawn after the fold but before
    /// the list is rebuilt keeps the row collapsed instead of showing it again.
    pub(super) fn listed(&self, id: i64) -> bool {
        self.exits.get(&id).is_some_and(|exit| !exit.dropped)
    }

    /// How far the row has faded and how far its height has folded, 0 to 1,
    /// with the width it lays out at.
    fn row(&self, id: i64) -> Option<(f32, f32, Pixels)> {
        let exit = self.exits.get(&id).filter(|exit| !exit.dropped)?;
        let elapsed = exit.start.elapsed();
        let fade = (elapsed.as_secs_f32() / FADE.as_secs_f32()).min(1.);
        Some((bezier(fade, 0.4, 0., 1., 1.), fold_at(elapsed), exit.width))
    }

    /// The title of a row on its way out.
    pub(super) fn dissolve(&self, id: i64) -> Option<Dissolve> {
        let exit = self.exits.get(&id).filter(|exit| !exit.dropped)?;
        Some(exit.dust.clone())
    }

    pub(super) fn exiting(&self, id: i64) -> bool {
        self.row(id).is_some()
    }
}

fn fold_at(elapsed: Duration) -> f32 {
    let fold =
        elapsed.saturating_sub(COLLAPSE_AT).as_secs_f32() / (ROW - COLLAPSE_AT).as_secs_f32();
    bezier(fold.min(1.), 0.2, 0., 0., 1.)
}

/// The dust thins as the next row moves up under it.
fn dust_strength(elapsed: Duration) -> f32 {
    1. - (1. - DUST_OVER_ROWS) * fold_at(elapsed)
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
        if self
            .session_row_bounds
            .get(&id)
            .and_then(AnchorSlot::get)
            .is_none()
        {
            return;
        }
        self.archive_exits.exits.insert(
            id,
            Exit {
                start: Instant::now(),
                dust: Dissolve::default(),
                width: self.sidebar_list.state.viewport_bounds().size.width,
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
                    .retain(|_, exit| exit.start.elapsed() < DUST);
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

    /// A sidebar row's title, handed to the dust while its thread leaves.
    pub(super) fn session_title_text(&self, id: i64, title: impl Into<String>) -> impl IntoElement {
        crate::particle_text::particle_text(title, self.archive_exits.dissolve(id))
    }

    /// The row at `ix` as the list shows it, faded and folded while its thread
    /// is on the way out.
    pub(super) fn archive_exit_row(
        &self,
        id: i64,
        row: AnyElement,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((fade, fold, width)) = self.archive_exits.row(id) else {
            return row;
        };
        div()
            .overflow_hidden()
            .child(Disclosure::new(
                div().opacity(1. - fade).child(row),
                width,
                1. - fold,
            ))
            .into_any_element()
    }

    /// Paints the dissolving titles above the list, in window coordinates.
    pub(super) fn archive_dust_overlay(&self) -> Option<AnyElement> {
        let titles: Vec<_> = self
            .archive_exits
            .exits
            .values()
            .map(|exit| (exit.dust.clone(), exit.start.elapsed()))
            .filter(|(_, elapsed)| *elapsed < DUST)
            .collect();
        if titles.is_empty() {
            return None;
        }
        Some(
            canvas(
                |_, _, _| (),
                move |_, _, window, cx| {
                    for (dust, elapsed) in &titles {
                        dust.paint(elapsed.as_secs_f32(), dust_strength(*elapsed), window, cx);
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
                dust: Dissolve::default(),
                width: px(240.),
                dropped: false,
            },
        );
        exits
    }

    #[test]
    fn row_fades_before_it_folds_and_is_unlisted_when_done() {
        let early = exit_at(Duration::from_millis(120)).row(7).unwrap();
        assert!(early.0 > 0. && early.1 == 0.);
        let late = exit_at(Duration::from_millis(500)).row(7).unwrap();
        assert!(late.0 == 1. && late.1 > 0.5);
        // Between the end of the fold and the tick that drops the row, it
        // stays listed and fully collapsed rather than snapping back.
        let held = exit_at(ROW + Duration::from_millis(5));
        assert!(held.listed(7));
        assert_eq!(held.row(7), Some((1., 1., px(240.))));
        let mut done = held;
        done.exits.get_mut(&7).unwrap().dropped = true;
        assert!(!done.listed(7) && done.row(7).is_none());
        assert!(done.dissolve(7).is_none());
        assert!(!exit_at(Duration::ZERO).listed(8));
    }

    #[test]
    fn dust_outlives_the_row_and_thins_as_the_next_row_arrives() {
        assert!(DUST > ROW);
        assert_eq!(dust_strength(COLLAPSE_AT), 1.);
        assert!((dust_strength(ROW) - DUST_OVER_ROWS).abs() < 0.001);
        assert!((dust_strength(DUST) - DUST_OVER_ROWS).abs() < 0.001);
    }
}
