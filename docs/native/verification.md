# Native verification

Status recorded on October 22, 2026, on macOS Apple Silicon. These are local
results, not a release certification. Later dated sections supplement this
snapshot. See [preview limitations](parity.md) for
remaining runtime and platform work.

## Recorded results

| Check | Result |
| --- | --- |
| Complete native check (`npm run native:check`) | Passed |
| Native script tests (`npm run native:test:dev`) | 19 passed, including development restarts, bundle data modes and dependency guards |
| Native Rust formatting and strict Clippy | Passed for core and GPUI, including all targets |
| Native core tests | 547 passed, 1 ignored |
| GPUI tests | 551 passed, 11 ignored |
| Editor asset regeneration and smoke checks | Passed, including large-file diff highlighting |
| Remote checks | 50 core tests, 25 native runtime/UI tests, Connections and secret-input regressions passed; isolated Node-host and loopback SSH fixtures also passed |
| Host suite | 112 passed, 6 skipped |
| Production frontend | 5,560 tests passed, 13 skipped; TypeScript and production build passed under Node 24 |
| Production Rust workspace | 698 tests passed, 1 SSH fixture ignored; formatting and strict Clippy passed |
| Production macOS packaging | Tauri release `.app` built and its local ad-hoc signature verified |
| Native macOS commands | `native:dev` built and launched; `native:build` produced an optimized app without launching it; both bundle signatures verified |
| Native database defaults | Dev opened `Monocode Development/monocode.db`; release `--check-data` read the production database without migration |
| Native UI and chat | Provider order, live Codex streaming, Stop, saved history after relaunch, resumed provider context, and file preview/source passed |
| Native release UI | Restored existing workspace and conversation history from a consistent production database snapshot |
| macOS notifications | Reminder appeared in Notification Center; opening it returned from Settings to the matching chat and cleared the reminder |

The ignored tests require explicit SSH/host fixtures, a Metal device or a manual
frame-cost measurement. Cargo still reports an upstream future-compatibility
advisory for `block 0.1.6`. The preview workflow has not been run as part of these
local checks. Linux/Windows GUI checks and broad live-provider verification
remain outstanding.

Production source, lockfiles and release workflows were checked against `main`;
the native dependency graph remains separate. Live provider requests used the
development database and a temporary project. The release GUI used a disposable
production snapshot with background agents and automations disabled; only the
read-only data check opened the live production database. Notification delivery
used a separate test bundle identity. Notification sounds, Dock badges and
completion/approval alerts were not manually checked.

Reopening Tauri after native use of the live production database remains a manual
check. Concurrent shared-data startup, scheduled tasks and phone connections
remain unverified and deferred.

## Desktop preference migration — October 23, 2026

On Linux, all 27 automated desktop preference tests and strict core Clippy (`--all-targets`,
`--profile native-dev`, `-D warnings`) passed. The installed Linux WebKitGTK
preference store was readable. Synthetic fixtures verified that WebView2 reads
preserve source filenames and contents with the source database both open/locked
and closed, and that empty origins do not hide saved preferences. A WebKitGTK
fixture also verified eight saved projects reach the restored native workspace.
The full core suite passed serially: 557 passed, 1 ignored. The initial parallel
run failed two provider-process fixtures; both passed in a serial rerun and in
the full serial suite.

The Windows-only module's conditional-compilation path passed an isolated
`-D warnings` check on Linux. This was not a native Windows build. Real WebView2
profile compatibility, a Windows build and Windows execution remain unverified.
Copies of actively written LevelDB files are best-effort and may be incomplete
or unreadable; close Tauri before first-launch preference restoration.

The installed-profile integration test is ignored by default because it reads
personal storage. To run that read-only check explicitly:

```sh
cargo test --manifest-path crates/monocode-gpui/Cargo.toml --locked -p monocode-core --profile native-dev installed_desktop_dir_reads -- --ignored
```

## Preview 1 candidate — October 25, 2026

