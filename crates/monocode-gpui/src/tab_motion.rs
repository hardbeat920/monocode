//! useTabCloseMotion.ts with ClosingTab.tsx's TabWidthMotion: with Tab
//! animations on, an opened tab's slot grows from nothing and a closed one
//! collapses in place before it leaves the strip.
use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

/// `--motion-tab-close-duration`.
pub(crate) const DURATION: Duration = Duration::from_millis(200);
/// The strips' 2px gap, which a collapsed slot gives back (`-0.125rem`).
pub(crate) const GAP: f32 = 2.;

/// `--motion-tab-ease-out`, cubic-bezier(1/3, 2/3, 2/3, 1): quadratic ease-out.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    1. - (1. - t) * (1. - t)
}

/// How far a motion that began at `start` has eased, 0 to 1.
pub(crate) fn progress(start: Instant, now: Instant) -> f32 {
    ease(now.duration_since(start).as_secs_f32() / DURATION.as_secs_f32())
}

/// A slot in the strip, in display order.
pub(crate) enum Slot<K, G> {
    /// A live tab; `open` is its slot's share of its width, 1 once settled.
    Live { key: K, open: f32 },
    /// A closed tab collapsing, drawn from what it last showed.
    Closing { key: K, ghost: G, open: f32 },
}

pub(crate) struct TabMotion<K, G> {
    /// The last strip, live and closing, in display order.
    order: Vec<K>,
    live: Vec<K>,
    ghosts: HashMap<K, G>,
    opening: HashMap<K, Instant>,
    closing: HashMap<K, (G, Instant)>,
}
impl<K, G> Default for TabMotion<K, G> {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            live: Vec::new(),
            ghosts: HashMap::new(),
            opening: HashMap::new(),
            closing: HashMap::new(),
        }
    }
}

impl<K: Clone + Eq + Hash, G: Clone> TabMotion<K, G> {
    /// Replace the content identity of a live slot without opening or closing
    /// it. An existing entrance keeps its original start time.
    pub(crate) fn replace_key(&mut self, from: &K, to: K) {
        if from == &to || !self.live.contains(from) {
            return;
        }
        // The destination may still have a ghost from an earlier close.
        self.order.retain(|key| key != &to);
        self.live.retain(|key| key != &to);
        for key in self.order.iter_mut().chain(self.live.iter_mut()) {
            if key == from {
                *key = to.clone();
            }
        }
        self.ghosts.remove(&to);
        self.opening.remove(&to);
        self.closing.remove(&to);
        if let Some(ghost) = self.ghosts.remove(from) {
            self.ghosts.insert(to.clone(), ghost);
        }
        if let Some(start) = self.opening.remove(from) {
            self.opening.insert(to.clone(), start);
        }
        if let Some(closing) = self.closing.remove(from) {
            self.closing.insert(to, closing);
        }
    }

    /// The strip to draw for `tabs`, each with what a closing copy would show.
    /// Returns whether a slot is still moving, so the caller asks for a frame.
    pub(crate) fn sync(
        &mut self,
        tabs: Vec<(K, G)>,
        animate: bool,
        now: Instant,
    ) -> (Vec<Slot<K, G>>, bool) {
        let keys: Vec<K> = tabs.iter().map(|(k, _)| k.clone()).collect();
        if keys != self.live {
            let previous = std::mem::take(&mut self.live);
            // Switching projects replaces every tab; that is not a close.
            let replace_all = !previous.is_empty()
                && !keys.is_empty()
                && previous.iter().all(|k| !keys.contains(k));
            if replace_all || !animate {
                self.opening.clear();
                self.closing.clear();
                self.order = keys.clone();
            } else {
                for key in &previous {
                    if !keys.contains(key)
                        && let Some(ghost) = self.ghosts.get(key)
                    {
                        self.closing.insert(key.clone(), (ghost.clone(), now));
                    }
                }
                for key in &keys {
                    // Reopened while it was collapsing: it is live again.
                    self.closing.remove(key);
                    if !previous.is_empty() && !previous.contains(key) {
                        self.opening.entry(key.clone()).or_insert(now);
                    }
                }
                self.order =
                    order_with_closing(&self.order, &keys, |k| self.closing.contains_key(k));
            }
            self.live = keys;
        }
        self.ghosts = tabs.into_iter().collect();
        let progress =
            |start: Instant| ease(now.duration_since(start).as_secs_f32() / DURATION.as_secs_f32());
        self.opening.retain(|_, start| progress(*start) < 1.);
        self.closing.retain(|_, (_, start)| progress(*start) < 1.);
        self.order
            .retain(|k| self.ghosts.contains_key(k) || self.closing.contains_key(k));
        let slots: Vec<_> = self
            .order
            .iter()
            .filter_map(|key| {
                if let Some((ghost, start)) = self.closing.get(key) {
                    return Some(Slot::Closing {
                        key: key.clone(),
                        ghost: ghost.clone(),
                        open: 1. - progress(*start),
                    });
                }
                self.ghosts.contains_key(key).then(|| Slot::Live {
                    key: key.clone(),
                    open: self.opening.get(key).map_or(1., |start| progress(*start)),
                })
            })
            .collect();
        (slots, !self.opening.is_empty() || !self.closing.is_empty())
    }
}

