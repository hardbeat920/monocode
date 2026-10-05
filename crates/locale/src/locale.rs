use crate::{api::Api, check, length, LocaleError};
use std::{
    ffi::{c_char, c_void, CStr, CString},
    ptr::NonNull,
    sync::OnceLock,
};

#[derive(Clone, Copy)]
pub enum Service {
    Collation,
    Relative,
}

pub fn from_language_tag(api: &Api, tag: &str) -> Result<CString, LocaleError> {
    if !has_unicode_language_prefix(tag) {
        return Err(LocaleError::InvalidLocale);
    }
    let tag_len = length(tag.len())?;
    let tag = CString::new(tag).map_err(|_| LocaleError::InvalidLocale)?;
    let mut parsed = 0;
    let locale = char_output(|buffer, capacity, status| unsafe {
        (api.uloc_forLanguageTag)(tag.as_ptr(), buffer, capacity, &mut parsed, status)
    })?;
    // ICU permits a valid prefix of a malformed BCP47 tag. Intl rejects it.
    if parsed != tag_len {
        return Err(LocaleError::InvalidLocale);
    }
    Ok(locale)
}

// Intl accepts Unicode locale identifiers, a subset of the BCP47 tags that
// ICU accepts. Private-only, grandfathered and extlang forms are excluded.
fn has_unicode_language_prefix(tag: &str) -> bool {
    let mut subtags = tag.split('-');
    let language = subtags.next().unwrap_or_default();
    let valid_language = matches!(language.len(), 2 | 3 | 5..=8)
        && language.bytes().all(|byte| byte.is_ascii_alphabetic());
    let extlang = subtags.next().is_some_and(|subtag| {
        subtag.len() == 3 && subtag.bytes().all(|byte| byte.is_ascii_alphabetic())
    });
    valid_language && !extlang
}

struct Enumeration<'a> {
    ptr: NonNull<c_void>,
    api: &'a Api,
}
impl Drop for Enumeration<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.api.uenum_close)(self.ptr.as_ptr());
        }
    }
}
impl Enumeration<'_> {
    fn values(&self) -> Result<Vec<CString>, LocaleError> {
        let mut values = Vec::new();
        loop {
            let mut status = 0;
            let mut len = 0;
            let value = unsafe { (self.api.uenum_next)(self.ptr.as_ptr(), &mut len, &mut status) };
            check("enumerate locale values", status)?;
            if value.is_null() {
                return Ok(values);
            }
            // ICU owns this NUL-terminated value until the enumeration advances.
            values.push(unsafe { CStr::from_ptr(value) }.to_owned());
        }
    }
}

pub fn resolve_available(
    api: &Api,
    requested: &CStr,
    service: Service,
) -> Result<Option<CString>, LocaleError> {
    let language = requested
        .to_bytes()
        .split(|byte| matches!(byte, b'_' | b'@'))
        .next()
        .unwrap_or_default();
    // ICU versions represent und as empty, und, or root when keywords exist.
    // Intl resolves all of these requests to its selected default.
    if language.is_empty() || matches!(language, b"und" | b"root") {
        return Ok(None);
    }
    static COLLATION: OnceLock<Vec<CString>> = OnceLock::new();
    static RELATIVE: OnceLock<Vec<CString>> = OnceLock::new();
    let (cache, count, get) = match service {
        Service::Collation => (&COLLATION, api.ucol_countAvailable, api.ucol_getAvailable),
        // V8 also derives RelativeTimeFormat availability from DateFormat.
        Service::Relative => (&RELATIVE, api.udat_countAvailable, api.udat_getAvailable),
    };
    let available = cache.get_or_init(|| {
        (0..unsafe { count() })
            .filter_map(|index| {
                let locale = unsafe { get(index) };
                (!locale.is_null()).then(|| unsafe { CStr::from_ptr(locale) }.to_owned())
            })
            .collect()
    });
    let pointers: Vec<*const c_char> = available.iter().map(|locale| locale.as_ptr()).collect();
    let mut status = 0;
    let ptr = unsafe {
        (api.uenum_openCharStringsEnumeration)(
            pointers.as_ptr(),
            length(pointers.len())?,
            &mut status,
        )
    };
    let enumeration = Enumeration {
        ptr: NonNull::new(ptr).ok_or(LocaleError::Icu {
            operation: "open locale enumeration",
            status,
        })?,
        api,
    };
    check("open locale enumeration", status)?;
    let requested_pointers = [requested.as_ptr()];
    let mut accepted = 0;
    let matched = char_output(|buffer, capacity, status| unsafe {
        (api.uloc_acceptLanguage)(
            buffer,
            capacity,
            &mut accepted,
            requested_pointers.as_ptr(),
            1,
            enumeration.ptr.as_ptr(),
            status,
        )
    })?;
    // ULOC_ACCEPT_FAILED=0. ICU handles aliases and parent matching.
    if accepted == 0 || matched.is_empty() {
        return Ok(None);
    }
    // Open the negotiated data locale, not an unsupported alias such as cmn.
    // Preserve keywords for service-specific validation after negotiation.
    let mut locale = matched.into_bytes();
    if let Some(index) = requested.to_bytes().iter().position(|byte| *byte == b'@') {
        locale.extend_from_slice(&requested.to_bytes()[index..]);
    }
    Ok(Some(
        CString::new(locale).map_err(|_| LocaleError::InvalidLocale)?,
    ))
}

