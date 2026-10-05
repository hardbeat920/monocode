# Native documentation drafts

These files are review copies for the M6 source cutover. Canonical documentation and workflows have not changed. Promote each file only with the coordinated cutover commit and the final qualification report.

| Draft | Intended destination |
| --- | --- |
| `README.md` | Root `README.md` |
| `CONTRIBUTING.md` | Root `CONTRIBUTING.md` |
| `docs/remote-access.md` | `docs/remote-access.md` |
| `.github/pull_request_template.md` | `.github/pull_request_template.md` |
| `.cargo/config.toml` | `.cargo/config.toml` |
| `golden_json_bytes.rs` | Original proposal. Root promoted the actual test to `crates/core/tests/golden_json_bytes.rs`, and its private-copy byte check passed. |

Links in the replacement documents target their canonical destinations. The commands come from the native CLI, Cargo manifests, packaging tool, and native workflows in this checkout. They do not claim that a native release is already published.

Provider installation instructions remain with each provider. An external provider may require Node. The MonoCode desktop and host build do not. Rust provider transport tests still need Node for inline fixtures, and the contributor draft states that test dependency.

The source still retains the legacy implementation. The literal live Claude gate remains blocked by the organization's disabled subscription access. Native Codex, Cursor, and Droid evidence does not substitute for that gate. The separate native OpenCode 2 adapter passed its protocol fixtures and one private zero-price model turn. [OpenCode qualification](../opencode-v2/qualification.md) records that snapshot and its limits. Package signing, installation, updater relaunch, and actual remote window qualification must use the final binary.

The editable macOS icon and installer sidebar bitmap now have byte-equal independent copies under `packaging`. Their hashes and intended retention are in the cutover inventory.
