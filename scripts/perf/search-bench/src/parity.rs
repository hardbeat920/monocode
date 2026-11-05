//! Differential checks against the actual pre-refactor bundle. These are an
//! opt-in audit, not hundreds of separate tests in the application's CI suite.
use crate::{editor_search, old};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    source: String,
    query: String,
    replacement: String,
    case: bool,
    word: bool,
    regex: bool,
    #[serde(default)]
    expected: Option<editor_search::SearchResult>,
}

pub fn verify(bundle: &str) -> Result<(), String> {
    let mut cases: Vec<Fixture> = serde_json::from_str(include_str!(
        "../../../../crates/monocode-gpui/src/editor_search/fixtures.json"
    ))
    .map_err(|e| e.to_string())?;
    // Each row targets a distinct semantic boundary; both switches are exercised.
    for (source, query, replacement, regex) in [
        ("", "needle", "x", false),
        ("unchanged", "", "x", true),
        ("a\\nb a\nb", "a\\nb", "$1", false),
        ("aaa aa aaaa", "aa", "🙂", false),
        ("a-b-a-b a-b", "a-b", "x", false),
        ("xfoo-foo-foo", "foo-foo", "x", false),
        ("xé-é-é", "é-é", "x", false),
        ("xfoo-foo-foo", "foo-foo", "x", true),
        ("xfoo foo\nfoo", r"foo\s*", "x", true),
        ("xfoo foo\nfoo", r"foo[^x]*?", "x", true),
        ("ﬃ ﬃffi ㍿ ㎏", "fi", "x", false),
        ("ﬃ ff ffi", "ff", "x", false),
        ("é e\u{301} É", "\u{301}", "x", false),
        ("a\u{301}b a\u{301} a", "a", "x", false),
        ("İ I i ı", "i\u{307}", "x", false),
        ("ΟΣ ΟΣΟ Σ σ ς", "ΟΣ", "x", false),
        ("🙂 👨‍👩‍👧‍👦 🇬🇧 𐐀𐐨", "𐐨", "x", false),
        ("foo_foo foo-bar éfooé ١foo", "foo", "x", false),
        ("a\nb\r\nc\rd\u{2028}e\u{2029}f", "^.", "x", true),
        ("a\nb\r\nc\rd", r"\s+", "x", true),
        ("a\nb\r\nc", r"[^x]+", "x", true),
        ("a\nb", r"a\Sb|a\sb", "x", true),
        ("a\nb", r"a[\S\s]b", "x", true),
        ("a\nb", r"a\x0ab", "x", true),
        ("a\nb", r"(?<=\n)b", "x", true),
        ("a\nb", r"a(?=\n)", "x", true),
        ("a a\nA", r"(a)\s+\1", "$1", true),
        ("é ١ K ſ k s", r"\w+|\d+", "$&", true),
        ("🙂a🙂", r".|(?=🙂)", "<$&>", true),
        ("🙂a\nb", r"a*|\s", "x", true),
        (
            "abc",
            r"(?<first>a)(b)?(z)?",
            "$<first>|$3|$02|$$|$&|$`|$'",
            true,
        ),
        ("abc", r"(a)(b)(c)", "$123|$01|$0001|$99|$0", true),
        ("aaab", r"(?<=a+)b", "x", true),
        ("ab ba aa", r"(a)?b\1", "$1", true),
        ("hello 世界 ١", r"\p{L}+|\p{N}+", "x", true),
        ("a", "[", "x", true),
        ("a", r"\q", "x", true),
        ("a", r"[[:alpha:]]", "x", true),
        ("a", r"(?i:a)", "x", true),
    ] {
        for case in [false, true] {
            for word in [false, true] {
                cases.push(Fixture {
                    // FileEditor receives LF-normalized document text. The old
                    // bridge also normalizes CRLF but maps offsets against the
                    // original input, so feeding raw disk bytes is not a valid
                    // comparison of the editor's buffer contract.
                    source: source.replace("\r\n", "\n").replace('\r', "\n"),
                    query: query.into(),
                    replacement: replacement.into(),
                    case,
                    word,
                    regex,
                    expected: None,
                });
            }
        }
    }
    let mut failures = 0;
    for fixture in &cases {
        let expected = old::search(
            bundle,
            &fixture.source,
            &fixture.query,
            &fixture.replacement,
            fixture.case,
            fixture.word,
            fixture.regex,
        )?;
        let actual = editor_search::search(
            &fixture.source,
            &fixture.query,
            &fixture.replacement,
            editor_search::SearchOptions {
                case_sensitive: fixture.case,
                whole_word: fixture.word,
                regex: fixture.regex,
            },
        );
        let frozen_ok = fixture
            .expected
            .as_ref()
            .is_none_or(|frozen| frozen == &expected);
        if actual != expected || !frozen_ok {
            failures += 1;
            eprintln!(
                "DIFF source={:?} query={:?} case={} word={} regex={}\n  old={expected:?}\n  new={actual:?} frozen_ok={frozen_ok}",
                fixture.source, fixture.query, fixture.case, fixture.word, fixture.regex
            );
        } else if !actual.invalid && !actual.timed_out {
            let mut output = fixture.source.clone();
            for m in expected.matches.iter().rev() {
                output.replace_range(m.from..m.to, &m.insert);
            }
            if editor_search::replace_all(&fixture.source, &actual.matches)? != output {
                return Err(format!("replacement mismatch: {fixture:?}"));
            }
        }
    }
    println!(
        "Exact differential parity: {} cases, {failures} differences (ranges, replacements, flags and output)",
        cases.len()
    );
    if failures > 0 {
        Err("differential parity failed".into())
    } else {
        Ok(())
    }
}
