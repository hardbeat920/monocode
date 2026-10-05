# Retained Rust check preparation

The root `check` script runs both `check:web` and `check:rust`. The coordinator has verified 4,075 web tests and TypeScript checking. That web pass alone does not complete the retained build check.

`check:rust` runs these checks against the retained Tauri application:

```sh
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

The retained application has its own workspace and `src-tauri/Cargo.lock`. The native root workspace excludes `src-tauri`. The retained manifest now depends on the shared platform, process, Git, store, terminal, integrations, and remote crates. Those crates inherit dependency definitions from the native root manifest. The existing isolated hometop source already has that root manifest, its shared crates, and the portable PTY vendor source.

The retained source staging must include its Rust files, build script, capability definitions, Tauri configuration files, icons, and macOS asset catalog. It must exclude the local `src-tauri/target` directory. The configuration points at a development URL, so these development-profile checks do not require launching Vite or the retained GUI. The build script and context macro require the real icon and asset files.

The [prepared runner](check-retained-rust.sh) uses hometop's task-owned toolchain. It uses a separate `retained-cargo` cache and `retained-target`, with two build jobs and debug information disabled. It clones the existing task registry cache through APFS, tries an offline dependency fetch first, and allows a scoped fetch into that isolated cache if needed. It preserves the retained lockfile before and after dependency resolution and records any difference. It uses `--locked` for Clippy and tests after that resolution.

The runner completed after the coordinator released the shared remote Cargo work. Formatting and strict Clippy passed. All 35 library tests passed. The binary and documentation targets had no tests and passed. The [summary](retained-rust/summary.json), [final Clippy log](retained-rust/minimal-clippy.log), and [final test log](retained-rust/minimal-test.log) preserve the result.

The first dependency resolution succeeded offline but also updated two unrelated Wayland packages. Those package blocks were restored to the staged original versions. The final lockfile adds only the required shared platform dependency edges and three Core Media packages. The [minimal lock difference](retained-rust/minimal-lock.diff) records the final change. The isolated cache fetched the two original Wayland package archives, and the final runner kept the lockfile unchanged through its locked Clippy and test checks. The local `src-tauri/Cargo.lock` matches that qualified minimal lockfile. No Rust source fix or baseline failure occurred.

No local Cargo build or production application launch ran. These checks qualify the retained application build and tests on macOS. They do not prove Windows or Linux retained application behavior, or an actual retained desktop GUI connection to the native host.