pub fn os_default(api: &Api) -> Result<CString, LocaleError> {
    static DEFAULT: OnceLock<Result<CString, LocaleError>> = OnceLock::new();
    DEFAULT
        .get_or_init(|| {
            // Linux Intl selects ICU's process locale. sys-locale prioritizes
            // LANGUAGE, which can disagree with ICU's LC_ALL/LANG selection.
            #[cfg(not(target_os = "linux"))]
            if let Some(tag) = sys_locale::get_locale() {
                let tag = normalize_default(&tag);
                if let Ok(locale) = from_language_tag(api, tag) {
                    if let Some(locale) = resolve_available(api, &locale, Service::Relative)? {
                        return Ok(locale);
                    }
                }
            }
            let locale = unsafe { (api.uloc_getDefault)() };
            if locale.is_null() {
                return Err(LocaleError::Unavailable("OS locale"));
            }
            let locale = unsafe { CStr::from_ptr(locale) };
            // V8 maps ICU's POSIX default to en-US for undefined-locale Intl calls.
            // Do this only for the system default. Explicit en-US-POSIX is retained.
            let name = locale
                .to_str()
                .map_err(|_| LocaleError::Unavailable("OS locale encoding"))?;
            if name.eq_ignore_ascii_case("en_US_POSIX")
                || name.eq_ignore_ascii_case("c")
                || name.eq_ignore_ascii_case("POSIX")
            {
                from_language_tag(api, "en-US")
            } else {
                Ok(locale.to_owned())
            }
        })
        .clone()
}
#[cfg(any(not(target_os = "linux"), test))]
fn normalize_default(tag: &str) -> &str {
    if ["C", "POSIX", "en-US-POSIX"]
        .iter()
        .any(|candidate| tag.eq_ignore_ascii_case(candidate))
    {
        "en-US"
    } else {
        tag
    }
}

fn keywords(api: &Api, locale: &CStr) -> Result<Vec<CString>, LocaleError> {
    let mut status = 0;
    let ptr = unsafe { (api.uloc_openKeywords)(locale.as_ptr(), &mut status) };
    check("open locale keywords", status)?;
    let Some(ptr) = NonNull::new(ptr) else {
        return Ok(Vec::new());
    };
    Enumeration { ptr, api }.values()
}
fn keyword_value(api: &Api, locale: &CStr, keyword: &CStr) -> Result<CString, LocaleError> {
    char_output(|buffer, capacity, status| unsafe {
        (api.uloc_getKeywordValue)(locale.as_ptr(), keyword.as_ptr(), buffer, capacity, status)
    })
}
fn remove_keyword(api: &Api, locale: &CStr, keyword: &CStr) -> Result<CString, LocaleError> {
    let mut buffer = locale.to_bytes_with_nul().to_vec();
    let mut status = 0;
    let len = unsafe {
        (api.uloc_setKeywordValue)(
            keyword.as_ptr(),
            std::ptr::null(),
            buffer.as_mut_ptr().cast(),
            length(buffer.len())?,
            &mut status,
        )
    };
    check("remove locale keyword", status)?;
    let len = usize::try_from(len).map_err(|_| LocaleError::InputTooLong)?;
    let output = buffer
        .get(..len)
        .ok_or(LocaleError::Unavailable("ICU locale result"))?;
    CString::new(output).map_err(|_| LocaleError::InvalidLocale)
}

