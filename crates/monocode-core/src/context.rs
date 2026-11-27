//! Context-window accounting, matching sessions/model/contextUsage.ts.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub used: u64,
    pub window: Option<u64>,
}
impl Usage {
    pub fn ratio(self) -> Option<f32> {
        self.window
            .filter(|w| *w > 0)
            .map(|w| (self.used as f64 / w as f64).min(1.) as f32)
    }
    pub fn merge(&mut self, used: Option<u64>, window: Option<u64>) {
        if let Some(used) = used {
            self.used = used;
        }
        if let Some(window) = window.filter(|w| *w > 0) {
            self.window = Some(window);
        }
    }
}

pub fn claude_used(usage: &Value) -> Option<u64> {
    let used = [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
        "output_tokens",
    ]
    .iter()
    .map(|key| usage[key].as_u64().unwrap_or(0))
    .sum();
    (used > 0).then_some(used)
}

pub fn format_tokens(count: u64) -> String {
    let (value, suffix) = if count >= 1_000_000 {
        (count as f64 / 1_000_000., "M")
    } else if count >= 1000 {
        (count as f64 / 1000., "K")
    } else {
        return count.to_string();
    };
    if value < 10. {
        format!(
            "{}{suffix}",
            format!("{value:.1}")
                .trim_end_matches('0')
                .trim_end_matches('.')
        )
    } else {
        format!("{value:.0}{suffix}")
    }
}
