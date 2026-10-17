# Local skill library

`SkillManager` imports a complete directory into an editable working copy. Apply validates that copy and stores an immutable content snapshot. The registry records skill IDs, revisions, bundle digests, original import directories, provider targets, and export ownership. Its schema version is 1.

The library creates copies in documented local skill directories. Each status describes the files on this host. An exported status does not establish provider loading or dependency availability. Antigravity has no default export because its active personal runtime directory is unresolved.

Reconciliation replaces or removes a directory only if the current bytes and executable modes match its recorded ownership. Unmanaged directories and externally edited exports produce conflicts. Stop sharing removes unchanged owned copies and preserves conflicts. It does not remove unmanaged skills or instructions already loaded by a conversation.

All manager calls that change files hold an OS file lock and reload the registry. An atomic manifest records each directory replacement before the manager changes the destination. Restarting the manager recovers interrupted operations, finishes a complete staged revision, or restores the previous complete revision. A conflict preserves the destination and transaction copies. Recovery does not overwrite an external edit.

Supply absolute data and home directories to `SkillManager::open`. Storage lives in the data directory's `skills` folder. Tests use temporary data and home directories. Account exports use explicit resolved `ExportTarget` values and persist in the registry so later Apply and Stop sharing operations cover them.

`snapshot` reads the generation, skill entries, and remembered export targets under one registry lock. The public JSON types use snake_case field names. Export state values are `exported`, `pending`, `conflict`, `disabled`, and `unsupported`. Paths serialize as strings.

The importer accepts at most 4096 files and directories, 64 MiB of content, 32 directory levels, and a 1 MiB `SKILL.md`. It parses complete YAML frontmatter and preserves the source bytes and unknown fields. It reads at most 256 frontmatter fields. Names and descriptions have character limits, optional license and compatibility strings have a 4096 character limit, and unknown values remain unexpanded. Internal symbolic links become copies. External links, cycles, special files, and paths that cannot be represented on supported platforms fail validation.

Run the focused checks with `cargo test -p monocode-skills` and `cargo clippy -p monocode-skills --all-targets -- -D warnings`.

The headless example uses the same service for fixture preparation and local administration. Every result is JSON. Supply isolated directories when preparing a preview.

```sh
cargo run -p monocode-skills --example manage -- /tmp/skill-preview/data /tmp/skill-preview/home import /absolute/path/to/skill
cargo run -p monocode-skills --example manage -- /tmp/skill-preview/data /tmp/skill-preview/home list
```

The example also accepts `apply <id>`, `share <id> on`, `share <id> off`, and `repair`.
