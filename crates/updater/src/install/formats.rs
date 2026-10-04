//! Magic-byte checks for the package formats, the same tests
//! tauri-plugin-updater ran through the `infer` crate. Each platform uses a
//! subset, and the tests use them all.
#![cfg_attr(not(test), allow(dead_code))]

/// gzip, for `.app.tar.gz` and `.AppImage.tar.gz`.
pub(crate) fn is_gz(bytes: &[u8]) -> bool {
    bytes.len() > 2 && bytes[0] == 0x1F && bytes[1] == 0x8B && bytes[2] == 0x08
}

/// A Windows executable (`MZ`).
pub(crate) fn is_exe(bytes: &[u8]) -> bool {
    bytes.len() > 1 && bytes[0] == 0x4D && bytes[1] == 0x5A
}

/// An MSI (OLE compound file).
pub(crate) fn is_msi(bytes: &[u8]) -> bool {
    bytes.len() > 7 && bytes[..8] == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]
}

/// A Debian package.
pub(crate) fn is_deb(bytes: &[u8]) -> bool {
    bytes.len() > 20 && &bytes[..21] == b"!<arch>\ndebian-binary"
}

/// An RPM package.
pub(crate) fn is_rpm(bytes: &[u8]) -> bool {
    bytes.len() > 96 && bytes[..4] == [0xED, 0xAB, 0xEE, 0xDB]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_each_format() {
        assert!(is_gz(&[0x1F, 0x8B, 0x08, 0x00]));
        assert!(!is_gz(&[0x1F, 0x8B]));
        assert!(is_exe(b"MZ\x90\x00"));
        assert!(!is_exe(b"M"));
        assert!(is_msi(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0]));
        assert!(is_deb(b"!<arch>\ndebian-binary   1234"));
        assert!(!is_deb(b"!<arch>\nother"));
        let mut rpm = vec![0u8; 100];
        rpm[..4].copy_from_slice(&[0xED, 0xAB, 0xEE, 0xDB]);
        assert!(is_rpm(&rpm));
        assert!(!is_rpm(&rpm[..50]));
    }
}
