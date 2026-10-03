# Notes for the lead from the workspace package

Each item has a local workaround in place. None blocks the package.

## Shared files I changed

- crates/engine/Cargo.toml: the `workspace` feature now also turns on
  `dep:monocode-terminal` and `dep:monocode-terminal-view`, added as
  optional dependencies. The conventions table lists terminal-view below
  the engine, but the task asked for the `Pty` adapter here.

## Runtime

- `WorkspaceHooks::hydrate_snapshot` gets the interrupted ids as a
  `HashSet`. The TypeScript added tabs for interrupted chats in quit-list
  order. I sort the ids by value. Passing a `Vec` would keep the order.
- No hook reaches the history, remote, or sidebar code that tab closing
  calls (`refreshHistory`, `rememberRemoteSession`,
  `rememberRemotePendingWorktree`, `remoteSessionFor`, `setRecents`). The
  workspace takes a `WorkspaceDelegate` with no-op defaults instead.
- `Sessions` has no way to drop only a loaded-cache entry
  (`loadedSessionCache.delete`). `focus_open_session` calls
  `invalidate_loaded`, which also bumps the load epoch.
- The workspace implements `HasId` and `TransferTab` for `WorkspaceTab`
  (and `HasId` for `FilePaneTab`). Another package adding the same impls
  would conflict.

## Layout

- `monocode_layout::js` is private now, so `decode_uri_component` is
  copied into workspace/paths.rs.

## Other crates

- monocode-terminal: `PtyStatus.foreground` is private, so the adapter
  reads it back through serde. `pty_write` takes a `String`, so input bytes
  that are not UTF-8 (X10 mouse reports past column 95) are replaced.
- monocode-git: `DirEntry` and `FileMtime` keep their fields private; the
  file backend reads them back through serde.
- `resolve_workspace_path`, `join_path`, and `parent_path` now exist in
  both workspace/paths.rs and view-transcript. They belong in
  `monocode_core::paths`.
- workspace/chat_context.rs ports only the part of chatContext.ts that
  add-to-chat needs. The composer package should own the full module,
  ideally in core, and the workspace should import it.
