use crate::LocaleError;
use std::ffi::{c_char, c_void};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}
#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
}

pub struct Libraries {
    i18n: usize,
    common: usize,
    legacy: bool,
}
impl Libraries {
    pub fn load() -> Result<Self, LocaleError> {
        if let Some(combined) = load("icu.dll") {
            return Ok(Self {
                i18n: combined as usize,
                common: combined as usize,
                legacy: false,
            });
        }
        Self::load_legacy()
    }
    pub fn load_legacy() -> Result<Self, LocaleError> {
        let i18n = load("icuin.dll").ok_or(LocaleError::Unavailable("Windows ICU i18n library"))?;
        let Some(common) = load("icuuc.dll") else {
            // No function pointer has escaped this failed initialization.
            unsafe {
                FreeLibrary(i18n);
            }
            return Err(LocaleError::Unavailable("Windows ICU common library"));
        };
        Ok(Self {
            i18n: i18n as usize,
            common: common as usize,
            legacy: true,
        })
    }
    pub fn is_legacy(&self) -> bool {
        self.legacy
    }
    pub fn symbol(&self, name: &[u8]) -> Result<*mut c_void, LocaleError> {
        // Both modules stay loaded for the process lifetime. The static API
        // stores their function pointers, and every name ends with NUL.
        let mut symbol = unsafe { GetProcAddress(self.i18n as *mut c_void, name.as_ptr().cast()) };
        if symbol.is_null() && self.common != self.i18n {
            symbol = unsafe { GetProcAddress(self.common as *mut c_void, name.as_ptr().cast()) };
        }
        if symbol.is_null() {
            Err(LocaleError::Unavailable("Windows ICU C API export"))
        } else {
            Ok(symbol)
        }
    }
}
fn load(name: &str) -> Option<*mut c_void> {
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // LOAD_LIBRARY_SEARCH_SYSTEM32 prevents DLLs in the working directory or
    // application directories from replacing the operating system ICU library.
    let module = unsafe { LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), 0x0000_0800) };
    (!module.is_null()).then_some(module)
}

pub struct Apartment {
    balance: bool,
}
impl Apartment {
    pub fn new() -> Result<Self, LocaleError> {
        // Legacy Windows ICU requires COM. An existing STA or MTA is suitable.
        // RPC_E_CHANGED_MODE means this thread already owns another apartment.
        let status = unsafe { CoInitializeEx(std::ptr::null_mut(), 0) };
        match status {
            0 | 1 => Ok(Self { balance: true }),
            -2147417850 => Ok(Self { balance: false }),
            _ => Err(LocaleError::Unavailable("Windows ICU COM apartment")),
        }
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.balance {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

impl Drop for Libraries {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.i18n as *mut c_void);
            if self.common != self.i18n {
                FreeLibrary(self.common as *mut c_void);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_windows_icu_uses_system_libraries_and_an_initialized_apartment() {
        let api = crate::api::Api::load_from(Libraries::load_legacy().unwrap()).unwrap();
        assert!(api.legacy);
        let apartment = Apartment::new().unwrap();
        let mut status = 0;
        let locale = c"fr_FR";
        let formatter = unsafe {
            (api.ureldatefmt_open)(locale.as_ptr(), std::ptr::null_mut(), 0, 256, &mut status)
        };
        assert!(!formatter.is_null());
        assert!(status <= 0);
        let mut result = [0_u16; 128];
        status = 0;
        let len = unsafe {
            (api.ureldatefmt_format)(
                formatter,
                -2.0,
                5,
                result.as_mut_ptr(),
                result.len() as i32,
                &mut status,
            )
        };
        unsafe {
            (api.ureldatefmt_close)(formatter);
        }
        assert!(status <= 0);
        assert_eq!(
            String::from_utf16(&result[..len as usize]).unwrap(),
            "il y a 2 heures"
        );
        drop(apartment);
    }
}
