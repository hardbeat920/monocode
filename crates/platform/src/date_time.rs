//! User-visible dates follow the system locale and time zone. Stored dates
//! and date input values keep their machine format in their owning crates.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateTimeStyle {
    MonthDay,
    Time,
    MonthDayTime,
    DateTime,
    MediumDateTime,
    Month,
    FullDate,
    ShortMonth,
    WeekdayMonthDay,
    Reminder,
    TimeZone,
}

impl DateTimeStyle {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn skeleton(self) -> &'static str {
        match self {
            Self::MonthDay => "MMMd",
            Self::Time => "jmm",
            Self::MonthDayTime => "MMMdjmm",
            Self::DateTime => "yMdjmmss",
            Self::MediumDateTime => "yMMMdjmm",
            Self::Month => "LLLL",
            Self::FullDate => "yMMMMEEEEd",
            Self::ShortMonth => "LLL",
            Self::WeekdayMonthDay => "MMMEd",
            Self::Reminder => "MMMEdjmm",
            Self::TimeZone => "z",
        }
    }
}

/// Format epoch milliseconds for display, or return an empty label for an
/// invalid timestamp. The formatter reads current OS preferences per call.
pub fn format_local(epoch_ms: i64, style: DateTimeStyle) -> String {
    format_with(epoch_ms, style, None, false).unwrap_or_default()
}

fn format_with(
    epoch_ms: i64,
    style: DateTimeStyle,
    locale: Option<&str>,
    utc: bool,
) -> Option<String> {
    // JavaScript Date's range also prevents native formatter overflow.
    if !(-8_640_000_000_000_000..=8_640_000_000_000_000).contains(&epoch_ms) {
        return None;
    }
    native::format(epoch_ms, style, locale, utc)
}

#[cfg(target_os = "macos")]
mod native {
    use objc2::AnyThread as _;
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{NSDate, NSDateFormatter, NSLocale, NSString, NSTimeZone};

    use super::DateTimeStyle;

    pub fn format(
        ms: i64,
        style: DateTimeStyle,
        locale: Option<&str>,
        utc: bool,
    ) -> Option<String> {
        autoreleasepool(|_| format_in_pool(ms, style, locale, utc))
    }

    fn format_in_pool(
        ms: i64,
        style: DateTimeStyle,
        locale: Option<&str>,
        utc: bool,
    ) -> Option<String> {
        let formatter = NSDateFormatter::new();
        let locale = locale.map_or_else(NSLocale::currentLocale, |name| {
            NSLocale::initWithLocaleIdentifier(NSLocale::alloc(), &NSString::from_str(name))
        });
        formatter.setLocale(Some(&locale));
        if utc {
            formatter.setTimeZone(Some(&NSTimeZone::timeZoneForSecondsFromGMT(0)));
        }
        formatter.setLocalizedDateFormatFromTemplate(&NSString::from_str(style.skeleton()));
        let date = NSDate::dateWithTimeIntervalSince1970(ms as f64 / 1000.);
        Some(formatter.stringFromDate(&date).to_string())
    }
}

#[cfg(windows)]
mod native {
    use windows::Foundation::DateTime;
    use windows::Globalization::DateTimeFormatting::{
        DateTimeFormatter, IDateTimeFormatterFactory,
    };
    use windows::Win32::Foundation::{
        CO_E_SERVER_STOPPING, E_INVALIDARG, E_POINTER, RPC_E_CHANGED_MODE,
    };
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
    use windows::core::{HRESULT, HSTRING, Interface};
    use windows_collections::IIterable;

    use super::DateTimeStyle;

