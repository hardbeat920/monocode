use super::*;

#[test]
fn icu_collation_matches_intl_punctuation_accents_and_canonical_equivalence() {
    for locale in ["en-US", "fr-FR", "ja-JP", "ar"] {
        for (a, b, expected) in [
            ("_a", ".a", Ordering::Less),
            ("a-b", "a_b", Ordering::Greater),
            ("é", "f", Ordering::Less),
            ("e\u{301}", "é", Ordering::Equal),
            ("ä\u{323}", "a\u{323}\u{308}", Ordering::Equal),
            ("9", "10", Ordering::Greater),
            ("aB", "Ab", Ordering::Less),
        ] {
            assert_eq!(
                compare_for_locale(a, b, locale).unwrap(),
                expected,
                "{locale}: {a:?} / {b:?}"
            );
        }
    }
    assert_eq!(
        compare_for_locale("ä", "z", "sv-SE").unwrap(),
        Ordering::Greater
    );
    assert_eq!(
        compare_for_locale("ä", "z", "de-DE").unwrap(),
        Ordering::Less
    );
}

#[test]
fn relative_time_uses_auto_words_language_and_localized_numbers() {
    for (locale, value, unit, expected) in [
        ("en-US", 0, RelativeTimeUnit::Second, "now"),
        ("en-US", -1, RelativeTimeUnit::Day, "yesterday"),
        ("fr-FR", -2, RelativeTimeUnit::Hour, "il y a 2 heures"),
        ("ja-JP", -2, RelativeTimeUnit::Hour, "2 時間前"),
        ("ar", -2, RelativeTimeUnit::Hour, "قبل ساعتين"),
        (
            "fr-FR-u-nu-arab",
            -3,
            RelativeTimeUnit::Hour,
            "il y a ٣ heures",
        ),
    ] {
        assert_eq!(
            format_relative_time(value, unit, Some(locale)).unwrap(),
            expected
        );
    }
}

#[test]
fn unicode_extensions_retain_intl_numeric_and_case_preferences() {
    assert_eq!(
        compare_for_locale("9", "10", "en-US").unwrap(),
        Ordering::Greater
    );
    assert_eq!(
        compare_for_locale("9", "10", "en-US-u-kn-true").unwrap(),
        Ordering::Less
    );
    assert_eq!(
        compare_for_locale("a", "A", "en-US-u-kf-upper").unwrap(),
        Ordering::Greater
    );
    assert_eq!(
        compare_for_locale("a", "A", "en-US-u-kf-lower").unwrap(),
        Ordering::Less
    );
}

#[test]
fn malformed_locales_fail_and_unsupported_languages_use_the_selected_default() {
    for locale in ["", "fr_FR", "fr--FR", "fr-FR-invalid!", "fr\0FR"] {
        assert!(
            format_relative_time(-2, RelativeTimeUnit::Hour, Some(locale)).is_err(),
            "{locale:?}"
        );
    }
    with_locale("fr-FR", || {
        assert_eq!(
            format_relative_time(-2, RelativeTimeUnit::Hour, Some("zz-ZZ")).unwrap(),
            "il y a 2 heures"
        );
    })
    .unwrap();
}

#[test]
fn scoped_locale_restores_on_unwind_and_never_changes_other_threads() {
    let original = default_locale().unwrap();
    with_locale("sv-SE", || {
        assert_eq!(compare("ä", "z"), Ordering::Greater);
        with_locale("de-DE", || assert_eq!(compare("ä", "z"), Ordering::Less)).unwrap();
        assert_eq!(compare("ä", "z"), Ordering::Greater);
        let other = std::thread::spawn(default_locale).join().unwrap().unwrap();
        assert_eq!(other, original);
        let failure = std::panic::catch_unwind(|| {
            with_locale("de-DE", || panic!("controlled locale scope unwind"))
        });
        assert!(failure.is_err());
        assert_eq!(compare("ä", "z"), Ordering::Greater);
    })
    .unwrap();
    assert_eq!(default_locale().unwrap(), original);
}

#[test]
fn repeated_default_comparisons_reuse_the_same_native_collator() {
    with_locale("fr-FR", || {
        compare("é", "f");
        let before =
            with_cache(|cache| Ok(cache.entries.last().unwrap().collator.as_ref().unwrap().ptr))
                .unwrap();
        for _ in 0..100 {
            assert_eq!(compare("é", "f"), Ordering::Less);
        }
        let after =
            with_cache(|cache| Ok(cache.entries.last().unwrap().collator.as_ref().unwrap().ptr))
                .unwrap();
        assert_eq!(before, after);
    })
    .unwrap();
}

#[test]
fn search_collation_is_not_used_for_default_intl_sort() {
    assert_eq!(
        compare_for_locale("ä", "ae", "de-DE-u-co-search").unwrap(),
        Ordering::Less
    );
    assert_eq!(
        compare_for_locale("ひ", "ヒ", "ja-JP-u-co-search").unwrap(),
        Ordering::Equal
    );
    assert_eq!(
        compare_for_locale("9", "10", "en-US-u-co-search-kn-true").unwrap(),
        Ordering::Less
    );
}

#[test]
fn und_locale_resolves_to_the_selected_default_and_drops_unsupported_extensions() {
    with_locale("fr-FR", || {
        for locale in ["und", "und-u-nu-arab", "und-Latn", "und-US", "und-Cyrl"] {
            assert_eq!(
                format_relative_time(-3, RelativeTimeUnit::Hour, Some(locale)).unwrap(),
                "il y a 3 heures"
            );
        }
    })
    .unwrap();
}

#[test]
fn unsupported_numeric_and_case_extensions_keep_the_default_comparison() {
    assert_eq!(
        compare_for_locale("9", "10", "en-u-kn-foo").unwrap(),
        Ordering::Greater
    );
    assert_eq!(
        compare_for_locale("a", "A", "en-u-kf-yes").unwrap(),
        Ordering::Less
    );
    assert_eq!(
        compare_for_locale("9", "10", "en-u-kn-yes").unwrap(),
        Ordering::Less
    );
}

#[test]
fn non_relevant_collation_extensions_do_not_change_default_sort_options() {
    assert_eq!(
        compare_for_locale("a-b", "ab", "en-u-ka-shifted").unwrap(),
        Ordering::Less
    );
    assert_eq!(
        compare_for_locale("a", "A", "en-u-kc-true").unwrap(),
        Ordering::Less
    );
    assert_eq!(
        compare_for_locale("é", "e", "en-u-ks-level1").unwrap(),
        Ordering::Greater
    );
}

#[test]
fn unsupported_or_algorithmic_numbering_extensions_keep_the_locale_and_default_digits() {
    for locale in ["fr-u-nu-foo", "fr-u-nu-roman", "fr-u-nu-native"] {
        assert_eq!(
            format_relative_time(-3, RelativeTimeUnit::Hour, Some(locale)).unwrap(),
            "il y a 3 heures",
            "{locale}"
        );
    }
}

#[test]
fn negotiated_aliases_use_the_matching_locale_data() {
    assert_eq!(
        format_relative_time(-3, RelativeTimeUnit::Hour, Some("cmn")).unwrap(),
        "3小时前"
    );
    assert_eq!(
        compare_for_locale("木", "水", "cmn").unwrap(),
        compare_for_locale("木", "水", "zh").unwrap()
    );
}

#[test]
fn unsupported_collation_language_uses_the_selected_default() {
    with_locale("sv-SE", || {
        assert_eq!(
            compare_for_locale("ä", "z", "ccp").unwrap(),
            Ordering::Greater
        );
    })
    .unwrap();
}
