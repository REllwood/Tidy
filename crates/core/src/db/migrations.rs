use rusqlite_migration::{Migrations, M};

/// Versioned schema migrations. Append new `M::up(...)` entries; never edit
/// shipped ones.
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(
        r#"
        CREATE TABLE page (
            id          TEXT PRIMARY KEY,
            parent_id   TEXT REFERENCES page(id) ON DELETE CASCADE,
            title       TEXT NOT NULL DEFAULT '',
            icon        TEXT,
            type        TEXT NOT NULL DEFAULT 'doc',   -- 'doc' | 'database'
            position    REAL NOT NULL DEFAULT 0,
            is_favorite INTEGER NOT NULL DEFAULT 0,
            content     TEXT,                          -- BlockNote JSON for doc pages
            created_at  INTEGER NOT NULL,
            updated_at  INTEGER NOT NULL
        );
        CREATE INDEX idx_page_parent ON page(parent_id);

        CREATE TABLE database (
            id      TEXT PRIMARY KEY,
            page_id TEXT NOT NULL REFERENCES page(id) ON DELETE CASCADE
        );
        CREATE INDEX idx_database_page ON database(page_id);

        CREATE TABLE field (
            id          TEXT PRIMARY KEY,
            database_id TEXT NOT NULL REFERENCES database(id) ON DELETE CASCADE,
            name        TEXT NOT NULL DEFAULT '',
            type        TEXT NOT NULL,                 -- text|number|select|date|checkbox
            options     TEXT,                          -- JSON (e.g. select choices)
            position    REAL NOT NULL DEFAULT 0
        );
        CREATE INDEX idx_field_database ON field(database_id);

        CREATE TABLE db_row (
            id          TEXT PRIMARY KEY,
            database_id TEXT NOT NULL REFERENCES database(id) ON DELETE CASCADE,
            position    REAL NOT NULL DEFAULT 0,
            created_at  INTEGER NOT NULL
        );
        CREATE INDEX idx_row_database ON db_row(database_id);

        CREATE TABLE cell (
            row_id   TEXT NOT NULL REFERENCES db_row(id) ON DELETE CASCADE,
            field_id TEXT NOT NULL REFERENCES field(id) ON DELETE CASCADE,
            value    TEXT,                             -- JSON typed by field
            PRIMARY KEY (row_id, field_id)
        );
        CREATE INDEX idx_cell_field ON cell(field_id);

        CREATE TABLE db_view (
            id          TEXT PRIMARY KEY,
            database_id TEXT NOT NULL REFERENCES database(id) ON DELETE CASCADE,
            kind        TEXT NOT NULL,                 -- grid|board|calendar
            config      TEXT,                          -- JSON (group-by, date field, ...)
            position    REAL NOT NULL DEFAULT 0
        );
        CREATE INDEX idx_view_database ON db_view(database_id);

        CREATE TABLE model (
            id            TEXT PRIMARY KEY,
            name          TEXT NOT NULL,
            size          INTEGER NOT NULL DEFAULT 0,
            path          TEXT,
            is_selected   INTEGER NOT NULL DEFAULT 0,
            downloaded_at INTEGER
        );

        CREATE TABLE meeting (
            id         TEXT PRIMARY KEY,
            page_id    TEXT REFERENCES page(id) ON DELETE SET NULL,
            started_at INTEGER NOT NULL,
            duration   INTEGER NOT NULL DEFAULT 0,
            audio_path TEXT,
            model_used TEXT
        );

        CREATE TABLE setting (
            key   TEXT PRIMARY KEY,
            value TEXT
        );
        "#,
    ),
    // ---- v2 ----
    M::up(
        r#"
        -- Full-text search over page title + content (standalone FTS5; ids stored UNINDEXED).
        CREATE VIRTUAL TABLE page_fts USING fts5(
            page_id UNINDEXED, title, content, tokenize = 'unicode61'
        );
        INSERT INTO page_fts (page_id, title, content)
            SELECT id, title, coalesce(content, '') FROM page;

        CREATE TRIGGER page_fts_ai AFTER INSERT ON page BEGIN
            INSERT INTO page_fts (page_id, title, content)
            VALUES (new.id, new.title, coalesce(new.content, ''));
        END;
        CREATE TRIGGER page_fts_au AFTER UPDATE ON page BEGIN
            UPDATE page_fts SET title = new.title, content = coalesce(new.content, '')
            WHERE page_id = new.id;
        END;
        CREATE TRIGGER page_fts_ad AFTER DELETE ON page BEGIN
            DELETE FROM page_fts WHERE page_id = old.id;
        END;
        "#,
    ),
    // ---- v3 (the all-in-one pivot: linked knowledge + vault bookkeeping + row promotion) ----
    M::up(
        r#"
        -- Prose / vault bookkeeping on page (vault_* unused until the sync phase; harmless now).
        ALTER TABLE page ADD COLUMN vault_path  TEXT;
        ALTER TABLE page ADD COLUMN body_hash   TEXT;
        ALTER TABLE page ADD COLUMN frontmatter TEXT;
        ALTER TABLE page ADD COLUMN file_mtime  INTEGER;
        ALTER TABLE page ADD COLUMN dirty       INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE page ADD COLUMN deleted_at  INTEGER;
        ALTER TABLE page ADD COLUMN body_text   TEXT;   -- plain-text projection (search / RAG)
        CREATE UNIQUE INDEX idx_page_vault_path ON page(vault_path) WHERE vault_path IS NOT NULL;

        -- Opt-in row promotion (the additive "row = page") + kanban intra-column order.
        ALTER TABLE db_row ADD COLUMN page_id             TEXT REFERENCES page(id) ON DELETE SET NULL;
        ALTER TABLE db_row ADD COLUMN order_within_column REAL;
        CREATE UNIQUE INDEX idx_row_page ON db_row(page_id) WHERE page_id IS NOT NULL;

        -- Universal edge table: backlinks = query; relations = kind='relation' + field_id.
        CREATE TABLE link (
            id             TEXT PRIMARY KEY,
            source_page_id TEXT NOT NULL REFERENCES page(id) ON DELETE CASCADE,
            target_page_id TEXT          REFERENCES page(id) ON DELETE SET NULL,
            dst_title      TEXT,                 -- raw target for unresolved [[links]]
            kind           TEXT NOT NULL,        -- mention | relation | meeting_ref | task_of
            field_id       TEXT REFERENCES field(id) ON DELETE CASCADE,
            context        TEXT,
            created_at     INTEGER NOT NULL
        );
        CREATE INDEX idx_link_source    ON link(source_page_id);
        CREATE INDEX idx_link_target    ON link(target_page_id);
        CREATE INDEX idx_link_dst_title ON link(dst_title);

        -- Tags (normalized index; frontmatter canonical once the vault lands).
        CREATE TABLE tag (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
        CREATE TABLE page_tag (
            page_id TEXT REFERENCES page(id) ON DELETE CASCADE,
            tag_id  INTEGER REFERENCES tag(id) ON DELETE CASCADE,
            PRIMARY KEY (page_id, tag_id)
        );

        -- Sync anti-loop journal (unused until the vault sync phase).
        CREATE TABLE sync_op (
            path TEXT PRIMARY KEY, expected_hash TEXT,
            op   TEXT NOT NULL,               -- app_write | app_delete
            at   INTEGER NOT NULL
        );
        ALTER TABLE database ADD COLUMN vault_dir TEXT;
        "#,
    ),
    M::up(r#"
        CREATE TABLE meeting_ai (
            page_id TEXT PRIMARY KEY REFERENCES page(id) ON DELETE CASCADE,
            state TEXT NOT NULL DEFAULT 'queued',
            summary TEXT,
            error TEXT,
            source_hash TEXT,
            updated_at INTEGER NOT NULL DEFAULT 0
        );
        INSERT OR IGNORE INTO meeting_ai(page_id)
            SELECT page_id FROM meeting WHERE page_id IS NOT NULL;
        CREATE TABLE meeting_chunk (
            id TEXT PRIMARY KEY,
            page_id TEXT NOT NULL REFERENCES page(id) ON DELETE CASCADE,
            block_id TEXT,
            position INTEGER NOT NULL,
            text TEXT NOT NULL,
            timestamp TEXT,
            embedding TEXT,
            embedding_model TEXT
        );
        CREATE INDEX meeting_chunk_page ON meeting_chunk(page_id);
        CREATE VIRTUAL TABLE meeting_chunk_fts USING fts5(id UNINDEXED, text);
        CREATE TRIGGER meeting_chunk_ai AFTER INSERT ON meeting_chunk BEGIN
            INSERT INTO meeting_chunk_fts(id,text) VALUES(new.id,new.text);
        END;
        CREATE TRIGGER meeting_chunk_ad AFTER DELETE ON meeting_chunk BEGIN
            DELETE FROM meeting_chunk_fts WHERE id=old.id;
        END;
        CREATE TRIGGER meeting_ai_insert AFTER INSERT ON meeting WHEN new.page_id IS NOT NULL BEGIN
            INSERT OR IGNORE INTO meeting_ai(page_id) VALUES(new.page_id);
        END;
        CREATE TRIGGER meeting_ai_edit AFTER UPDATE OF content ON page WHEN old.content IS NOT new.content BEGIN
            UPDATE meeting_ai SET state='queued',summary=NULL,error=NULL,source_hash=NULL WHERE page_id=new.id;
            DELETE FROM meeting_chunk WHERE page_id=new.id;
        END;
    "#),
    M::up(r#"
        ALTER TABLE meeting ADD COLUMN transcript_state TEXT NOT NULL DEFAULT 'saved';
        ALTER TABLE meeting ADD COLUMN transcript_error TEXT;
        CREATE TABLE transcript_version (
            id TEXT PRIMARY KEY,
            meeting_id TEXT NOT NULL REFERENCES meeting(id),
            created_at INTEGER NOT NULL,
            model TEXT,
            language TEXT,
            body_json TEXT NOT NULL,
            reason TEXT NOT NULL
        );
        CREATE INDEX transcript_version_meeting ON transcript_version(meeting_id, created_at);
        INSERT INTO transcript_version(id,meeting_id,created_at,model,body_json,reason)
            SELECT lower(hex(randomblob(16))),m.id,m.started_at,m.model_used,p.content,'Existing transcript'
            FROM meeting m JOIN page p ON p.id=m.page_id WHERE p.content IS NOT NULL;
    "#),
    M::up("ALTER TABLE meeting ADD COLUMN retain_audio INTEGER NOT NULL DEFAULT 1;")])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_keeps_existing_meeting_text_and_audio_reference() {
        let mut c = rusqlite::Connection::open_in_memory().unwrap();
        migrations().to_version(&mut c, 4).unwrap();
        let body = r#"[{"type":"paragraph","content":"Original client handover"}]"#;
        c.execute("INSERT INTO page(id,title,content,created_at,updated_at) VALUES('page','Handover',?1,1,1)", [body]).unwrap();
        c.execute("INSERT INTO meeting(id,page_id,started_at,audio_path,model_used) VALUES('meeting','page',1,'/saved.wav','base')", []).unwrap();
        migrations().to_latest(&mut c).unwrap();
        let row: (String,String,String) = c.query_row("SELECT m.audio_path,m.transcript_state,v.body_json FROM meeting m JOIN transcript_version v ON v.meeting_id=m.id", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!(row, ("/saved.wav".into(),"saved".into(),body.into()));
        assert_eq!(c.query_row("SELECT content FROM page WHERE id='page'", [], |r| r.get::<_,String>(0)).unwrap(), body);
    }

    #[test]
    fn migrations_validate() {
        assert!(migrations().validate().is_ok());
    }
}
