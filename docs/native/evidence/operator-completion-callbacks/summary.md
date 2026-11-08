# MonoCode callback thermonuclear review

Reviewed the full branch against origin/main, then reviewed the fixes. The maintainability review is complete with no remaining blocking findings in this feature. Publication validation has the suite limitations listed below.

## Findings and resolution

The original delivery path could consume a callback when the parent's selected provider account had been removed. The launch path then rejected the report. This was a real delivery-loss defect, not a style concern. A new test failed before the fix. Delivery now waits for account recovery and checks that condition again at dispatch. See 01-delivery.md.

Pending siblings were called still running even if they had already completed behind a busy parent. A second failing-before regression established the misleading context. Reports now use Other reports pending. See 01-delivery.md.

The callback tests pushed an existing 667-line test file to 1375 lines. The new tests had their own coherent fixtures and behaviour, so that growth was unjustified. They now live in a 709-line completion module and the parent is 669 lines. No changed file newly crosses 1000 lines. See 02-structure-tests.md.

Final async race validation, delivery persistence and dispatch were nested inside a large scheduling closure. The implementation now separates scheduling/report loading from final delivery, with the latter rechecking the current parent and subscription state. The named SessionLaunch payload also removes the six-field positional tuple and adjacent boolean ambiguity. See both detail files.

## Verification

Both new behavioural regressions failed before the fixes, while the other 16 callback tests passed. All 18 callback tests passed after the final structural extraction. The core suite passed 674 tests, with 2 ignored. The native script suite passed 29 tests.

The full GPUI suite passed 865 tests, failed 1 and ignored 11. The failure was title_particle_frames_only_invalidate_the_sidebar, at the assertion title requested its next frame. That test's source matches origin/main. This review does not claim a green full suite. The isolated recheck passed, 1 test with no failures. That does not erase the full-suite failure.

Strict Clippy found six existing warnings in unchanged core files. With only nonminimal_bool and collapsible_match allowed, Clippy passed both packages and all targets with -D warnings for the other categories. Global formatting still reports the same baseline differences in shell.rs and shell/terminal_panes.rs; direct checks pass every other touched Rust file using its package edition.

The native app demonstrated two separately labelled workers, two app-owned reports and two automatic parent answers. A screenshot shows A complete while B is still running. A second screenshot shows both parent replies separately. Persisted state confirmed two sessions.start calls, no polling calls, both notifications delivered and Operator disabled in both children. The user independently confirmed the live flow worked.

## Remaining limits

Delivery is at most once. A crash between saving delivered state and dispatch can lose that report; this is documented. Labels are supported for ordinary Operator callers only, and Monos reject them explicitly. A worker finishing does not establish verification success. The review did not broaden any child capabilities or self-session management restrictions.

The related open PRs cover lead/subagent orchestration, Mono handoff or report content. Reusing those mechanisms here would change the caller model and expand scope. This branch keeps grouping and subscription ownership in a dedicated native Operator module.
