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
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_validate() {
        assert!(migrations().validate().is_ok());
    }
}
