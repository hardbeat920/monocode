//! The live agents panel list: the `liveAgents` memo in App.tsx (lines
//! 1677-1683). The rows themselves are attention's port of
//! src/features/sessions/model/liveAgents.ts, re-exported here.

use gpui::App;

pub use crate::attention::live_agents::{
    LiveAgent, format_live_elapsed, live_agents_from_sessions,
};
use crate::runtime::engine::Engine;

/// Working sessions plus finished ones the user has not seen yet, or none
/// when the "live agents" setting is off.
pub fn live_agents(cx: &App) -> Vec<LiveAgent> {
    let hooks = Engine::hooks(cx);
    if !hooks.attention.live_agents_enabled(cx) {
        return Vec::new();
    }
    let unseen = hooks.attention.unseen_finished_ids(cx);
    live_agents_from_sessions(Engine::sessions(cx).read(cx).all(), &unseen)
}
