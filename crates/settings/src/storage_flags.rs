//! Port of src/features/settings/model/storageFlags.ts: on and off settings
//! stored as `"1"` and `"0"`.

use monocode_core::settings;

use crate::kv::Kv;

/// `readFlag`: `"1"` and `"true"` are on, any other stored string is off,
/// and a missing key is `None`.
pub fn read_flag(kv: &Kv, key: &str) -> Option<bool> {
    settings::read_flag(kv.get_item(key).as_deref())
}

/// `writeFlag`.
pub fn write_flag(kv: &Kv, key: &str, value: bool) {
    kv.set_item(key, settings::write_flag(value));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_one_and_zero_and_reads_them_back() {
        let kv = Kv::in_memory();
        assert_eq!(read_flag(&kv, "monocode.x"), None);
        write_flag(&kv, "monocode.x", true);
        assert_eq!(kv.get_item("monocode.x").as_deref(), Some("1"));
        assert_eq!(read_flag(&kv, "monocode.x"), Some(true));
        write_flag(&kv, "monocode.x", false);
        assert_eq!(kv.get_item("monocode.x").as_deref(), Some("0"));
        assert_eq!(read_flag(&kv, "monocode.x"), Some(false));
        kv.set_item("monocode.x", "true");
        assert_eq!(read_flag(&kv, "monocode.x"), Some(true));
    }
}
