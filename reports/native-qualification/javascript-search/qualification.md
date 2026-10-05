# Native find parity

The retained editor uses CodeMirror's JavaScript regular expressions with Unicode and multiline flags. Its preview search uses JavaScript Unicode regular expressions without the multiline flag. The native editor and preview used Rust regex syntax, which rejected lookaround and backreferences and applied different word, digit and whitespace classes.

The retained editor also normalizes literal queries and text with NFKD. Its cursor records whether a match covers a whole normalized character. Replace skips a match that covers only part of a character. The original native literal search omitted both rules.

The [oracle script](node-oracle.mjs) runs the installed retained CodeMirror `SearchQuery` for the editor cases and Node regular expressions for all retained flag combinations. Its [results](node-oracle.json) record UTF-8 byte ranges, capture replacement and literal match precision. The fixtures cover positive lookahead, fixed and variable-width lookbehind, backreferences, character classes, invalid inline flags, canonical equivalents, compatibility characters and replacement of partial normalized characters.

All seven regressions failed against the original implementation, as recorded in [the before summary](before-summary.json). All seven pass against the repaired source on macOS, Linux and Windows. The changed Rust files passed formatting checks.

The selected regular-expression dependency is [`regress` 0.12.0](https://github.com/ridiculousfish/regress). It implements JavaScript syntax in Rust and has MIT and Apache-2.0 licenses. The official crate archive has SHA-256 `32eef8b209c3c1c15dbad02c1f30f9539f00dc7253e0cbcdaae442a50a09d7c1`. The exact dependency enables only `std`, which uses the already locked `memchr` package. Literal editor matching uses the already locked `unicode-normalization` package. The preview's literal escaping uses `regress::escape` so its Unicode mode accepts punctuation such as hyphens.

## Platform runs

Each platform extracted the same source archive into a new directory, compiled the workspace test targets, ran [the after check](check-after.py), and ran workspace Clippy with warnings denied. The [macOS runner](macos-check.sh) ran on hometop with macOS 27.0.1 and Rust 1.98.1. The [Linux runner](linux-check.sh) and [Windows runner](windows-check.ps1) ran on QRK-GLUON with WSL Ubuntu 24.04.5, Windows 11 build 26200 and Rust 1.99.0. Each run's `archive-sha256.txt` records archive SHA-256 `f894d753a18245acde27d80c808feeade6f1bd0e8afab683e63ffd1832ec868a`. All three summaries record the same source hashes and their test binary hashes.

The first Windows attempt failed to compile `monocode-view-pages` because Cargo reused a stale `monocode-engine` build from the shared target directory. Tar had restored file times older than that cached build. All three runners now extract with `tar -m`, and each later compile log shows all 31 workspace crates rebuilt.

The next run passed all seven regressions and both suites on Linux and Windows. Clippy 1.99 then rejected six one-element `vec![a..b]` table rows in the preview search tests under its `single_range_in_vec_init` lint. The editor search tests had the same six rows. Clippy 1.98 does not have that lint. Both tables now hold single ranges, and the one three-match lookbehind case has its own assertion. Workspace Clippy 1.99 then passed against the macOS working tree.

The final run passed on all three platforms. All seven regressions passed, the editor suite passed 116 tests, the file-view suite passed 97 tests, and workspace Clippy passed. See the [macOS summary](after/summary.json) and [Clippy log](after/clippy.log), the [Linux summary](linux/summary.json) and [Clippy log](linux/clippy.log), and the [Windows summary](windows/summary.json) and [Clippy log](windows/clippy.log).
