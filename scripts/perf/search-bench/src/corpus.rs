//! Deterministic benchmark inputs. Both implementations run exactly the same
//! source, query, replacement and flags.

pub struct Case {
    pub id: String,
    pub source: String,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub replace_all: bool,
}

const SMALL_SIZES: &[(&str, usize)] = &[
    ("10KB", 10 * 1024),
    ("100KB", 100 * 1024),
    ("1MB", 1024 * 1024),
];
const LARGE_SIZES: &[(&str, usize)] = &[("5MB", 5 * 1024 * 1024)];
const FILLER: &str =
    "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor\n";

fn fill(target: usize, every: usize, match_line: impl Fn(usize) -> String) -> String {
    let mut out = String::with_capacity(target + FILLER.len());
    let mut i = 0;
    while out.len() < target {
        if i % every == 0 {
            out.push_str(&match_line(i));
        } else {
            out.push_str(FILLER);
        }
        i += 1;
    }
    out
}

fn whole_word_source(target: usize) -> String {
    let mut out = String::with_capacity(target + 32);
    let mut i = 0;
    while out.len() < target {
        if i % 5 == 0 {
            out.push_str("café naïve résumé\n");
        } else {
            // `caféteria` starts with the query but must not match whole-word.
            out.push_str("caféteria naïve déjà vu\n");
        }
        i += 1;
    }
    out
}

pub fn cases(large: bool, filter: Option<&str>) -> Vec<Case> {
    let mut sizes = SMALL_SIZES.to_vec();
    if large {
        sizes.extend_from_slice(LARGE_SIZES);
    }
    let mut out = Vec::new();
    // Construct only selected inputs. This matters for per-case peak-RSS runs.
    let mut add = |id: String,
                   source: &dyn Fn() -> String,
                   query: &str,
                   replacement: &str,
                   case_sensitive: bool,
                   whole_word: bool,
                   regex: bool,
                   replace_all: bool| {
        if filter.is_none_or(|filter| id.contains(filter)) {
            out.push(Case {
                id,
                source: source(),
                query: query.into(),
                replacement: replacement.into(),
                case_sensitive,
                whole_word,
                regex,
                replace_all,
            });
        }
    };
    for &(size_label, size) in &sizes {
        let literal = || fill(size, 10, |_| "needle alpha beta gamma\n".into());
        add(
            format!("literal-{size_label}"),
            &literal,
            "needle",
            "replacement",
            false,
            false,
            false,
            false,
        );
        add(
            format!("literal-case-{size_label}"),
            &|| fill(size, 10, |_| "Needle alpha beta gamma\n".into()),
            "Needle",
            "replacement",
            true,
            false,
            false,
            false,
        );
        add(
            format!("unicode-literal-{size_label}"),
            &|| fill(size, 7, |_| "café ﬃ É 🙂\n".into()),
            "café",
            "tea",
            false,
            false,
            false,
            false,
        );
        add(
            format!("whole-word-{size_label}"),
            &|| whole_word_source(size),
            "café",
            "tea",
            false,
            true,
            false,
            false,
        );
        add(
            format!("regex-email-{size_label}"),
            &|| {
                fill(size, 8, |i| {
                    format!("contact user{i}@example.com for details\n")
                })
            },
            r"(\w+)@(\w+)\.com",
            "$2 at $1",
            false,
            false,
            true,
            false,
        );
        add(
            format!("regex-backref-{size_label}"),
            &|| fill(size, 12, |i| format!("token{i} token{i}\n")),
            r"(\w+)\s+\1",
            "$1",
            false,
            false,
            true,
            false,
        );
        if size <= 1024 * 1024 {
            add(
                format!("replace-all-{size_label}"),
                &literal,
                "needle",
                "replacement",
                false,
                false,
                false,
                true,
            );
        }
    }
    // Stay within the editor's 16 KiB line limit for representative cases.
    let dense = || format!("{}\n", "needle ".repeat(100)).repeat((1024 * 1024) / 701);
    add(
        "replace-all-dense-1MB".into(),
        &dense,
        "needle",
        "replacement",
        false,
        false,
        false,
        true,
    );
    add(
        "no-match-1MB".into(),
        &dense,
        "missing",
        "",
        false,
        false,
        false,
        false,
    );
    add(
        "single-char-1MB".into(),
        &dense,
        "e",
        "",
        false,
        false,
        false,
        false,
    );
    if large {
        add(
            "stress-regex-long-line-1MB".into(),
            &|| "needle ".repeat((1024 * 1024) / 7),
            r"n(e+)dle",
            "$1",
            false,
            false,
            true,
            false,
        );
    }
    add(
        "real-tsx".into(),
        &|| include_str!("../../../../src/app/App.tsx").into(),
        r"\buse[A-Z]\w*",
        "",
        true,
        false,
        true,
        false,
    );
    add(
        "empty-query".into(),
        &|| "small file".into(),
        "",
        "",
        false,
        false,
        false,
        false,
    );
    add(
        "invalid-regex".into(),
        &|| "small file".into(),
        "[",
        "",
        false,
        false,
        true,
        false,
    );
    out
}
