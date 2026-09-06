# portable-pty 0.9.0

Source: https://crates.io/crates/portable-pty/0.9.0

Archive SHA-256 (verified against MonoCode's original Cargo.lock):
`b4a596a2b3d2752d94f51fac2d4a96737b8705dddd311a32b9af47211f08671e`

Upstream commit: `f8921727a11b9f8b073e8c24821d72fd41283500` in
https://github.com/wezterm/wezterm (the `pty/` directory).

This copy preserves the published source, manifest, examples, and MIT license.
Package-only metadata (`Cargo.lock`, `Cargo.toml.orig`, `.cargo_vcs_info.json`)
is omitted.

The local patch adds `SlavePty::spawn_command_in_job` on Windows, implemented
by ConPTY using `PROC_THREAD_ATTRIBUTE_JOB_LIST`. Windows assigns the child
to the supplied job during process creation, before startup scripts can create
descendants. The borrowed job and attribute storage stay alive through
`CreateProcessW`; failure never falls back to an unprotected spawn.

Only these upstream files are modified:

- `src/lib.rs`
- `src/win/conpty.rs`
- `src/win/procthreadattr.rs`
- `src/win/psuedocon.rs`

Remove this copy once an upstream release provides equivalent atomic job
assignment. Existing unassigned spawns and Unix behavior are unchanged.