    struct Apartment(bool);
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe { RoUninitialize() };
            }
        }
    }

    pub(super) struct FormatError {
        stage: &'static str,
        code: HRESULT,
    }

    impl std::fmt::Debug for FormatError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FormatError")
                .field("stage", &self.stage)
                .field("code", &self.code)
                .finish()
        }
    }

    pub fn format(
        ms: i64,
        style: DateTimeStyle,
        locale: Option<&str>,
        utc: bool,
    ) -> Option<String> {
        format_result(ms, style, locale, utc).ok()
    }

    fn retry_stopping_factory<T>(
        mut activate: impl FnMut() -> windows::core::Result<T>,
        mut wait: impl FnMut(),
    ) -> windows::core::Result<T> {
        for _ in 0..4 {
            match activate() {
                Err(error) if error.code() == CO_E_SERVER_STOPPING => wait(),
                result => return result,
            }
        }
        activate()
    }

    pub(super) fn format_result(
        ms: i64,
        style: DateTimeStyle,
        locale: Option<&str>,
        utc: bool,
    ) -> Result<String, FormatError> {
        let apartment = match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
            Ok(()) => Apartment(true),
            Err(error) if error.code() == RPC_E_CHANGED_MODE => Apartment(false),
            Err(error) => {
                return Err(FormatError {
                    stage: "RoInitialize",
                    code: error.code(),
                });
            }
        };
        let template = HSTRING::from(match style {
            DateTimeStyle::MonthDay => "month.abbreviated day",
            DateTimeStyle::Time => "hour minute",
            DateTimeStyle::MonthDayTime => "month.abbreviated day hour minute",
            DateTimeStyle::DateTime => "year.full month.numeric day hour minute second",
            DateTimeStyle::MediumDateTime => "year.full month.abbreviated day hour minute",
            DateTimeStyle::Month => "month.full",
            DateTimeStyle::FullDate => "longdate",
            DateTimeStyle::ShortMonth => "month.abbreviated",
            DateTimeStyle::WeekdayMonthDay => "dayofweek.abbreviated month.abbreviated day",
            DateTimeStyle::Reminder => "dayofweek.abbreviated month.abbreviated day hour minute",
            DateTimeStyle::TimeZone => "timezone.abbreviated",
        });
        // Keep the factory in this apartment. Concurrent calls can encounter
        // a stopping COM server during activation. Only that error retries,
        // with at most four 10 ms waits and no change to locale preferences.
        let factory = retry_stopping_factory(
            windows::core::factory::<DateTimeFormatter, IDateTimeFormatterFactory>,
            || std::thread::sleep(std::time::Duration::from_millis(10)),
        )
        .map_err(|error| FormatError {
            stage: "RoGetActivationFactory",
            code: error.code(),
        })?;
        let mut raw = std::ptr::null_mut();
        // SAFETY: These are the generated binding's constructor calls, using
        // this call's factory. The strings and iterable live through the call.
        let created = unsafe {
            match locale {
                Some(locale) => {
                    let languages: IIterable<HSTRING> = vec![HSTRING::from(locale)].into();
                    (factory.vtable().CreateDateTimeFormatterLanguages)(
                        factory.as_raw(),
                        std::mem::transmute_copy(&template),
                        languages.as_raw(),
                        &mut raw,
                    )
                }
                None => (factory.vtable().CreateDateTimeFormatter)(
                    factory.as_raw(),
                    std::mem::transmute_copy(&template),
                    &mut raw,
                ),
            }
        };
        created.ok().map_err(|error| FormatError {
            stage: "CreateDateTimeFormatter",
            code: error.code(),
        })?;
        if raw.is_null() {
            return Err(FormatError {
                stage: "CreateDateTimeFormatter/null",
                code: E_POINTER,
            });
        }
        // SAFETY: A successful constructor gives ownership of this COM object.
        let formatter = unsafe { DateTimeFormatter::from_raw(raw) };
        let date = DateTime {
            UniversalTime: ms
                .checked_mul(10_000)
                .and_then(|ticks| ticks.checked_add(116_444_736_000_000_000))
                .ok_or(FormatError {
                    stage: "timestamp conversion",
                    code: E_INVALIDARG,
                })?,
        };
        let result = if utc {
            formatter.FormatUsingTimeZone(date, &HSTRING::from("UTC"))
        } else {
            formatter.Format(date)
        }
        .map(|value| value.to_string())
        .map_err(|error| FormatError {
            stage: "Format",
            code: error.code(),
        });
        // Release WinRT objects before balancing this thread's initialization.
        drop(formatter);
        drop(factory);
        drop(apartment);
        result
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn activation_recovers_when_the_stopping_server_restarts() {
            let mut attempts = 0;
            let mut waits = 0;
            let value = retry_stopping_factory(
                || {
                    attempts += 1;
                    if attempts < 3 {
                        Err(windows::core::Error::from_hresult(CO_E_SERVER_STOPPING))
                    } else {
                        Ok(7)
                    }
                },
                || waits += 1,
            )
            .expect("restarted activation server");
            assert_eq!(value, 7);
            assert_eq!(attempts, 3);
            assert_eq!(waits, 2);
        }

        #[test]
        fn activation_preserves_permanent_errors_without_retrying() {
            let mut attempts = 0;
            let mut waits = 0;
            let error = retry_stopping_factory::<()>(
                || {
                    attempts += 1;
                    Err(windows::core::Error::from_hresult(E_INVALIDARG))
                },
                || waits += 1,
            )
            .expect_err("permanent activation error");
            assert_eq!(error.code(), E_INVALIDARG);
            assert_eq!(attempts, 1);
            assert_eq!(waits, 0);
        }

        #[test]
        fn activation_stops_retrying_when_the_server_remains_unavailable() {
            let mut attempts = 0;
            let mut waits = 0;
            let error = retry_stopping_factory::<()>(
                || {
                    attempts += 1;
                    Err(windows::core::Error::from_hresult(CO_E_SERVER_STOPPING))
                },
                || waits += 1,
            )
            .expect_err("bounded transient activation error");
            assert_eq!(error.code(), CO_E_SERVER_STOPPING);
            assert_eq!(attempts, 5);
            assert_eq!(waits, 4);
        }
    }
}

