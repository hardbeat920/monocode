//! `crypto.randomUUID()`, which the TypeScript called for every new tab,
//! pane, split, and tab group id.

/// A random version 4 UUID in the lowercase hyphenated form that
/// `crypto.randomUUID()` returns.
pub fn random_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_browser_format() {
        let id = random_uuid();
        assert_eq!(id.len(), 36);
        assert_eq!(id, id.to_lowercase());
        assert_eq!(id.as_bytes()[14], b'4');
        assert_ne!(random_uuid(), id);
    }
}
