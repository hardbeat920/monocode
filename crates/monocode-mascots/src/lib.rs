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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_preserves_saved_names_hashes_and_eight_pixel_art() {
        assert_eq!(project_hash("A😀"), 8_424_064); // Legacy JS UTF-16 hash.
        assert_eq!(project_index("A😀", Some("unknown")), 4);
        for (i, mascot) in CATALOG.iter().enumerate() {
            assert_eq!(mascot.id.index(), i);
            assert_eq!(MascotId::find(MASCOTS[i]), Some(mascot.id));
            assert_eq!(project_index("any project", Some(MASCOTS[i])), i);
            for frame in [&mascot.rest, &mascot.talk] {
                assert!(frame
                    .rows
                    .iter()
                    .all(|row| row.len() == 8
                        && row.bytes().all(|pixel| matches!(pixel, b'.' | b'#'))));
                assert!(frame.eyes.iter().all(|[x, y]| *x < 8 && *y < 8));
            }
        }
    }
}