#[cfg(target_os = "linux")]
mod native {
    use std::ffi::{CString, c_char, c_void};
    use std::ptr;

    use super::DateTimeStyle;

    include!(concat!(env!("OUT_DIR"), "/icu_date.rs"));

    struct Generator(*mut c_void);
    impl Drop for Generator {
        fn drop(&mut self) {
            unsafe { udatpg_close(self.0) };
        }
    }
    struct Formatter(*mut c_void);
    impl Drop for Formatter {
        fn drop(&mut self) {
            unsafe { udat_close(self.0) };
        }
    }

    fn system_locale() -> Option<String> {
        ["LC_ALL", "LC_TIME", "LANG"]
            .iter()
            .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
    }

    fn icu_locale(locale: &str) -> Option<CString> {
        let (base, modifier) = locale.split_once('@').unwrap_or((locale, ""));
        let base = base.split('.').next()?;
        let value = if matches!(base, "C" | "POSIX") {
            "en_US_POSIX@hours=h23".into()
        } else if modifier.is_empty() {
            base.to_string()
        } else {
            format!("{base}@{modifier}")
        };
        CString::new(value).ok()
    }

    pub fn format(
        ms: i64,
        style: DateTimeStyle,
        locale: Option<&str>,
        utc: bool,
    ) -> Option<String> {
        let current = system_locale();
        let locale = match locale.or(current.as_deref()) {
            Some(locale) => Some(icu_locale(locale)?),
            None => None,
        };
        let locale_ptr: *const c_char = locale.as_ref().map_or(ptr::null(), |value| value.as_ptr());
        let mut status = 0;
        let raw = unsafe { udatpg_open(locale_ptr, &mut status) };
        if raw.is_null() {
            return None;
        }
        let generator = Generator(raw);
        if status > 0 {
            return None;
        }
        let skeleton: Vec<u16> = style.skeleton().encode_utf16().collect();
        let mut pattern = [0u16; 256];
        status = 0;
        let length = unsafe {
            udatpg_getBestPattern(
                generator.0,
                skeleton.as_ptr(),
                skeleton.len() as i32,
                pattern.as_mut_ptr(),
                pattern.len() as i32,
                &mut status,
            )
        };
        if status > 0 || !(1..=pattern.len() as i32).contains(&length) {
            return None;
        }
        let zone: Vec<u16> = if utc {
            "UTC".encode_utf16().collect()
        } else {
            Vec::new()
        };
        status = 0;
        // UDAT_PATTERN is -2. The pattern generator supplies locale-specific
        // field order, month names, and the preferred hour cycle for `j`.
        let raw = unsafe {
            udat_open(
                -2,
                -2,
                locale_ptr,
                if utc { zone.as_ptr() } else { ptr::null() },
                zone.len() as i32,
                pattern.as_ptr(),
                length,
                &mut status,
            )
        };
        if raw.is_null() {
            return None;
        }
        let formatter = Formatter(raw);
        if status > 0 {
            return None;
        }
        let mut output = [0u16; 512];
        let length = unsafe {
            udat_format(
                formatter.0,
                ms as f64,
                output.as_mut_ptr(),
                output.len() as i32,
                ptr::null_mut(),
                &mut status,
            )
        };
        if status > 0 || !(1..=output.len() as i32).contains(&length) {
            return None;
        }
        String::from_utf16(&output[..length as usize]).ok()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod native {
    use super::DateTimeStyle;
    pub fn format(_: i64, _: DateTimeStyle, _: Option<&str>, _: bool) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OCTOBER_2: i64 = 1_790_942_400_000;

    fn in_locale(style: DateTimeStyle, locale: &str) -> String {
        format_with(OCTOBER_2, style, Some(locale), true)
            .expect("native date formatter")
            .replace(['\u{200e}', '\u{200f}'], "")
    }

    #[test]
    fn localized_months_and_order_follow_the_requested_locale() {
        assert_eq!(in_locale(DateTimeStyle::MonthDay, "en-US"), "Oct 2");
        assert_eq!(in_locale(DateTimeStyle::MonthDay, "fr-FR"), "2 oct.");
        assert_eq!(in_locale(DateTimeStyle::Month, "fr-FR"), "octobre");
        let full = in_locale(DateTimeStyle::FullDate, "fr-FR");
        assert!(
            full.contains("vendredi") && full.contains("octobre") && full.contains("2026"),
            "{full}"
        );
        let japanese = in_locale(DateTimeStyle::MonthDay, "ja-JP");
        assert!(japanese.contains("10月2日"), "{japanese}");
        let medium = in_locale(DateTimeStyle::MediumDateTime, "fr-FR");
        assert!(
            medium.contains("oct.") && medium.contains("2026"),
            "{medium}"
        );
        let reminder = in_locale(DateTimeStyle::Reminder, "fr-FR");
        assert!(
            reminder.contains("ven.") && reminder.contains("oct."),
            "{reminder}"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn preferred_hour_cycle_and_seconds_follow_the_locale() {
        let english = in_locale(DateTimeStyle::Time, "en-US");
        assert!(
            english.contains("12:00") && english.contains("PM"),
            "{english}"
        );
        assert_eq!(in_locale(DateTimeStyle::Time, "fr-FR"), "12:00");
        let date = in_locale(DateTimeStyle::DateTime, "fr-FR");
        assert!(
            date.contains("02/10/2026") && date.contains("12:00:00"),
            "{date}"
        );
    }

    #[test]
    fn valid_timestamp_has_every_display_label() {
        for style in [
            DateTimeStyle::MonthDay,
            DateTimeStyle::Time,
            DateTimeStyle::MonthDayTime,
            DateTimeStyle::DateTime,
            DateTimeStyle::MediumDateTime,
            DateTimeStyle::Month,
            DateTimeStyle::FullDate,
            DateTimeStyle::ShortMonth,
            DateTimeStyle::WeekdayMonthDay,
            DateTimeStyle::Reminder,
            DateTimeStyle::TimeZone,
        ] {
            assert!(!format_local(OCTOBER_2, style).is_empty(), "{style:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn default_locale_formats_epoch_and_current_dates_concurrently() {
        let start = std::sync::Barrier::new(16);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    for _ in 0..64 {
                        for timestamp in [0, OCTOBER_2] {
                            let label =
                                native::format_result(timestamp, DateTimeStyle::Time, None, false)
                                    .expect("default-locale native clock formatter");
                            assert!(!label.is_empty());
                        }
                    }
                });
            }
        });
    }

    #[test]
    fn invalid_timestamps_have_no_label() {
        assert_eq!(format_local(i64::MAX, DateTimeStyle::DateTime), "");
        assert_eq!(format_local(i64::MIN, DateTimeStyle::Time), "");
        assert!(!format_local(OCTOBER_2, DateTimeStyle::Time).is_empty());
    }
}
