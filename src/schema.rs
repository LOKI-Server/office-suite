//! The app's tables. LOKI runs the health check, which calls [`migrate`],
//! before the app goes live, so every statement must be idempotent.
use loki_sdk::{Database, Result, Statement, params};

const STATEMENTS: &[&str] = &[
    // Every tool's files. `plain` and `body` come last so list queries, which
    // skip them, never read their overflow pages.
    "CREATE TABLE IF NOT EXISTS files(
        id TEXT PRIMARY KEY,
        owner TEXT NOT NULL,
        kind TEXT NOT NULL,
        title TEXT NOT NULL,
        excerpt TEXT NOT NULL DEFAULT '',
        info TEXT NOT NULL DEFAULT '',
        version INTEGER NOT NULL DEFAULT 1,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        trashed_at INTEGER,
        plain TEXT NOT NULL DEFAULT '',
        body TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS files_by_owner ON files(owner, trashed_at, updated_at)",
    // Docs 0.1 kept documents in a table of their own: move any into `files`.
    "CREATE TABLE IF NOT EXISTS documents(
        id TEXT PRIMARY KEY,
        owner TEXT NOT NULL,
        title TEXT NOT NULL,
        body TEXT NOT NULL,
        excerpt TEXT NOT NULL DEFAULT '',
        plain TEXT NOT NULL DEFAULT '',
        words INTEGER NOT NULL DEFAULT 0,
        version INTEGER NOT NULL DEFAULT 1,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        trashed_at INTEGER
    )",
    "INSERT OR IGNORE INTO files(id,owner,kind,title,excerpt,info,version,created_at,updated_at,trashed_at,plain,body)
     SELECT id,owner,'doc',title,excerpt,CASE words WHEN 1 THEN '1 word' ELSE words || ' words' END,
            version,created_at,updated_at,trashed_at,plain,body
     FROM documents",
    "DELETE FROM documents",
];

pub fn migrate() -> Result<()> {
    let statements = STATEMENTS
        .iter()
        .map(|sql| Statement::new(*sql, params![]))
        .collect::<Result<Vec<_>>>()?;
    Database.transaction(&statements)?;
    Ok(())
}