The [native CI run for `b106d32b`](https://github.com/hardbeat920/monocode/actions/runs/40185511105)
passed. It checked core on macOS, Linux and Windows, and formatting, strict
Clippy, tests and a GUI build on macOS. The same revision passed all 657 GPUI
tests locally, with 11 ignored, plus formatting and strict Clippy. Local fixture
tests required access to loopback networking, file watching and process sampling.

A Linux ARM64 source build was launched and exercised in an Ubuntu 24.04 Noodle
VM. Rust 1.98 avoided a compiler failure with Rust 1.99.1, and a VM-specific
shared-memory workaround with software OpenGL enabled rendering. This was a
launch/UI smoke test, not live provider or packaged Linux x64 upgrade coverage.

Packaged preview-to-preview updates, Windows GUI execution, and macOS
completion/approval alerts, sounds and Dock badges still need runtime testing.

## Windows title bar — October 27, 2026

On Windows 10 22H2 x64, a `native-dev` build was launched and its title bar
exercised by hand. The window showed one bar, the app's tab row. Dragging it
moved the window, double-clicking it maximized and restored the window, and
dragging it to the screen's edge snapped the window to half the screen. The
edges and corners resized the window. Minimize, maximize/restore and close
worked. `WM_NCHITTEST` returned the caption for the bar's empty parts, the
client area for tabs and buttons, the caption buttons for the window controls
and the resize edges for the frame.

All 684 GPUI tests ran serially: 668 passed and 16 failed. The same 16 failed
on `f23cb29c` without these changes, in file-link, worktree, terminal and
pane-layout fixtures. Core tests (589 passed, 2 ignored), native script tests
and formatting passed. Strict Clippy failed on existing compiler warnings in
unrelated files of the Windows build.

Client-decorated Linux and the Windows 11 Snap Layouts flyout were not checked.

## Preview 10 candidate — October 29, 2026

The code at [`bc8c73da`](https://github.com/hardbeat920/monocode/commit/bc8c73daa3eeea03171a402ecdca5f1d1f7e89e7)
passed the complete native check locally on macOS Apple Silicon with Rust
1.99.1, matching the toolchain used by the native CI run. This covers the changes
since `native-v0.16.0-preview.9`.

All jobs in the [native preview workflow](https://github.com/hardbeat920/monocode/actions/runs/40900397284)
and [main CI workflow](https://github.com/hardbeat920/monocode/actions/runs/40900397314)
also passed for this commit. Native Core checks passed on macOS, Linux and
Windows; the native desktop formatting, Clippy, tests and build passed on macOS.

| Check | Result |
| --- | --- |
| Native script tests | 29 passed |
| Native workspace formatting | Passed |
| Strict Clippy for core and GPUI, all targets | Passed |
| Native core tests | 640 passed, 2 ignored |
| GPUI tests, run serially | 747 passed, 11 ignored |
| Native macOS build, `native-dev` profile | Passed |

The final checks used:

```sh
RUSTUP_TOOLCHAIN=1.99.1 npm run native:check
cargo +1.99.1 build --manifest-path crates/monocode-gpui/Cargo.toml --locked -p monocode-gpui --profile native-dev
```

Local fixtures required permission to open loopback sockets and inspect
processes.

These results cover automated fixtures and a local development-profile build.
Preview 10 installers, signing/notarization, packaged upgrades, live heartbeat
and background delegation behavior, and live Hermes reasoning-effort behavior
have not been validated by these checks. Linux and Windows runtime coverage
remains as recorded above and in [preview limitations](parity.md).

### Review fixes

The pre-release review fixes retain a Mono's selected permissions for heartbeat
runs, exclude read-only GitHub review lookups from contribution counts, and
resolve commit IDs and line counts in the command's checkout. Regression cases
cover all five permission modes, submitted versus pending reviews, quoted and
repeated `git -C` options, explicit tool working directories and commits batched
across multiple checkouts. The permission fixture also exposed and now covers
a crash when rendering the Mono's usage-limit notice.

Local revalidation with Rust 1.99.1 passed 29 native script tests, 644 core tests
(2 ignored), and 748 GPUI tests (11 ignored). Native workspace formatting,
strict Clippy for core and GPUI across all targets, and the macOS `native-dev`
build passed.
The linked CI runs above cover the `bc8c73da` baseline; these additional review
fixes were checked locally.

The first candidate CI run exposed a timing race in the existing split-pane
animation test: keyboard simulation could finish the animation before the test
froze its elapsed time. The fixture now creates and freezes the split in one
update, retaining its mid-slide and settled-layout assertions. Keyboard split
behavior remains covered by the separate split-and-drag interaction test.

## Preview 11 candidate — October 30, 2026

The release-review fixes based on `dbec0c5f` passed locally on macOS Apple
Silicon with Rust 1.97.1. Branch naming now uses only the selected provider
and account. The ink canvas leases its animation clock after painting has
initialized its state, and releases it when quiet. Legacy empty-session
choices survive both isolated workspace restoration and desktop preference
reconciliation. Changes to the desktop toggle during an upgrade still apply.

Native dependencies were updated to Tokio 1.54.1, rustls-webpki 0.103.16,
webpki-roots 1.0.11, p256 0.14.0 / crypto-bigint 0.7.10, zstd 0.14.3 /
zstd-safe 8.2.0, and pulldown-cmark 0.13.6. The production lockfile was also
updated for the shared mobile-link requirements and TLS patches.

| Check | Result |
| --- | --- |
| Complete native check, including formatting and strict Clippy for all targets | Passed |
| Native script tests | 29 passed |
| Native core tests | 650 passed, 2 ignored |
| GPUI tests, run serially | 761 passed, 11 ignored |
| Shared phone-link tests in the native workspace | 25 passed |
| Shared phone-link tests in the production workspace | 25 passed |
| Native macOS executable build, `native-dev` profile | Passed |
| Native RustSec vulnerability matches | 0; no advisory IDs ignored |
| Long-history CPU benchmark | Passed for preview 10 and this candidate |

Regression coverage includes unsupported/unavailable branch-name providers,
preserving the selected account, first-paint animation startup, settling to
sleep, waking by pointer interaction, reduced motion, and preference
migration with explicit new choices and subsequent desktop edits. A synthetic
identity and enrollment proof generated with p256 0.13.2 verifies that the new
signing library imports the same saved key, preserves its certificate
fingerprint, verifies the old proof, and produces the same proof bytes. The
fixture is public test data, not a real paired device's credentials.

The RustSec database was refreshed to
`2295466c4e9df26647fd67d8fa1c705ac2fc7f81` before the final scan. Eleven
unmaintained-package entries and one informational Rand logger-soundness
warning remain. Rand 0.7.3 comes through the legacy LevelDB reader; its `log`
feature, required to trigger that advisory, is absent from the resolved
all-platform core feature graph. These warnings were not suppressed. This
native result does not clear the separate React/Tauri and npm dependency
graphs: the production Rust lockfile still has seven vulnerability matches
in zip/zstd dependencies, and the npm findings from the review are unchanged.

### Long-history CPU comparison

The ignored `pane_motion_frame_cost_with_long_history` fixture now freezes the
requested animation position. It continues to require one shell render per
sample, allowing a second only when the slide installs its settled layout.
That settling work remains in the measured time; slow samples are not dropped.

An isolated tracked-source copy of preview 10 used the identical measurement
fix, unchanged vendored dependencies, the same compiler, and the same
`native-dev` profile. After one warm-up per revision, three alternating runs
were measured with no compiler jobs active. Each run sampled 80 positions in
a conversation with 10,000 messages and checked that fewer than 100 visible
rows were rendered.

| CPU measurement | Preview 10 | Candidate |
| --- | --- | --- |
| Median of run p50 values | 12.27 ms | 11.87 ms |
| Median of run p95 values | 13.20 ms | 12.72 ms |
| Range of run p95 values | 12.41–13.28 ms | 12.56–13.65 ms |

The overlapping ranges show no material regression in this benchmark; they do
not establish a speedup. These are CPU measurements at simulated 120 Hz
animation positions, not evidence of 120 fps presentation. GPU costs, live
provider contention, canvas power use in a real window, and packaged upgrades
still need runtime checks. The executable was built without launching it or
opening the installed database. Cross-platform CI and signed installers remain
release-pipeline checks.

## Local checks

Run the complete native check from the repository root:

```sh
npm run native:check
```

Core tests run in parallel. GPUI tests run serially to isolate rendering state
and avoid CPU contention with the interpreter's wall-clock limits.

For focused work, run the relevant part:

```sh
npm run native:test:dev
cargo fmt --manifest-path crates/monocode-gpui/Cargo.toml --all --check
cargo clippy --manifest-path crates/monocode-gpui/Cargo.toml --locked -p monocode-core -p monocode-gpui --all-targets --profile native-dev -- -D warnings
npm run native:test
```

On macOS with a Metal device, the renderer regression opens no window:

```sh
cargo test --manifest-path crates/monocode-gpui/Cargo.toml --locked -p monocode-gpui --profile native-dev overlapping_shell_pane -- --ignored
```

## Optional runtime checks

The provider smoke example uses an existing CLI login. `--models` only discovers
the catalog; without it, the example makes two real provider requests and checks
streaming and resumed context:

```sh
cargo run --manifest-path crates/monocode-gpui/Cargo.toml --locked -p monocode-core --profile native-dev --example provider_smoke -- cursor --models
cargo run --manifest-path crates/monocode-gpui/Cargo.toml --locked -p monocode-core --profile native-dev --example provider_smoke -- cursor
```

For long-history interaction, use the isolated fixture from the
[native guide](README.md#checks-and-fixtures). Check typing and scrolling while a
separate real session streams, then switch tabs and verify draft/viewport state,
Stop, and saved history after relaunch. Synthetic history is not sent to providers.
