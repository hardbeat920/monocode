use monocode_locale::{compare, default_locale, format_relative_time, RelativeTimeUnit};

fn main() {
    let pairs: Vec<_> = [("_a", ".a"), ("ä", "z"), ("é", "f"), ("e\u{301}", "é"), ("9", "10"), ("a", "B")]
        .into_iter().map(|(a, b)| serde_json::json!({ "a": a, "b": b, "order": match compare(a,b) { std::cmp::Ordering::Less => -1, std::cmp::Ordering::Equal => 0, std::cmp::Ordering::Greater => 1 } })).collect();
    println!(
        "{}",
        serde_json::json!({
            "system_locale": sys_locale::get_locale(),
            "icu_locale": default_locale().unwrap(),
            "pairs": pairs,
            "relative": format_relative_time(-2, RelativeTimeUnit::Hour, None).unwrap(),
        })
    );
}
