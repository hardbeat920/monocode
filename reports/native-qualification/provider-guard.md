# Provider cleanup after hard host exit

The retained `host/provider-guard.mjs` watches a host-only descriptor. When the host exits, it stops the provider's process group and escalates to SIGKILL after one second. Native Unix process groups and `HarnessShared::Drop` did not preserve that behavior after SIGKILL. The next-launch orphan reaper also does not run when a host crashes.

## Reproduced failure

The fixture compiles an isolated native Rust executable named `codex`. It answers the real version probe and exchanges exact `ping` and `pong` bytes through `HeadlessChildBackend`. It records its PID and process group, starts a native descendant that ignores SIGTERM, and leaves an unrelated sentinel outside the provider group.

The host subprocess uses the actual native host backend in the compiled host test executable. The fixture kills that subprocess with SIGKILL on Unix or force termination on Windows. It does not invoke normal shutdown or Rust Drop. It checks both provider PIDs, the Unix group, and the unrelated sentinel. Its cleanup targets only the isolated fixture processes.

[Mac BEFORE](provider-guard/macos-before.log) failed after the protocol reply and group assertions. Provider PID 55084 and descendant PID 55085 survived the host's hard exit. The fixture then cleaned up its own processes.

## Native behavior

Unix providers now have a native guardian in a separate process group. The host retains the pipe writer in its registered `LiveChild`. The provider records its group before exec. Its writer is close-on-exec, so neither the executed provider nor its descendants retain the host's lifeline. The guardian closes other inherited descriptors and waits for pipe EOF. It sends SIGTERM to the recorded provider group, waits one second, then sends SIGKILL to that group.

Linux creates the pipe with `pipe2` and `O_CLOEXEC`. The Mac opens both endpoints of a private FIFO with `O_CLOEXEC`, then unlinks it before fork. This also prevents a concurrent exec from inheriting a temporary writer during descriptor setup. The FIFO contains only the native provider PID record and leaves no retained filesystem entry.

The parent creates the descriptors and guardian wait thread before the provider starts. After fork, the guardian performs descriptor I/O, group signals, the escalation wait, and `_exit`. It does not allocate, enter Rust runtime cleanup, or call application callbacks. It handles interrupted reads. The existing provider signal-mask reset remains in place.

The same supervisor serves the desktop app and the standalone host. It preserves their executable paths, provider command arguments, output bytes, scoped environment overrides, and session IDs. Windows keeps its existing managed job. The host owns the only job handle, and the OS kills the enrolled provider tree after host termination.

## Focused checks

| Check | Required behavior |
| --- | --- |
| `unexpected_host_exit_stops_the_provider_group` | Actual hard host death stops the provider and its stubborn descendant. The owned group disappears and the unrelated sentinel survives. |
| `ordinary_provider_exit_reaps_the_descendant_and_guard` | The native provider exits with code zero, its stubborn descendant and guardian disappear, and the real host and unrelated sentinel remain alive. |
| `failed_spawn_releases_the_native_guard` | A spawn error closes the pipe and the guardian exits and is reaped. |

The existing process spawn-stamp, replacement, cancellation, kill-all, escalation, and inherited-signal-mask tests keep their assertions. Their manually registered Unix `LiveChild` fixtures now retain a real guardian. The existing host provider protocol fixtures also keep the production guard active.

The Mac BEFORE command used the established broad feature graph:

```sh
cargo test --workspace --lib --bins --all-features --locked unexpected_host_exit_stops_the_provider_group -j 2 -- --nocapture
```

The final Mac AFTER command selects all three new checks:

```sh
cargo test --workspace --lib --bins --all-features --locked provider_guard -j 2 -- --nocapture
```

After that build, the runner launches the exact emitted process and host test executables with `--nocapture --test-threads=2`. This runs their complete suites without rebuilding a narrower feature graph. Strict validation uses:

```sh
cargo clippy --workspace --all-targets --all-features --locked -j 2 -- -D warnings
```

The final Mac descriptor-creation rerun passed all three [focused checks](provider-guard/native-provider-guard-after.log). The exact compiled [process suite](provider-guard/native-provider-guard-process-suite.log) passed 101 tests, and the [host suite](provider-guard/native-provider-guard-host-suite.log) passed 69 tests. [Strict workspace Clippy](provider-guard/native-provider-guard-clippy.log) passed. The final runner exited with code zero.

The [Windows hard-exit fixture](windows-provider-guard-tests.log) passed one test in 0.31 seconds. Its existing job lifetime stopped the provider and descendant while the sentinel survived. [Windows strict workspace Clippy](windows-provider-guard-clippy.log) also passed. The three fixture hashes exactly match the qualified Mac files below. Windows used:

```sh
cargo test --workspace --lib --bins --all-features --locked -j2 provider_guard -- --nocapture
cargo clippy --workspace --all-targets --all-features --locked -j2 -- -D warnings
```

The final-source [Linux checks](linux-provider-guard-final-tests.log) passed both host fixtures and the failed-spawn fixture, three tests in total. [Strict workspace Clippy](linux-provider-guard-final-clippy.log) passed. The runner and outer SSH command both exited with code zero. The [Linux source proof](linux-provider-guard-final-source-sha256.txt) records guard hash `75c06157642d19dfdf2f9372e8a38b20b2de41ba1072a5420016ff64d1d827c7`, the same final source as the Mac qualification below.

## Qualified Mac source

The checks used the isolated hometop task source and Cargo cache with the established all-feature workspace graph. The [local source manifest](provider-guard/source-sha256.json) exactly matches the [remote hashes](provider-guard/source-sha256-hometop.txt) for all seven files. These full-file hashes include the preserved OpenCode environment helper and earlier shared work.

| Source | SHA-256 |
| --- | --- |
| `crates/process/src/lib.rs` | `c8e9dbe071e0cb891db703b044e62459eb462cfbf82d67d1cc6af568284587b7` |
| `crates/process/src/harness.rs` | `6afe21ae4bc6741d3e383fae368b57562de28b48a437a6b5442e91c0e508bed5` |
| `crates/process/src/provider_guard.rs` | `75c06157642d19dfdf2f9372e8a38b20b2de41ba1072a5420016ff64d1d827c7` |
| `crates/host/src/child_backend.rs` | `a06e897388ad4eed24d4fba3844687ff0c0a8f328803c09c052d5c5e700a2c9a` |
| `crates/host/src/child_backend/tests.rs` | `e436c67646c12773be64793787feed7e5cce4c1a751f1ac8dda32897e978585f` |
| `crates/host/src/child_backend/provider_guard_tests.rs` | `70a971d94275a7599d75a75aab4034fecea18cbb99f28b62a31b54c451518288` |
| `crates/host/src/child_backend/provider_guard_fixture.rs` | `9c978606d6695688eb25e53eacf5b22fcf042cbba1cbc67a6833ed6a342296fe` |

The guard qualification uses owned native fixtures and the existing fake-provider suites. It does not claim a fresh paid-provider run against this snapshot. No live provider, production database, package entry point, or user configuration changed.
