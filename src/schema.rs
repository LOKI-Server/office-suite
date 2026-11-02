//! The app's tables. LOKI runs the health check, which calls [`migrate`],
//! before the app goes live, so every statement must be idempotent.
use loki_sdk::{Database, Result, Statement, params};

const STATEMENTS: &[&str] = &[
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
    "CREATE INDEX IF NOT EXISTS documents_by_owner ON documents(owner, trashed_at, updated_at)",
];

pub fn migrate() -> Result<()> {
    let statements = STATEMENTS
        .iter()
        .map(|sql| Statement::new(*sql, params![]))
        .collect::<Result<Vec<_>>>()?;
    Database.transaction(&statements)?;
    Ok(())
}
