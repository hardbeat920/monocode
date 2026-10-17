# Shared skills

The Skills page has a shared library for skills on the local computer. Import a folder containing `SKILL.md`. MonoCode copies the complete bundle into its app data directory and exports the applied version to supported provider folders. The original folder remains unchanged.

Use **Open source folder** to edit the managed copy. **Apply edits** creates a new revision and updates the shared copies. The instructions preview shows the applied revision. Edits in the source folder do not affect provider copies until you apply them.

**Stop sharing** removes copies that MonoCode still owns. It keeps the source, applied revision, and original imported folder. **Start sharing** exports the current applied revision again. **Repair sharing** checks missing exports and reports conflicts.

## Provider folders

| Folder | Providers |
| --- | --- |
| `~/.agents/skills` | Codex, Cursor, Grok, OpenCode, Pi, fx |
| `~/.claude/skills` | Claude Code |
| `~/.omp/agent/skills` | omp |
| `~/.hermes/skills` | Hermes |

MonoCode also exports skills into the actual config directory of a named Claude or Codex account before launching a provider process. It remembers these targets for later edits and sharing changes. Removing an account retires its targets before deleting the account directory.

Antigravity has no automatic export in this library. The existing skill discovery page can still list its files. The library belongs to the local computer even when the open project is remote. Remote host sharing is outside this contribution.

Running provider processes may need a restart to load new or changed native skills. MonoCode keeps Pi and omp native commands under the provider's control. For file skills that MonoCode injects into a prompt, it includes the skill file and resource directory so relative scripts and references resolve without changing the project directory.

## Conflicts and bundle limits

MonoCode records a digest for every export it creates. If another file or folder already occupies the destination, the library reports a conflict and preserves it. If an exported copy changes outside MonoCode, the library preserves that copy and reports a conflict. Repair does not overwrite these files. Move or resolve the conflicting copy yourself, then repair sharing.

Imports preserve file bytes, unknown frontmatter fields, and executable permissions. They materialize links that point inside the imported folder and reject links that escape it or form a cycle. A bundle can contain at most 4,096 entries, 64 MiB of file content, and 32 directory levels. `SKILL.md` can be at most 1 MiB. Imports read metadata with bounded YAML parsing.

The library stores an editable source, an applied object, a registry, and recovery journals under `<app data>/skills`. It uses a process lock and journals to recover interrupted updates. It never treats a matching skill name as proof that a provider file belongs to MonoCode.

## Validation

The Rust library has independent tests for bundle copying, conflicts, edits, sharing changes, recovery, concurrent access, and account target retirement. Tauri tests cover provider roots and launch integration. The UI tests exercise imports, applied previews, sharing changes, repairs, errors, and stale responses.
