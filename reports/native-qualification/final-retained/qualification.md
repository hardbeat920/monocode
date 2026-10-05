# Retained Rust checkpoint

The retained Tauri application passed its Rust checks against native source digest `58c060e4687dc6bf600d89ce6423f2682e25355ea2867406ef3d996bb544a4c0`. This repeat includes the shared Unix provider guardian and remote and platform repairs present in that frozen snapshot. It covers that checkpoint, rather than subsequent locale or fixture edits.

Formatting passed. Strict Clippy passed every retained target with `--locked`, `--offline`, and `-D warnings`. All 35 library tests passed, with no failures or ignored tests. The binary and documentation targets had zero tests and passed. See the [format log](fmt.log), [Clippy log](clippy.log), [test log](test.log), and [summary](summary.json).

The runner used the existing task-owned Rust 1.98.1 toolchain on arm64 macOS. It kept its separate `retained-cargo` cache and `retained-target` build directory, with two jobs and debug information disabled. It performed no dependency fetch, unlocked resolution, local Cargo build, or GUI launch. The [runner](check-final-retained-rust.sh) records the exact commands.

All 76 retained source files matched the local checkout before the run and remained unchanged afterward. This includes the retained manifest, lockfile, Rust source, Tauri configuration, capabilities, icons, and asset catalog. The [retained source manifest](source-sha256.json) excludes build targets, Git directories, and Finder metadata. The [before](retained-source-before.log) and [after](retained-source-after.log) checks report zero changed, missing, or extra files.

The [frozen native manifest](native-source-manifest.json) records 2,629 shared native inputs. Both the [native before](native-source-before.log) and [native after](native-source-after.log) checks report the same digest and zero differences. The minimal retained lockfile kept SHA-256 `92286fbedc0dfda0ab49e3e48d87e0a818dc88040790b650438ad8d93ae08bfd` before and after the locked checks. The [result log](result.log) records successful source and lock guards.

No source or lock repair was needed. No production profile, credentials, host service, or updater feed changed. This result qualifies retained macOS compilation and tests for the recorded snapshot. It does not qualify a retained desktop GUI connection to the native host, a live retained provider turn, or retained Windows and Linux behavior. New shared source changes require a new retained check before cutover.
