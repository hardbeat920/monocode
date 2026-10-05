use crate::LocaleError;
use std::{
    ffi::{c_char, c_void},
    sync::OnceLock,
};

#[cfg(not(target_os = "windows"))]
#[allow(non_snake_case)]
mod ffi {
    include!(concat!(env!("OUT_DIR"), "/icu_locale.rs"));
}
#[cfg(target_os = "windows")]
mod windows;

macro_rules! icu_api {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)+) => {
        #[allow(non_snake_case)]
        pub struct Api {
            $(pub $name: unsafe extern "C" fn($($ty),*) -> $ret,)+
            pub legacy: bool,
            #[cfg(target_os = "windows")]
            _libraries: windows::Libraries,
        }
        impl Api {
            #[cfg(not(target_os = "windows"))]
            fn load() -> Result<Self, LocaleError> {
                Ok(Self { $($name: ffi::$name,)+ legacy: false })
            }
            #[cfg(target_os = "windows")]
            fn load() -> Result<Self, LocaleError> {
                Self::load_from(windows::Libraries::load()?)
            }
            #[cfg(target_os = "windows")]
            fn load_from(libraries: windows::Libraries) -> Result<Self, LocaleError> {
                Ok(Self {
                    $($name: {
                        let symbol = libraries.symbol(concat!(stringify!($name), "\0").as_bytes())?;
                        // The public ICU C ABI fixes this function's signature.
                        unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn($($ty),*) -> $ret>(symbol) }
                    },)+
                    legacy: libraries.is_legacy(),
                    _libraries: libraries,
                })
            }
        }
    };
}

icu_api! {
    ucol_open(locale: *const c_char, status: *mut i32) -> *mut c_void;
    ucol_close(collator: *mut c_void) -> ();
    ucol_strcollUTF8(collator: *const c_void, a: *const c_char, a_len: i32, b: *const c_char, b_len: i32, status: *mut i32) -> i32;
    ucol_setAttribute(collator: *mut c_void, attribute: i32, value: i32, status: *mut i32) -> ();
    ureldatefmt_open(locale: *const c_char, number_format: *mut c_void, width: i32, context: i32, status: *mut i32) -> *mut c_void;
    ureldatefmt_close(formatter: *mut c_void) -> ();
    ureldatefmt_format(formatter: *const c_void, offset: f64, unit: i32, result: *mut u16, capacity: i32, status: *mut i32) -> i32;
    uloc_getDefault() -> *const c_char;
    uloc_forLanguageTag(tag: *const c_char, locale: *mut c_char, capacity: i32, parsed: *mut i32, status: *mut i32) -> i32;
    uloc_getKeywordValue(locale: *const c_char, keyword: *const c_char, value: *mut c_char, capacity: i32, status: *mut i32) -> i32;
    uloc_setKeywordValue(keyword: *const c_char, value: *const c_char, locale: *mut c_char, capacity: i32, status: *mut i32) -> i32;
    uloc_acceptLanguage(result: *mut c_char, capacity: i32, accepted: *mut i32, requested: *const *const c_char, count: i32, available: *mut c_void, status: *mut i32) -> i32;
    uenum_openCharStringsEnumeration(strings: *const *const c_char, count: i32, status: *mut i32) -> *mut c_void;
    uloc_openKeywords(locale: *const c_char, status: *mut i32) -> *mut c_void;
    uenum_next(enumeration: *mut c_void, length: *mut i32, status: *mut i32) -> *const c_char;
    ucol_countAvailable() -> i32;
    ucol_getAvailable(index: i32) -> *const c_char;
    udat_countAvailable() -> i32;
    udat_getAvailable(index: i32) -> *const c_char;
    ucol_getKeywordValuesForLocale(keyword: *const c_char, locale: *const c_char, common: i8, status: *mut i32) -> *mut c_void;
    unumsys_openByName(name: *const c_char, status: *mut i32) -> *mut c_void;
    unumsys_close(system: *mut c_void) -> ();
    unumsys_isAlgorithmic(system: *const c_void) -> i8;
    uenum_close(enumeration: *mut c_void) -> ();
}

pub fn get() -> Result<&'static Api, LocaleError> {
    static API: OnceLock<Result<Api, LocaleError>> = OnceLock::new();
    API.get_or_init(Api::load).as_ref().map_err(Clone::clone)
}

pub struct Apartment {
    #[cfg(target_os = "windows")]
    inner: Option<windows::Apartment>,
}
impl Apartment {
    pub fn new(api: &Api) -> Result<Self, LocaleError> {
        #[cfg(target_os = "windows")]
        {
            Ok(Self {
                inner: api.legacy.then(windows::Apartment::new).transpose()?,
            })
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = api.legacy;
            Ok(Self {})
        }
    }
}
// The inner guard's Drop balances only COM initialization done by this thread.
#[cfg(target_os = "windows")]
impl Drop for Apartment {
    fn drop(&mut self) {
        self.inner.take();
    }
}