/// `orderWithClosingTabs`: closing tabs keep their place after the live tab
/// that preceded them.
fn order_with_closing<K: Clone + Eq>(
    old: &[K],
    live: &[K],
    closing: impl Fn(&K) -> bool,
) -> Vec<K> {
    let mut order = live.to_vec();
    for (old_ix, key) in old.iter().enumerate() {
        if !closing(key) || order.contains(key) {
            continue;
        }
        let at = old[..old_ix]
            .iter()
            .rev()
            .find_map(|previous| order.iter().position(|k| k == previous))
            .map_or(0, |ix| ix + 1);
        order.insert(at, key.clone());
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(slots: &[Slot<u8, ()>]) -> Vec<(char, u8, f32)> {
        slots
            .iter()
            .map(|s| match s {
                Slot::Live { key, open } => ('l', *key, *open),
                Slot::Closing { open, .. } => ('c', 0, *open),
            })
            .collect()
    }

    #[test]
    fn replacing_a_live_slot_keeps_it_settled_and_preserves_other_motions() {
        let mut m = TabMotion::<u8, ()>::default();
        let now = Instant::now();
        m.sync(vec![(1, ()), (2, ())], true, now);
        m.replace_key(&2, 3);
        let (slots, moving) = m.sync(vec![(1, ()), (3, ())], true, now);
        assert_eq!(keys(&slots), [('l', 1, 1.), ('l', 3, 1.)]);
        assert!(!moving);

        // A real close/open elsewhere keeps animating during a replacement.
        m.sync(vec![(3, ()), (4, ())], true, now);
        m.replace_key(&3, 5);
        let (slots, moving) = m.sync(vec![(5, ()), (4, ())], true, now + DURATION / 2);
        assert!(moving);
        assert_eq!(keys(&slots), [('c', 0, 0.25), ('l', 5, 1.), ('l', 4, 0.75)]);

        // Replacing a newly opened slot continues its existing entrance.
        m.replace_key(&4, 6);
        let (slots, moving) = m.sync(vec![(5, ()), (6, ())], true, now + DURATION);
        assert_eq!(keys(&slots), [('l', 5, 1.), ('l', 6, 1.)]);
        assert!(!moving);
    }

    #[test]
    fn replacing_a_slot_can_reuse_a_session_that_is_still_closing() {
        let mut m = TabMotion::<u8, ()>::default();
        let now = Instant::now();
        m.sync(vec![(1, ()), (2, ())], true, now);
        m.sync(vec![(1, ())], true, now);
        m.replace_key(&1, 2);
        let (slots, moving) = m.sync(vec![(2, ())], true, now);
        assert_eq!(keys(&slots), [('l', 2, 1.)]);
        assert!(!moving);
    }

    #[test]
    fn opened_tabs_grow_closed_tabs_collapse_in_place_and_both_settle() {
        let mut m = TabMotion::<u8, ()>::default();
        let t0 = Instant::now();
        let (slots, moving) = m.sync(vec![(1, ()), (2, ())], true, t0);
        // The first strip never animates in.
        assert_eq!(keys(&slots), [('l', 1, 1.), ('l', 2, 1.)]);
        assert!(!moving);
        let (slots, moving) = m.sync(vec![(1, ()), (2, ()), (3, ())], true, t0);
        assert_eq!(keys(&slots)[2], ('l', 3, 0.));
        assert!(moving);
        let half = t0 + DURATION / 2;
        let (slots, _) = m.sync(vec![(2, ()), (3, ())], true, half);
        // Tab 1 collapses where it was, ahead of 2.
        assert_eq!(keys(&slots)[0], ('c', 0, 1.));
        assert!((keys(&slots)[2].2 - 0.75).abs() < 1e-4);
        let (slots, moving) = m.sync(vec![(2, ()), (3, ())], true, half + DURATION);
        assert_eq!(keys(&slots), [('l', 2, 1.), ('l', 3, 1.)]);
        assert!(!moving);
    }

    #[test]
    fn disabled_or_replaced_strips_change_at_once() {
        let mut m = TabMotion::<u8, ()>::default();
        let now = Instant::now();
        m.sync(vec![(1, ())], false, now);
        let (slots, moving) = m.sync(vec![(1, ()), (2, ())], false, now);
        assert_eq!(keys(&slots), [('l', 1, 1.), ('l', 2, 1.)]);
        assert!(!moving);
        let (slots, moving) = m.sync(vec![(7, ()), (8, ())], true, now);
        assert_eq!(keys(&slots), [('l', 7, 1.), ('l', 8, 1.)]);
        assert!(!moving);
    }
}
