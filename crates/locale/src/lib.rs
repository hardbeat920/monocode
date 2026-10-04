//! ICU collation and relative time for the native app and headless engine.
//! Locale preferences come from the OS. ICU owns the rules and locale data.

mod api;
mod locale;

use std::{
    cell::RefCell,
    cmp::Ordering,
    ffi::{c_void, CStr, CString},
    fmt,
    ptr::NonNull,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocaleError {
    InvalidLocale,
    InputTooLong,
    Unavailable(&'static str),
    Icu {
        operation: &'static str,
        status: i32,
    },
}
impl fmt::Display for LocaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLocale => f.write_str("Invalid locale language tag"),
            Self::InputTooLong => f.write_str("Input exceeds the ICU C API length limit"),
            Self::Unavailable(operation) => write!(f, "Unavailable {operation}"),
            Self::Icu { operation, status } => {
                write!(f, "ICU {operation} failed with status {status}")
            }
        }
    }
}
impl std::error::Error for LocaleError {}

/// Values are the stable URelativeDateTimeUnit constants in ureldatefmt.h.
#[repr(i32)]
#[derive(Clone, Copy, Debug)]
pub enum RelativeTimeUnit {
    Year = 0,
    Month = 2,
    Week = 3,
    Day = 4,
    Hour = 5,
    Minute = 6,
    Second = 7,
}

/// Compare like String.localeCompare with no options and the OS default locale.
/// A missing native ICU service is an installation failure, not a different sort.
pub fn compare(a: &str, b: &str) -> Ordering {
    try_compare(a, b, None).expect("Native ICU collation must be available")
}

/// Explicit locale selection for the same no-option comparison contract.
pub fn compare_for_locale(a: &str, b: &str, locale: &str) -> Result<Ordering, LocaleError> {
    try_compare(a, b, Some(locale))
}

fn try_compare(a: &str, b: &str, locale: Option<&str>) -> Result<Ordering, LocaleError> {
    let a_len = length(a.len())?;
    let b_len = length(b.len())?;
    with_cache(|cache| {
        let locale = cache.resolve(locale, locale::Service::Collation)?;
        let entry = cache.entry(locale);
        if entry.collator.is_none() {
            entry.collator = Some(Collator::new(&entry.locale)?);
        }
        let collator = entry.collator.as_ref().unwrap();
        let mut status = 0;
        // Rust strings are valid UTF-8. Explicit lengths retain embedded NULs.
        let order = unsafe {
            (collator.api.ucol_strcollUTF8)(
                collator.ptr.as_ptr(),
                a.as_ptr().cast(),
                a_len,
                b.as_ptr().cast(),
                b_len,
                &mut status,
            )
        };
        check("compare UTF-8", status)?;
        Ok(order.cmp(&0))
    })
}

/// Format the caller's already rounded offset with LONG style and numeric:auto.
/// Callers can return an empty string on failure, as retained Intl callers do.
pub fn format_relative_time(
    value: i32,
    unit: RelativeTimeUnit,
    locale: Option<&str>,
) -> Result<String, LocaleError> {
    with_cache(|cache| {
        let locale = cache.resolve(locale, locale::Service::Relative)?;
        let entry = cache.entry(locale);
        let formatter = match &entry.relative {
            Some(formatter) => formatter,
            None => entry
                .relative
                .insert(RelativeFormatter::new(&entry.locale)?),
        };
        let mut buffer = vec![0_u16; 128];
        loop {
            let mut status = 0;
            let result_len = unsafe {
                (formatter.api.ureldatefmt_format)(
                    formatter.ptr.as_ptr(),
                    f64::from(value),
                    unit as i32,
                    buffer.as_mut_ptr(),
                    length(buffer.len())?,
                    &mut status,
                )
            };
            if status == 15 && result_len >= 0 {
                buffer.resize(
                    usize::try_from(result_len).map_err(|_| LocaleError::InputTooLong)? + 1,
                    0,
                );
                continue;
            }
            check("format relative time", status)?;
            let result_len = usize::try_from(result_len).map_err(|_| LocaleError::InputTooLong)?;
            let output = buffer
                .get(..result_len)
                .ok_or(LocaleError::Unavailable("ICU relative time result"))?;
            return String::from_utf16(output)
                .map_err(|_| LocaleError::Unavailable("ICU UTF-16 output"));
        }
    })
}

/// The ICU locale identifier selected from OS preferences, after validation.
/// This exposes the selector for qualification without changing global locale.
pub fn default_locale() -> Result<String, LocaleError> {
    with_cache(|cache| {
        Ok(cache
            .resolve(None, locale::Service::Relative)?
            .to_string_lossy()
            .into_owned())
    })
}

fn length(value: usize) -> Result<i32, LocaleError> {
    value.try_into().map_err(|_| LocaleError::InputTooLong)
}
fn check(operation: &'static str, status: i32) -> Result<(), LocaleError> {
    // ICU warnings have negative values and are successful results.
    if status > 0 {
        Err(LocaleError::Icu { operation, status })
    } else {
        Ok(())
    }
}

