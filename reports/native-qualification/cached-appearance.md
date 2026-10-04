# Cached native viewer appearance

The bounded audit found five cached viewers that did not react to global appearance changes. The image, PDF, SCM diff, terminal, and Inbox PR diff now update their existing entities. The audit also reproduced a DiffView syntax race. An old highlight job could replace the new theme's tokens after a theme change.

## Changes and preserved state

The image and PDF use one appearance subscription on `BinaryFileSurface`. The terminal, SCM diff, and Inbox PR diff subscribe inside their existing viewer contexts. Their release callbacks retain each subscription until its viewer entity closes. These callbacks call the existing theme setters without replacing the cached viewers or reloading their content.

| Regression | State checked after the appearance change |
| --- | --- |
| `cached_image_follows_appearance_without_resetting_zoom` | Same ImageView entity, scale 2.5, and exact controlled PNG bytes |
| `cached_pdf_follows_appearance_without_reopening_the_document` | Same PdfView entity, loaded two-page document, current page, scale 1.75, and exact PDF bytes |
| `cached_diff_follows_appearance_without_resetting_content_or_expansion` | Same SCM DiffView and model allocation, exact diff content, expanded file, collapsed second file, and revealed context |
| `cached_terminal_follows_appearance_without_restarting_the_pty` | Same cached TerminalView, one PTY spawn with the original ID and cwd, unchanged screen text and grid, and working input and output after the change |
| `cached_inbox_pr_diff_follows_appearance_without_reloading_or_resetting_expansion` | Same Inbox DiffView, exact PrDiff model and patches, expanded files, Code tab, and unchanged data calls |

Each fixture switches the actual global appearance from dark to light with a custom accent. It requires the existing cached entity to match the live theme. The fixtures use controlled files, models, and PTY inputs. They do not touch production data or launch a paid provider.

The pending-highlight fixture delays an actual dark Rust syntax job. It changes the theme, lets the newer light syntax finish, then releases the dark job. The BEFORE result failed because the dark tokens replaced the light tokens. The setter now clears its stored highlight tasks before scheduling the new syntax jobs. `set_files` already uses this cancellation mechanism. The fixture checks the exact diff, expanded files, revealed folds, row model, and scroll position before its final token assertion.

The used CodeEditor constructor already observes global appearance. Its theme setter updates the existing find bar and search decorations. The audit did not change that path. Read-only viewer inspection is behind the editor's `test-support` feature and the relevant test dependency declarations.

## Commands and evidence

The tests ran on the hometop Mac with Rust 1.98.1 in `/Users/gianvillarini/Library/Caches/monocode-gpui-build/source`. Cargo used the isolated task cache and target directory. The checked source is the uncommitted `gpui-native` worktree based on `b585761aafb39c265618191897c5579fe509ccd2`.

The first four fixtures used the same workspace graph for BEFORE and AFTER:

```sh
cargo test --workspace --lib --bins --all-features --locked cached_ -j 2 --no-fail-fast
```

[BEFORE](appearance/before-all.log) had four intended palette failures and ten passing existing tests. [AFTER](appearance/after-all.log) passed all 14 selected tests. The earlier [first run](appearance/before-terminal.log) stopped at the terminal assertion, so the complete BEFORE run used `--no-fail-fast`.

The Inbox PR fixture used:

```sh
cargo test --workspace --lib --bins --all-features --locked cached_inbox_pr_diff -j 2
```

[Inbox BEFORE](appearance/inbox-before.log) failed its palette assertion after checking cached identity, patches, and expansion. [Inbox AFTER](appearance/inbox-after.log) passed the same fixture.

The pending-highlight fixture used:

```sh
cargo test --workspace --lib --bins --all-features --locked pending_highlight_cannot_restore_the_previous_theme -j 2
```

The [race BEFORE](appearance/race-before.log) failed the intended old-palette overwrite assertion. The [race AFTER](appearance/race-after.log) passed the same fixture after the one-line task cancellation. The fixture retained its exact model and view state throughout both runs.

The five observer fixes already passed [affected-target strict Clippy](appearance/clippy.log):

```sh
cargo clippy -p monocode-app -p monocode-view-files -p monocode-view-scm -p monocode-view-inbox --all-targets --all-features --locked -j 2 -- -D warnings
```

The final check included the editor and harness explicitly and [passed](appearance/final-clippy.log):

```sh
cargo clippy -p monocode-app -p monocode-editor -p monocode-harness -p monocode-view-files -p monocode-view-scm -p monocode-view-inbox --all-targets --all-features --locked -j 2 -- -D warnings
```

## Qualified source snapshot

The [local hash manifest](appearance/source-sha256.json) and [hometop hash output](appearance/source-sha256-hometop.txt) match for all 13 files below. These hashes identify the full shared files used in the qualified Mac snapshot, including preserved earlier work.

| Source | SHA-256 |
| --- | --- |
| `crates/editor/Cargo.toml` | `b833cd603e21cb46db67707b295f476aed2f824d8f918898566e1be9cf4f5692` |
| `crates/editor/src/image_view.rs` | `8d063f7115def91d1b0ccfe3365d7c91bdaf78be8fa6c6fb71cef27f172f559f` |
| `crates/editor/src/pdf_view.rs` | `6914c9c11b997b1a0122a038f4032598d411cead8f16e8f5a4c92462be3a0b4e` |
| `crates/editor/src/diff_view.rs` | `cf56330266be156f6475ce647b84174ffb649e06c7910616bbfbfa65f10104ea` |
| `crates/view-files/Cargo.toml` | `80d65bd1aeee67e3916ef9e05495e46afb0164fcd3c9835fcdd3ffb2cbfe7043` |
| `crates/view-files/src/binary_view.rs` | `be099191cca90167844549bc6cdbe2a882c8522dd85eeb77d533b0fcc6370e0d` |
| `crates/view-scm/Cargo.toml` | `2b73eab4c8a15340f6e566418facc3dc94245ef9e0681be64f656c32df51d805` |
| `crates/view-scm/src/ui/diffs.rs` | `fc7fb13e3f18c08e00f13c000d187dbd96b16760aef1ae3a6b5009c3542184b5` |
| `apps/monocode-app/src/file_pane.rs` | `279c0c76ada6b28d93d37fb2825ecff883603427af3d74ee067046fb4125a230` |
| `crates/view-inbox/Cargo.toml` | `6c3855f4afbdd2682bfae68c9949188bcce68399d33316103489084828ff7aaa` |
| `crates/view-inbox/src/pr/detail.rs` | `71190b0798d7c3044d418eb5308902e5f1b02b19b6ca257012cfda49f7e2bc38` |
| `crates/view-inbox/src/pr/detail_appearance_tests.rs` | `8b6761c90cc83c52a4125abf7f69396144081b780ffb6ccbc364589f385f0ab2` |
| `crates/view-inbox/src/pr/diff.rs` | `1bc4098b64a71c5dad8feafa760aab95d0dc15b47b1f466ddc29eef00a962cc9` |

## Qualification limits

These are GPUI entity and engine behavior checks. They prove that global appearance changes reach the cached viewers and retain the checked state. They do not qualify native screenshots, hardware rendering, OS window behavior, release packages, or updater installation. The existing build logs contain linker and dependency future-compatibility notices. They do not change the test outcomes.

No legacy source or canonical cutover document changed for this audit.
