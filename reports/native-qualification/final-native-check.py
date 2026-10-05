#!/usr/bin/env python3
"""Qualify a frozen native source copy and record the compiled test inventory."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


REQUIRED_TESTS = [
    "removes_a_draft_and_discards_a_draft_only_session",
    "releasing_a_lead_invalidates_a_pending_worker_read",
    "missing_worker_models_fail_before_sending_a_planning_turn",
    "restored_typescript_receipt_reuses_the_delegation_without_dispatching_again",
    "restored_typescript_approval_receipt_accepts_ordinary_integral_spelling",
    "object_order_does_not_change_a_persisted_request",
    "integral_spelling_preserves_safe_values_without_rounding_large_integers",
    "corrupt_receipts_and_different_value_types_do_not_match",
    "decimal_and_exponent_comparisons_are_exact_and_fail_closed_on_overflow",
    "unexpected_host_exit_stops_the_provider_group",
    "cached_image_follows_appearance_without_resetting_zoom",
    "cached_pdf_follows_appearance_without_reopening_the_document",
    "cached_terminal_follows_appearance_without_restarting_the_pty",
    "cached_diff_follows_appearance_without_resetting_content_or_expansion",
    "cached_inbox_pr_diff_follows_appearance_without_reloading_or_resetting_expansion",
    "pending_highlight_cannot_restore_the_previous_theme",
    "steering_honors_changed_model_settings_and_rejects_an_idle_session",
    "step_usage_reports_catalog_capacity_and_aggregates_reasoning_and_cache",
    "remote_workspace_shortcut_preserves_selection_and_creates_the_host_worktree_before_sending",
]

LOCALE_TESTS = [
    "matches_intl_relative_french_past_and_future",
    "matches_intl_relative_japanese_past_and_future",
    "matches_intl_relative_arabic_past_and_future",
    "matches_intl_relative_rounding_and_unit_boundaries",
    "matches_intl_text_punctuation_and_accents",
    "matches_intl_text_canonical_equivalence",
    "matches_intl_history_ties_without_changing_pin_or_recency",
    "matches_intl_recent_project_ranking_punctuation_and_accents",
    "matches_intl_grouped_search_canonical_equivalence_and_score_priority",
    "matches_intl_file_ranking_punctuation",
    "matches_intl_file_ranking_accents",
    "matches_intl_file_ranking_canonical_equivalence",
    "matches_intl_mcp_order_without_changing_availability",
    "matches_intl_skill_ranking_without_changing_scope_or_score",
    "matches_intl_saved_note_ties_without_changing_recency",
    "matches_intl_note_picker_keeps_input_order_for_equal_score_and_time",
    "matches_intl_inbox_view_default_french_japanese_arabic",
    "matches_intl_inbox_view_shared_parser_and_rounding",
    "matches_intl_change_tree_directory_and_file_order",
    "matches_intl_notification_project_order",
    "matches_intl_local_note_save_ties_without_changing_recency",
    "matches_intl_composer_skill_order_without_changing_scope_or_score",
    "matches_intl_composer_mcp_order_without_changing_availability",
    "matches_intl_secondary_page_default_french_japanese_arabic",
    "matches_intl_secondary_page_rounding_and_unit_boundaries",
    "icu_collation_matches_intl_punctuation_accents_and_canonical_equivalence",
    "relative_time_uses_auto_words_language_and_localized_numbers",
    "unicode_extensions_retain_intl_numeric_and_case_preferences",
    "malformed_locales_fail_and_unsupported_languages_use_the_selected_default",
    "scoped_locale_restores_on_unwind_and_never_changes_other_threads",
    "repeated_default_comparisons_reuse_the_same_native_collator",
    "search_collation_is_not_used_for_default_intl_sort",
    "und_locale_resolves_to_the_selected_default_and_drops_unsupported_extensions",
    "posix_default_uses_the_intl_default_instead_of_binary_collation",
    "unsupported_numeric_and_case_extensions_keep_the_default_comparison",
    "non_relevant_collation_extensions_do_not_change_default_sort_options",
    "unsupported_or_algorithmic_numbering_extensions_keep_the_locale_and_default_digits",
    "negotiated_aliases_use_the_matching_locale_data",
    "unsupported_collation_language_uses_the_selected_default",
    "catalog_locale_preserves_effort_rank_and_collates_unknown_variants",
    "catalog_locale_keeps_equivalent_model_names_in_provider_order",
    "catalog_locale_preserves_equivalent_pi_and_omp_model_order",
    "catalog_locale_selects_french_and_swedish_pi_and_omp_model_order",
]

SEARCH_TESTS = [
    "javascript_regex_search_accepts_lookaround_and_backreferences",
    "javascript_regex_replacement_keeps_lookbehind_and_capture_groups",
    "javascript_regex_classes_keep_word_digit_and_whitespace_rules",
    "javascript_preview_regex_accepts_lookaround_and_backreferences",
    "javascript_preview_regex_keeps_unicode_character_class_rules",
    "codemirror_literal_search_normalizes_canonical_and_compatibility_characters",
    "codemirror_literal_replacement_skips_partial_normalized_characters",
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--manifest-helper", type=Path, required=True)
    parser.add_argument("--require-localization", action="store_true")
    parser.add_argument("--require-javascript-search", action="store_true")
    options = parser.parse_args()
    source = options.source.resolve()
    output = options.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    manifest = options.manifest.resolve()
    helper = options.manifest_helper.resolve()
    summary = {"platform": sys.platform, "source": str(source), "commands": {}}

    def run(name, arguments):
        log = output / f"{name}.log"
        started = time.monotonic()
        with log.open("w") as stream:
            result = subprocess.run(arguments, cwd=source, stdout=stream, stderr=subprocess.STDOUT)
        summary["commands"][name] = {
            "arguments": [str(value) for value in arguments],
            "exit_code": result.returncode,
            "seconds": round(time.monotonic() - started, 3),
            "log": log.name,
        }
        (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(f"{name}: exit {result.returncode}", flush=True)
        if result.returncode:
            raise RuntimeError(f"{name} failed, see {log}")
        return log

    run("source-before", [sys.executable, helper, "verify", "--root", source, "--manifest", manifest])
    # Force the source copy newer than existing Cargo artifacts after a sync.
    for root in ("apps", "crates", "vendor", ".cargo"):
        for path in (source / root).rglob("*"):
            if path.is_file() and not {"target", ".git"}.intersection(path.relative_to(source).parts):
                os.utime(path, None)
    for name in ("Cargo.toml", "Cargo.lock"):
        os.utime(source / name, None)

    cargo = ["cargo"]
    locked = ["--locked", "--offline", "-j", "2"]
    workspace = ["--workspace", "--lib", "--bins", "--all-features"]
    run("fmt", [*cargo, "fmt", "--all", "--check"])
    compile_log = run("compile", [*cargo, "test", *workspace, *locked, "--no-run", "--message-format=json"])
    binaries = []
    for line in compile_log.read_text().splitlines():
        try:
            artifact = json.loads(line)
        except json.JSONDecodeError:
            continue
        if artifact.get("reason") == "compiler-artifact" and artifact.get("profile", {}).get("test") and artifact.get("executable"):
            binary = Path(artifact["executable"])
            if binary not in binaries:
                binaries.append(binary)
    if not binaries:
        raise RuntimeError("Cargo emitted no test executables")
    inventory = {}
    for binary in binaries:
        result = subprocess.run([binary, "--list"], cwd=source, capture_output=True, text=True, check=True)
        inventory[str(binary)] = [line[:-6] for line in result.stdout.splitlines() if line.endswith(": test")]
    (output / "test-inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")
    required = REQUIRED_TESTS.copy()
    if options.require_localization:
        required += LOCALE_TESTS
        if sys.platform == "win32":
            required += ["legacy_windows_icu_uses_system_libraries_and_an_initialized_apartment"]
    if options.require_javascript_search:
        required += SEARCH_TESTS
    if sys.platform != "win32":
        required += ["ordinary_provider_exit_reaps_the_descendant_and_guard", "failed_spawn_releases_the_native_guard"]
    registered = {}
    names = [name for tests in inventory.values() for name in tests]
    for required_name in required:
        matches = [name for name in names if name.rsplit("::", 1)[-1] == required_name]
        if len(matches) != 1:
            raise RuntimeError(f"Expected one compiled test named {required_name}, found {matches}")
        registered[required_name] = matches[0]
    http = [name for name in names if "host::http::tests::early_rejection" in name]
    if len(http) != 2:
        raise RuntimeError(f"Expected both early HTTP rejection regressions, found {http}")
    summary["registered_regressions"] = registered
    summary["registered_http_regressions"] = http
    summary["test_executables"] = len(binaries)
    workspace_log = run("workspace", [*cargo, "test", *workspace, *locked])
    passed = set(re.findall(r"^test (\S+) \.\.\. ok$", workspace_log.read_text(), re.MULTILINE))
    for name in [*registered.values(), *http]:
        if name not in passed:
            raise RuntimeError(f"Compiled regression did not pass in the full suite: {name}")
    run("clippy", [*cargo, "clippy", "--workspace", "--all-targets", "--all-features", *locked, "--", "-D", "warnings"])
    run("core", [*cargo, "test", "-p", "monocode-core", "--tests", *locked])
    run("recovery", [*cargo, "test", "-p", "monocode-host", "--test", "remote_recovery", *locked])
    run("askpass", [*cargo, "test", "-p", "monocode-app", "--test", "ssh_askpass", "--all-features", *locked])
    if sys.platform != "win32":
        run("video", [*cargo, "test", "-p", "monocode-platform", "--test", "inline_video", *locked])
    else:
        summary["video"] = "Interactive Session 1 fixture is recorded separately. Session 0 is not a video pass."
    run("build", [*cargo, "build", "-p", "monocode-app", "-p", "monocode-host", "-p", "monocode-package", "--bins", "--features", "monocode-app/screenshot", *locked])
    run("source-after", [sys.executable, helper, "verify", "--root", source, "--manifest", manifest])
    for name in ("workspace", "core", "recovery", "askpass", "video"):
        path = output / f"{name}.log"
        if not path.exists():
            continue
        counts = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", path.read_text())
        summary["commands"][name]["tests"] = {
            "targets": len(counts),
            "passed": sum(int(count[0]) for count in counts),
            "failed": sum(int(count[1]) for count in counts),
            "ignored": sum(int(count[2]) for count in counts),
        }
    target = Path(os.environ.get("CARGO_TARGET_DIR", source / "target"))
    if not target.is_absolute():
        target = source / target
    suffix = ".exe" if sys.platform == "win32" else ""
    summary["binary_sha256"] = {}
    for name in ("monocode-app", "monocode-host", "monocode-package"):
        path = target / "debug" / f"{name}{suffix}"
        summary["binary_sha256"][name] = hashlib.sha256(path.read_bytes()).hexdigest()
    summary["complete"] = True
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2), flush=True)


if __name__ == "__main__":
    main()
