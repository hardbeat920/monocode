//! Port of src/shared/lib/numbers.ts.

/// `formatInteger`: whole-number UI counts with `en-US` thousands
/// separators, rounded half away from zero like `Intl.NumberFormat`.
pub fn format_integer(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "∞" } else { "-∞" }.into();
    }
    let rounded = value.abs().round();
    let digits = format!("{rounded:.0}");
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if value < 0.0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands_with_commas() {
        assert_eq!(format_integer(1250.0), "1,250");
        assert_eq!(format_integer(4_222.0), "4,222");
        assert_eq!(format_integer(253.0), "253");
        assert_eq!(format_integer(1_234_567.4), "1,234,567");
        assert_eq!(format_integer(-1500.0), "-1,500");
    }
}