struct Collator {
    ptr: NonNull<c_void>,
    api: &'static api::Api,
}
impl Collator {
    fn new(locale: &CStr) -> Result<Self, LocaleError> {
        let api = api::get()?;
        let locale = locale::sort_locale(api, locale)?;
        let mut status = 0;
        let ptr = unsafe { (api.ucol_open)(locale.as_ptr(), &mut status) };
        let collator = Self {
            ptr: NonNull::new(ptr).ok_or(LocaleError::Icu {
                operation: "open collator",
                status,
            })?,
            api,
        };
        check("open collator", status)?;
        // Intl default sort uses tertiary strength and canonical equivalence.
        // ICU retains the locale's case, punctuation and Unicode extensions.
        // UCOL_NORMALIZATION_MODE=4, UCOL_ON=17, UCOL_STRENGTH=5, TERTIARY=2.
        for (attribute, value) in [(4, 17), (5, 2)] {
            status = 0;
            unsafe {
                (api.ucol_setAttribute)(collator.ptr.as_ptr(), attribute, value, &mut status);
            }
            check("configure collator", status)?;
        }
        Ok(collator)
    }
}
impl Drop for Collator {
    fn drop(&mut self) {
        unsafe {
            (self.api.ucol_close)(self.ptr.as_ptr());
        }
    }
}

struct RelativeFormatter {
    ptr: NonNull<c_void>,
    api: &'static api::Api,
}
impl RelativeFormatter {
    fn new(locale: &CStr) -> Result<Self, LocaleError> {
        let api = api::get()?;
        let mut status = 0;
        let locale = locale::relative_locale(api, locale)?;
        // UDAT_STYLE_LONG=0 and UDISPCTX_CAPITALIZATION_NONE=1<<8.
        let ptr = unsafe {
            (api.ureldatefmt_open)(locale.as_ptr(), std::ptr::null_mut(), 0, 256, &mut status)
        };
        let formatter = Self {
            ptr: NonNull::new(ptr).ok_or(LocaleError::Icu {
                operation: "open relative formatter",
                status,
            })?,
            api,
        };
        check("open relative formatter", status)?;
        Ok(formatter)
    }
}
impl Drop for RelativeFormatter {
    fn drop(&mut self) {
        unsafe {
            (self.api.ureldatefmt_close)(self.ptr.as_ptr());
        }
    }
}

struct Entry {
    locale: CString,
    collator: Option<Collator>,
    relative: Option<RelativeFormatter>,
}
struct Cache {
    entries: Vec<Entry>,
    api: &'static api::Api,
    // Entries drop before COM initialization is balanced on Windows.
    _apartment: api::Apartment,
}
impl Cache {
    fn new() -> Result<Self, LocaleError> {
        let api = api::get()?;
        Ok(Self {
            entries: Vec::new(),
            api,
            _apartment: api::Apartment::new(api)?,
        })
    }
    fn resolve(
        &self,
        locale: Option<&str>,
        service: locale::Service,
    ) -> Result<CString, LocaleError> {
        match locale {
            Some(locale) => {
                let requested = locale::from_language_tag(self.api, locale)?;
                match locale::resolve_available(self.api, &requested, service)? {
                    Some(locale) => Ok(locale),
                    None => self.resolve(None, service),
                }
            }
            None => {
                #[cfg(any(test, feature = "test-support"))]
                if let Some(locale) = LOCALE_SCOPE.with(|scope| scope.borrow().last().cloned()) {
                    return Ok(locale);
                }
                locale::os_default(self.api)
            }
        }
    }
    fn entry(&mut self, locale: CString) -> &mut Entry {
        if let Some(index) = self.entries.iter().position(|entry| entry.locale == locale) {
            let entry = self.entries.remove(index);
            self.entries.push(entry);
        } else {
            // Explicit locale inputs must not grow an unbounded process cache.
            if self.entries.len() == 16 {
                self.entries.remove(0);
            }
            self.entries.push(Entry {
                locale,
                collator: None,
                relative: None,
            });
        }
        self.entries.last_mut().unwrap()
    }
}
thread_local! { static CACHE: RefCell<Option<Cache>> = const { RefCell::new(None) }; }
fn with_cache<T>(
    operation: impl FnOnce(&mut Cache) -> Result<T, LocaleError>,
) -> Result<T, LocaleError> {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.is_none() {
            *cache = Some(Cache::new()?);
        }
        operation(cache.as_mut().unwrap())
    })
}

#[cfg(any(test, feature = "test-support"))]
thread_local! { static LOCALE_SCOPE: RefCell<Vec<CString>> = const { RefCell::new(Vec::new()) }; }
/// Run a controlled fixture with a locale on this thread only. Restore on unwind.
#[cfg(any(test, feature = "test-support"))]
pub fn with_locale<T>(locale: &str, operation: impl FnOnce() -> T) -> Result<T, LocaleError> {
    let locale = with_cache(|cache| cache.resolve(Some(locale), locale::Service::Relative))?;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            LOCALE_SCOPE.with(|scope| {
                scope.borrow_mut().pop();
            });
        }
    }
    LOCALE_SCOPE.with(|scope| scope.borrow_mut().push(locale));
    let _restore = Restore;
    Ok(operation())
}

#[cfg(test)]
mod tests;
