-- New installations only. Existing databases are validated, never altered.
CREATE TABLE sessions (
 id TEXT PRIMARY KEY, cwd TEXT NOT NULL, harness TEXT NOT NULL,
 model TEXT NOT NULL, model_settings TEXT NOT NULL DEFAULT '{}',
 runtime_mode TEXT NOT NULL, title TEXT NOT NULL, provider_session_id TEXT,
 blocks_json TEXT NOT NULL DEFAULT '[]', created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL, branch TEXT, context_used INTEGER, context_window INTEGER,
 archived INTEGER NOT NULL DEFAULT 0, worktree_cwd TEXT,
 has_user_message INTEGER NOT NULL DEFAULT 0, pinned INTEGER NOT NULL DEFAULT 0,
 linked_work_item_json TEXT, provider_account_id TEXT,
 worktree_removed INTEGER NOT NULL DEFAULT 0, is_draft INTEGER NOT NULL DEFAULT 0,
 automation_id TEXT, queued_messages_json TEXT NOT NULL DEFAULT '[]',
 queue_status TEXT, sidebar_hidden INTEGER NOT NULL DEFAULT 0, inbox_ask TEXT
);
CREATE INDEX sessions_cwd_updated_idx ON sessions(cwd,updated_at DESC);
CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
INSERT INTO schema_migrations VALUES(19,0);
CREATE TABLE workspace_snapshot(id INTEGER PRIMARY KEY CHECK(id=1),
 snapshot_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE in_flight_sessions(session_id TEXT PRIMARY KEY,cwd TEXT NOT NULL,sort_index INTEGER NOT NULL);
CREATE TABLE notes(id TEXT PRIMARY KEY,slug TEXT NOT NULL UNIQUE,title TEXT NOT NULL,
 body TEXT NOT NULL DEFAULT '',tags_json TEXT NOT NULL DEFAULT '[]',source_session_id TEXT,
 source_cwd TEXT,slug_pending INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL,content_kind TEXT NOT NULL DEFAULT 'note',artifact_kind TEXT);
CREATE TABLE session_reminders(session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
 due_at INTEGER NOT NULL CHECK(due_at>0),fired_at INTEGER);
CREATE INDEX session_reminders_pending ON session_reminders(due_at) WHERE fired_at IS NULL;
