use super::{Session, Store};
use rusqlite::OptionalExtension;

impl Store {
    /// Look up an existing Mono conversation independently of the sidebar.
    /// Hidden sessions are valid conversations and must never be replaced.
    pub fn mono_session(&self, id: i64) -> Result<Option<Session>, String> {
        if !self.uses_monocode_schema() {
            return Ok(self
                .sessions()?
                .into_iter()
                .find(|session| session.id == id));
        }
        let original = self.persisted_session_id(id)?;
        let columns = [
            "title".to_string(),
            "harness".into(),
            "cwd".into(),
            "provider_session_id".into(),
            "updated_at".into(),
            self.shared_field(
                "has_user_message",
                "CASE WHEN blocks_json!='[]' THEN 1 ELSE 0 END",
            ),
            self.shared_field("archived", "0"),
            self.shared_field("pinned", "0"),
            self.shared_field("linked_work_item_json", "NULL"),
        ]
        .join(",");
        self.conn
            .query_row(
                &format!("SELECT {columns} FROM sessions WHERE id=?1"),
                [original],
                |row| {
                    Ok(Session {
                        id,
                        title: row.get(0)?,
                        provider: row.get(1)?,
                        cwd: row.get(2)?,
                        provider_id: row.get(3)?,
                        updated_at: row.get::<_, i64>(4)?.max(0) as u64,
                        has_messages: row.get(5)?,
                        archived: row.get(6)?,
                        pinned: row.get(7)?,
                        work_item: row
                            .get::<_, Option<String>>(8)?
                            .and_then(|raw| serde_json::from_str(&raw).ok()),
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())
    }
    /// Cheap freshness token; the blocks themselves are loaded only on change.
    pub fn mono_work_revision(&self, id: i64) -> Result<Option<(u64, u64, u64)>, String> {
        if !self.uses_monocode_schema() {
            return Ok(None);
        }
        let original = self.persisted_session_id(id)?;
        let updated: i64 = self
            .conn
            .query_row(
                "SELECT updated_at FROM sessions WHERE id=?1",
                [original],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let notes = if super::shared::has_table(&self.conn, "notes")? {
            self.conn
                .query_row(
                    "SELECT updated_at FROM notes ORDER BY updated_at DESC LIMIT 1",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .optional()
                .map_err(|e| e.to_string())?
                .unwrap_or(0)
        } else {
            0
        };
        let version = self
            .conn
            .query_row("PRAGMA data_version", [], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())?;
        Ok(Some((
            updated.max(0) as u64,
            notes.max(0) as u64,
            version.max(0) as u64,
        )))
    }
    /// Read the exact note/artifact referenced by a card, including its HTML body.
    pub fn mono_document(&self, id: &str) -> Result<Option<crate::mono::Document>, String> {
        self.read_mono_document(id, true)
    }
    fn read_mono_document(
        &self,
        id: &str,
        body: bool,
    ) -> Result<Option<crate::mono::Document>, String> {
        if !self.uses_monocode_schema() || !super::shared::has_table(&self.conn, "notes")? {
            return Ok(None);
        }
        let has_kind: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('notes') WHERE name='artifact_kind')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        let kind = if has_kind {
            "COALESCE(artifact_kind,'document')"
        } else {
            "'document'"
        };
        let body_column = if body { "body" } else { "''" };
        self.conn
            .query_row(
                &format!(
                    "SELECT id,{kind},title,{body_column},created_at,updated_at FROM notes WHERE id=?1"
                ),
                [id],
                |r| {
                    Ok(crate::mono::Document {
                        id: r.get(0)?,
                        kind: r.get(1)?,
                        title: r.get(2)?,
                        summary: String::new(),
                        body: r.get(3)?,
                        created_at: r.get::<_, i64>(4)?.max(0) as u64,
                        updated_at: r.get::<_, i64>(5)?.max(0) as u64,
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())
    }
    /// Metadata comes from the saved conversation, in both indexed Mono history
    /// and older sessions.blocks_json. No provider rollout is opened here.
    pub fn mono_work(&self, id: i64) -> Result<Option<crate::mono::Work>, String> {
        if !self.uses_monocode_schema() {
            return Ok(None);
        }
        let original = self.persisted_session_id(id)?;
        let indexed = super::shared::has_table(&self.conn, "mono_transcripts")?
            && self
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM mono_transcripts WHERE session_id=?1)",
                    [&original],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(|e| e.to_string())?;
        // Return only references and confirmed app receipts from SQLite. A Mono
        // can have megabytes of prose/tools; none of that crosses this boundary.
        let source = if indexed {
            "SELECT block_json AS block,seq FROM mono_blocks WHERE session_id=?1"
        } else {
            "SELECT value AS block,key AS seq FROM sessions,json_each(sessions.blocks_json) WHERE sessions.id=?1"
        };
        let query = format!(
            r#"SELECT json_object('id',json_extract(block,'$.id'),'role',json_extract(block,'$.role'),
            'artifactCards',json_extract(block,'$.artifactCards'),'monoSpawnedSessions',json_extract(block,'$.monoSpawnedSessions'),
            'text',CASE WHEN json_extract(block,'$.role') IN ('tool','visual') THEN json_extract(block,'$.text') ELSE '' END,
            'sentAt',json_extract(block,'$.sentAt'),
            'tool',CASE WHEN json_extract(block,'$.role')='tool' THEN json_extract(block,'$.tool') ELSE NULL END)
            FROM ({source}) WHERE json_type(block,'$.artifactCards')='array' OR json_type(block,'$.monoSpawnedSessions')='array'
            OR json_extract(block,'$.role')='visual'
            OR (json_extract(block,'$.role')='tool' AND json_extract(block,'$.tool.status') IN ('completed','success')
                AND COALESCE(json_extract(block,'$.tool.preview.title'),json_extract(block,'$.tool.title'),json_extract(block,'$.text'),'') LIKE '%monocode%app%sessions.%') ORDER BY seq"#
        );
        let mut statement = self.conn.prepare(&query).map_err(|e| e.to_string())?;
        let values = statement
            .query_map([&original], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let mut blocks = Vec::new();
        for value in values {
            blocks.push(
                serde_json::from_str::<serde_json::Value>(&value.map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            );
        }
        let mut documents: Vec<crate::mono::Document> = Vec::new();
        let mut launches = Vec::new();
        for block in &blocks {
            // Native visuals published in the Mono's chat are its visual documents.
            if let Some(document) = crate::mono::work::visual_document(block) {
                if let Some(old) = documents.iter_mut().find(|d| d.id == document.id) {
                    *old = crate::mono::Document {
                        created_at: old.created_at,
                        ..document
                    };
                } else {
                    documents.push(document);
                }
                continue;
            }
            for card in crate::mono::work::document_cards(block) {
                let id = card["documentId"].as_str().unwrap();
                let mut document =
                    self.read_mono_document(id, false)?
                        .unwrap_or(crate::mono::Document {
                            id: id.into(),
                            kind: card["kind"].as_str().unwrap().into(),
                            title: card["title"].as_str().unwrap().into(),
                            summary: String::new(),
                            body: String::new(),
                            created_at: 0,
                            updated_at: 0,
                        });
                document.summary = card["summary"].as_str().unwrap_or_default().into();
                if let Some(old) = documents
                    .iter_mut()
                    .find(|d: &&mut crate::mono::Document| d.id == id)
                {
                    *old = document;
                } else {
                    documents.push(document);
                }
            }
        }
        blocks.retain(|block| block["role"] != "visual");
        for (index, values) in crate::mono::work::block_launches(&blocks)
            .into_iter()
            .enumerate()
        {
            for value in values {
                let external = value["sessionId"].as_str().unwrap();
                // Retain deleted references too: unavailable filtering needs them.
                let session_id = self.shared_register(external)?;
                if launches
                    .iter()
                    .any(|l: &crate::mono::Launch| l.session_id == session_id)
                {
                    continue;
                }
                launches.push(crate::mono::Launch {
                    session_id,
                    cwd: value["cwd"].as_str().unwrap().into(),
                    title: value["title"].as_str().unwrap().into(),
                    harness: value["harness"].as_str().unwrap().into(),
                    model: value["model"].as_str().map(str::to_owned),
                    origin_turn: index as u64,
                    notify_on_complete: false,
                    completed: true,
                    dismissed: false,
                });
            }
        }
        Ok(Some((documents, launches)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn hidden_mono_summary_preserves_original_session_and_provider() {
        let store = Store::open_monocode(Path::new(":memory:")).unwrap();
        let session = store.create("claude", "/project").unwrap();
        let original = store.persisted_session_id(session.id).unwrap();
        store.conn.execute(
            "UPDATE sessions SET sidebar_hidden=1,provider_session_id='existing-thread' WHERE id=?1",
            [&original],
        ).unwrap();
        assert!(store.sessions().unwrap().is_empty());
        let loaded = store.mono_session(session.id).unwrap().unwrap();
        assert_eq!(loaded.id, session.id);
        assert_eq!(loaded.provider, "claude");
        assert_eq!(loaded.provider_id.as_deref(), Some("existing-thread"));
        assert_eq!(store.persisted_session_id(loaded.id).unwrap(), original);
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM sessions", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn published_visuals_become_mono_visual_documents() {
        use serde_json::json;
        let store = Store::open_monocode(Path::new(":memory:")).unwrap();
        let mono = store.create("claude", "/mono").unwrap();
        let original = store.persisted_session_id(mono.id).unwrap();
        let visual = |title: &str| {
            json!({"id":"deploys","title":title,"source":"return () => visual.text('Ready');"})
                .to_string()
        };
        let blocks = json!([
            {"id":"a","role":"visual","text":visual("Deploys"),"sentAt":10},
            {"id":"b","role":"assistant","text":"Here it is"},
            {"id":"c","role":"visual","text":visual("Deploys per week"),"sentAt":20},
            {"id":"d","role":"visual","text":"not a visual"}
        ]);
        store
            .conn
            .execute(
                "UPDATE sessions SET blocks_json=?2 WHERE id=?1",
                rusqlite::params![original, blocks.to_string()],
            )
            .unwrap();
        let (documents, launches) = store.mono_work(mono.id).unwrap().unwrap();
        assert!(launches.is_empty());
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].id, "visual:deploys");
        assert_eq!(documents[0].kind, "visual");
        assert_eq!(documents[0].title, "Deploys per week");
        assert_eq!((documents[0].created_at, documents[0].updated_at), (10, 20));
        let loaded: crate::visual::VisualDocument =
            serde_json::from_str(&documents[0].body).unwrap();
        assert_eq!(loaded.title, "Deploys per week");
    }
    #[test]
    fn desktop_work_uses_saved_block_references_and_note_bodies_without_mutation() {
        use serde_json::json;
        for indexed in [false, true] {
            let store = Store::open_monocode(Path::new(":memory:")).unwrap();
            let mono = store.create("codex", "/mono").unwrap();
            let child = store.create("claude", "/moved-project").unwrap();
            let original = store.persisted_session_id(mono.id).unwrap();
            let child_id = store.persisted_session_id(child.id).unwrap();
            let blocks = json!([
                {"id":"user","role":"user","text":"Write it","artifactCards":[{"id":"existing-artifact","kind":"html","title":"Original title","summary":"Overview"}],"monoSpawnedSessions":[{"sessionId":child_id,"cwd":"/old-project","title":"Launch title","harness":"claude","model":"opus"}]},
                {"id":"assistant","role":"assistant","text":"Done"}
            ]);
            store.conn.execute("INSERT INTO notes(id,slug,title,body,tags_json,created_at,updated_at,content_kind,artifact_kind) VALUES('existing-artifact','existing-artifact','Saved title','<h1>Actual HTML</h1>','[]',10,20,'artifact','html')", []).unwrap();
            if indexed {
                store.conn.execute_batch("CREATE TABLE mono_transcripts(session_id TEXT PRIMARY KEY); CREATE TABLE mono_blocks(session_id TEXT,seq INTEGER,block_json TEXT);").unwrap();
                store
                    .conn
                    .execute("INSERT INTO mono_transcripts VALUES(?1)", [&original])
                    .unwrap();
                for (seq, block) in blocks.as_array().unwrap().iter().enumerate() {
                    store
                        .conn
                        .execute(
                            "INSERT INTO mono_blocks VALUES(?1,?2,?3)",
                            rusqlite::params![
                                original,
                                i64::try_from(seq).unwrap(),
                                block.to_string()
                            ],
                        )
                        .unwrap();
                }
            } else {
                store
                    .conn
                    .execute(
                        "UPDATE sessions SET blocks_json=?2 WHERE id=?1",
                        rusqlite::params![original, blocks.to_string()],
                    )
                    .unwrap();
            }
            let before = store.shared_blocks(mono.id).unwrap();
            let (documents, launches) = store.mono_work(mono.id).unwrap().unwrap();
            assert_eq!(documents.len(), 1);
            assert_eq!(documents[0].title, "Saved title");
            assert_eq!(documents[0].kind, "html");
            assert!(documents[0].body.is_empty());
            assert_eq!(
                store
                    .mono_document("existing-artifact")
                    .unwrap()
                    .unwrap()
                    .body,
                "<h1>Actual HTML</h1>"
            );
            assert_eq!(documents[0].summary, "Overview");
            assert_eq!(launches.len(), 1);
            assert_eq!(launches[0].session_id, child.id);
            assert_eq!(
                store
                    .mono_session(launches[0].session_id)
                    .unwrap()
                    .unwrap()
                    .cwd,
                "/moved-project"
            );
            assert_eq!(store.shared_blocks(mono.id).unwrap(), before);
            assert!(store.mono_work_revision(mono.id).unwrap().is_some());
        }
    }
}
