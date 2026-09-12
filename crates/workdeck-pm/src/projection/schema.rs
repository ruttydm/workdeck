use crate::{ErrorCode, PmError, Result};
use rusqlite::{Connection, Transaction};

pub(super) fn sql_error(error: rusqlite::Error) -> PmError {
    PmError::new(
        ErrorCode::InvalidSchema,
        format!("projection database: {error}"),
    )
}

pub(super) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS projection_documents (
            path TEXT PRIMARY KEY NOT NULL, kind TEXT NOT NULL, content TEXT NOT NULL,
            bytes INTEGER NOT NULL, document TEXT
         );
         CREATE TABLE IF NOT EXISTS projection_records (
            key TEXT PRIMARY KEY NOT NULL, kind TEXT NOT NULL, id TEXT NOT NULL,
            path TEXT NOT NULL, content TEXT NOT NULL, title TEXT NOT NULL,
            title_lower TEXT NOT NULL, search TEXT NOT NULL,
            archived INTEGER NOT NULL, retired INTEGER NOT NULL,
            status TEXT, priority INTEGER, assignee TEXT, project TEXT, cycle TEXT,
            milestone TEXT, due_at TEXT, parent TEXT, created_at TEXT, updated_at TEXT,
            decision TEXT, maturity TEXT, availability TEXT,
            UNIQUE(kind,id)
         );
         CREATE INDEX IF NOT EXISTS projection_record_path ON projection_records(path);
         CREATE INDEX IF NOT EXISTS projection_record_order ON projection_records(kind,archived,created_at,id);
         CREATE INDEX IF NOT EXISTS projection_record_status ON projection_records(kind,status,archived,id);
         CREATE INDEX IF NOT EXISTS projection_record_parent ON projection_records(kind,parent,id);
         CREATE INDEX IF NOT EXISTS projection_record_assignee ON projection_records(kind,assignee,archived,id);
         CREATE TABLE IF NOT EXISTS projection_values (
            key TEXT NOT NULL, field TEXT NOT NULL, value TEXT NOT NULL,
            PRIMARY KEY(key,field,value)
         );
         CREATE INDEX IF NOT EXISTS projection_value_lookup ON projection_values(field,value,key);
         CREATE TABLE IF NOT EXISTS projection_edges (
            from_kind TEXT NOT NULL, from_id TEXT NOT NULL,
            to_kind TEXT NOT NULL, to_id TEXT NOT NULL, relation TEXT NOT NULL,
            path TEXT NOT NULL, content TEXT NOT NULL,
            PRIMARY KEY(from_kind,from_id,to_kind,to_id,relation,path)
         );
         CREATE INDEX IF NOT EXISTS projection_edge_to ON projection_edges(to_kind,to_id,relation);
         CREATE INDEX IF NOT EXISTS projection_edge_path ON projection_edges(path);
         CREATE VIRTUAL TABLE IF NOT EXISTS projection_search USING fts5(key UNINDEXED,title,body);
         CREATE TABLE IF NOT EXISTS projection_configuration (id INTEGER PRIMARY KEY CHECK(id=1), document TEXT NOT NULL);"
    ).map_err(sql_error)
}

pub(super) fn validate(connection: &Connection) -> Result<()> {
    for (table, columns) in [
        ("projection_documents", "path,kind,content,bytes,document"),
        (
            "projection_records",
            "key,kind,id,path,content,title,title_lower,search,archived,retired,status,priority,assignee,project,cycle,milestone,due_at,parent,created_at,updated_at,decision,maturity,availability",
        ),
        ("projection_values", "key,field,value"),
        (
            "projection_edges",
            "from_kind,from_id,to_kind,to_id,relation,path,content",
        ),
        ("projection_search", "key,title,body"),
        ("projection_configuration", "id,document"),
    ] {
        connection
            .prepare(&format!("SELECT {columns} FROM {table} LIMIT 0"))
            .map_err(sql_error)?;
    }
    Ok(())
}