pub fn sort_locale(api: &Api, locale: &CStr) -> Result<CString, LocaleError> {
    let mut filtered = locale.to_owned();
    for keyword in keywords(api, locale)? {
        let value = keyword_value(api, locale, &keyword)?;
        let valid = match keyword.to_bytes() {
            b"collation" => valid_collation(api, locale, &value)?,
            b"colnumeric" => matches!(value.to_bytes(), b"yes" | b"no"),
            b"colcasefirst" => matches!(value.to_bytes(), b"upper" | b"lower" | b"no"),
            _ => false,
        };
        if !valid {
            filtered = remove_keyword(api, &filtered, &keyword)?;
        }
    }
    Ok(filtered)
}
fn valid_collation(api: &Api, locale: &CStr, value: &CStr) -> Result<bool, LocaleError> {
    // Intl's sort usage excludes these two ICU collation names.
    if matches!(value.to_bytes(), b"search" | b"standard") {
        return Ok(false);
    }
    let mut status = 0;
    let ptr = unsafe {
        (api.ucol_getKeywordValuesForLocale)(c"collation".as_ptr(), locale.as_ptr(), 0, &mut status)
    };
    let enumeration = Enumeration {
        ptr: NonNull::new(ptr).ok_or(LocaleError::Icu {
            operation: "open collation values",
            status,
        })?,
        api,
    };
    check("open collation values", status)?;
    Ok(enumeration
        .values()?
        .iter()
        .any(|candidate| candidate.as_c_str() == value))
}

pub fn relative_locale(api: &Api, locale: &CStr) -> Result<CString, LocaleError> {
    let mut filtered = locale.to_owned();
    for keyword in keywords(api, locale)? {
        let valid = if keyword.to_bytes() == b"numbers" {
            valid_numbering_system(api, &keyword_value(api, locale, &keyword)?)?
        } else {
            false
        };
        if !valid {
            filtered = remove_keyword(api, &filtered, &keyword)?;
        }
    }
    Ok(filtered)
}
fn valid_numbering_system(api: &Api, value: &CStr) -> Result<bool, LocaleError> {
    let mut status = 0;
    let ptr = unsafe { (api.unumsys_openByName)(value.as_ptr(), &mut status) };
    struct NumberingSystem<'a> {
        ptr: NonNull<c_void>,
        api: &'a Api,
    }
    impl Drop for NumberingSystem<'_> {
        fn drop(&mut self) {
            unsafe {
                (self.api.unumsys_close)(self.ptr.as_ptr());
            }
        }
    }
    let Some(ptr) = NonNull::new(ptr) else {
        if matches!(status, 1 | 16) {
            return Ok(false);
        }
        check("open numbering system", status)?;
        return Ok(false);
    };
    let system = NumberingSystem { ptr, api };
    check("open numbering system", status)?;
    Ok(unsafe { (api.unumsys_isAlgorithmic)(system.ptr.as_ptr()) } == 0)
}

fn char_output(
    mut operation: impl FnMut(*mut c_char, i32, *mut i32) -> i32,
) -> Result<CString, LocaleError> {
    let mut buffer = vec![0_u8; 128];
    loop {
        let mut status = 0;
        let result_len = operation(
            buffer.as_mut_ptr().cast(),
            length(buffer.len())?,
            &mut status,
        );
        if status == 15 && result_len >= 0 {
            buffer.resize(
                usize::try_from(result_len).map_err(|_| LocaleError::InputTooLong)? + 1,
                0,
            );
            continue;
        }
        check("resolve locale", status)?;
        let result_len = usize::try_from(result_len).map_err(|_| LocaleError::InputTooLong)?;
        let output = buffer
            .get(..result_len)
            .ok_or(LocaleError::Unavailable("ICU locale result"))?;
        return CString::new(output).map_err(|_| LocaleError::InvalidLocale);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn posix_default_uses_the_intl_default_instead_of_binary_collation() {
        for locale in ["C", "POSIX", "en-US-POSIX"] {
            assert_eq!(super::normalize_default(locale), "en-US");
        }
        assert_eq!(super::normalize_default("sv-SE"), "sv-SE");
    }
}
