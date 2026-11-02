//! Shared native mascot artwork, selection and composer animation.
mod catalog;
pub mod composer;
pub mod random;
pub use catalog::{Frame, Mascot, MascotId, CATALOG, MASCOTS};

#[cfg(feature = "gpui")]
pub mod gpui;

/// JavaScript-compatible UTF-16 hash; callers supply the seed unchanged.
pub fn project_hash(seed: &str) -> u32 {
    seed.encode_utf16().fold(0u32, |hash, ch| {
        hash.wrapping_mul(131).wrapping_add(u32::from(ch))
    })
}

pub fn project_index(seed: &str, explicit: Option<&str>) -> usize {
    explicit.and_then(MascotId::find).map_or_else(
        || project_hash(seed) as usize % MASCOTS.len(),
        MascotId::index,
    )
}
