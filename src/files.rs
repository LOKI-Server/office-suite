//! Every tool keeps its files in one table, so the home page, search and the
//! trash work across the suite. A tool stores its content as JSON in `body`
//! and, when saving, supplies the summary used by lists and search.
//!
//! Every query is scoped to the signed-in owner, so another person's file id
//! behaves exactly like one that does not exist.
use crate::ui::{Shell, Tool, now_ms, page, relative_time};
use crate::{docs, sheets};
use askama::Template;
use loki_sdk::{Database, Error, Request, Response, Result, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

/// Trashed files are deleted for good after this many days.
pub const TRASH_DAYS: i64 = 30;
const DAY_MS: i64 = 86_400_000;
const LIST_LIMIT: i64 = 500;
pub const MAX_TITLE_CHARS: usize = 200;
/// An autosave request; generous because JSON adds overhead to content limits.
pub const MAX_SAVE_BYTES: usize = 2 * 1024 * 1024;
/// LOKI stores at most 1 MiB per database value.
const MAX_STORED_BYTES: usize = 900 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Doc,
    Sheet,
}
impl Kind {
    pub fn tool(self) -> &'static Tool {
        match self {
            Kind::Doc => &docs::DOCS,
            Kind::Sheet => &sheets::SHEETS,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Kind::Doc => "doc",
            Kind::Sheet => "sheet",
        }
    }
}

pub fn owner(request: &Request) -> Result<String> {
    Ok(request.context().require_user()?.id.clone())
}
fn not_found() -> Error {
    Error::not_found("This file does not exist, or it isn't yours.")
}

/// Trims a title and falls back to "Untitled …" when it is blank.
pub fn clean_title(title: &str, kind: Kind) -> Result<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(Error::bad(format!("Titles can be at most {MAX_TITLE_CHARS} characters.")));
    }
    if title.chars().any(char::is_control) {
        return Err(Error::bad("Titles cannot contain control characters."));
    }
    Ok(if title.is_empty() {
        format!("Untitled {}", kind.tool().noun)
    } else {
        title
    })
}

#[derive(Deserialize)]
struct Summary {
    id: String,
    kind: Kind,
    title: String,
    excerpt: String,
    info: String,
    updated_at: i64,
    trashed_at: Option<i64>,
}
const SUMMARY: &str = "SELECT id,kind,title,excerpt,info,updated_at,trashed_at FROM files";

/// A file in a list, ready to display.
pub struct Row {
    pub id: String,
    pub title: String,
    pub excerpt: String,
    pub detail: String,
    /// The tool's icon template name, such as `doc`.
    pub icon: &'static str,
    pub empty: &'static str,
    pub href: String,
    pub view: String,
    pub exports: Vec<(&'static str, String)>,
}
impl Row {
    fn new(summary: Summary, now: i64) -> Self {
        let tool = summary.kind.tool();
        let detail = match summary.trashed_at {
            Some(trashed) => {
                let left = TRASH_DAYS - (now - trashed) / DAY_MS;
                let days = left.max(1);
                format!(
                    "{} · trashed {} · deleted in {days} day{}",
                    tool.name,
                    relative_time(trashed, now).to_lowercase(),
                    if days == 1 { "" } else { "s" }
                )
            }
            None if summary.info.is_empty() => {
                format!("Edited {}", relative_time(summary.updated_at, now).to_lowercase())
            }
            None => format!(
                "Edited {} · {}",
                relative_time(summary.updated_at, now).to_lowercase(),
                summary.info
            ),
        };
        let href = format!("{}/{}", tool.href, summary.id);
        Self {
            view: format!("{href}/view"),
            exports: tool
                .exports
                .iter()
                .map(|(label, file)| (*label, format!("{href}/{file}")))
                .collect(),
            href,
            id: summary.id,
            title: summary.title,
            excerpt: summary.excerpt,
            detail,
            icon: tool.icon,
            empty: tool.empty,
        }
    }
}

/// The owner's files, newest first, optionally of one kind and matching `query`.
pub fn list(owner: &str, kind: Option<Kind>, query: &str, limit: i64) -> Result<Vec<Row>> {
    let mut sql = format!("{SUMMARY} WHERE owner=? AND trashed_at IS NULL");
    let mut values: Vec<Value> = vec![owner.into()];
    if let Some(kind) = kind {
        sql.push_str(" AND kind=?");
        values.push(kind.as_str().into());
    }
    if !query.is_empty() {
        sql.push_str(" AND (title LIKE ? ESCAPE '\\' OR plain LIKE ? ESCAPE '\\')");
        let pattern = format!("%{}%", like_escape(query));
        values.push(pattern.clone().into());
        values.push(pattern.into());
    }
    sql.push_str(" ORDER BY updated_at DESC LIMIT ?");
    values.push(limit.into());
    let now = now_ms();
    let rows: Vec<Summary> = Database.fetch_all(&sql, values)?;
    Ok(rows.into_iter().map(|r| Row::new(r, now)).collect())
}

fn purge_expired(owner: &str, now: i64) -> Result<()> {
    Database.execute(
        "DELETE FROM files WHERE owner=? AND trashed_at IS NOT NULL AND trashed_at<?",
        params![owner, now - TRASH_DAYS * DAY_MS],
    )?;
    Ok(())
}

/// Creates an empty file and returns its id.
pub fn create(owner: &str, kind: Kind, body: &str, info: &str) -> Result<String> {
    let id = loki_sdk::random_id();
    let now = now_ms();
    Database.execute(
        "INSERT INTO files(id,owner,kind,title,info,version,created_at,updated_at,body) VALUES(?,?,?,?,?,1,?,?,?)",
        params![id, owner, kind.as_str(), format!("Untitled {}", kind.tool().noun), info, now, now, body],
    )?;
    Ok(id)
}

#[derive(Deserialize)]
pub struct Stored {
    pub id: String,
    pub title: String,
    pub version: i64,
    pub info: String,
    pub updated_at: i64,
    pub trashed_at: Option<i64>,
    body: String,
}
impl Stored {
    pub fn load(owner: &str, id: &str, kind: Kind) -> Result<Self> {
        Database
            .fetch_optional(
                "SELECT id,title,version,info,updated_at,trashed_at,body FROM files WHERE id=? AND owner=? AND kind=?",
                params![id, owner, kind.as_str()],
            )?
            .ok_or_else(not_found)
    }
    pub fn body<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_str(&self.body)
            .map_err(|e| Error::internal(format!("Stored file {} is unreadable: {e}", self.id)))
    }
    /// "Edited 5 min ago · 120 words", for page headers.
    pub fn detail(&self) -> String {
        let edited = format!("Edited {}", relative_time(self.updated_at, now_ms()).to_lowercase());
        if self.info.is_empty() { edited } else { format!("{edited} · {}", self.info) }
    }
}

/// What a tool hands over when saving: its validated content and its summary.
pub struct Content {
    pub body: String,
    /// The opening text, on one line, for lists.
    pub excerpt: String,
    /// A short fact for lists, such as "120 words".
    pub info: String,
    /// All searchable text.
    pub plain: String,
}

/// Saves when `version` is still current, else answers 409 with the current
/// version so the editor can offer to reload or overwrite.
pub fn save(owner: &str, id: &str, kind: Kind, version: i64, title: &str, content: Content, now: i64) -> Result<Response> {
    let title = clean_title(title, kind)?;
    if content.body.len() > MAX_STORED_BYTES {
        return Err(Error::new(
            413,
            format!("This {} is too large. Split it into several files.", kind.tool().noun),
        ));
    }
    let affected = Database.execute(
        "UPDATE files SET title=?,excerpt=?,info=?,plain=?,body=?,version=version+1,updated_at=?
         WHERE id=? AND owner=? AND kind=? AND version=? AND trashed_at IS NULL",
        params![
            title,
            content.excerpt,
            content.info,
            content.plain,
            content.body,
            now,
            id,
            owner,
            kind.as_str(),
            version
        ],
    )?;
    if affected == 1 {
        #[derive(Serialize)]
        struct Saved {
            version: i64,
            updated_at: i64,
            title: String,
        }
        return Response::json(&Saved {
            version: version + 1,
            updated_at: now,
            title,
        });
    }
    #[derive(Deserialize)]
    struct Current {
        version: i64,
        trashed_at: Option<i64>,
    }
    let current: Current = Database
        .fetch_optional(
            "SELECT version,trashed_at FROM files WHERE id=? AND owner=? AND kind=?",
            params![id, owner, kind.as_str()],
        )?
        .ok_or_else(not_found)?;
    #[derive(Serialize)]
    struct Conflict {
        error: &'static str,
        version: i64,
        trashed: bool,
    }
    let trashed = current.trashed_at.is_some();
    Ok(Response::json(&Conflict {
        error: if trashed {
            "This file was moved to the trash."
        } else {
            "This file was changed somewhere else."
        },
        version: current.version,
        trashed,
    })?
    .with_status(409))
}

// ------------------------------------------------------------------ pages

#[derive(Template)]
#[template(path = "files.html")]
struct ListPage {
    shell: Shell,
    tool: &'static Tool,
    query: String,
    rows: Vec<Row>,
}

/// A tool's home: its files, with search, actions and a "New" button.
pub fn list_page(request: &Request, kind: Kind) -> Result<Response> {
    let owner = owner(request)?;
    purge_expired(&owner, now_ms())?;
    let query = request.query("q").unwrap_or_default().trim().to_string();
    page(Response::render(ListPage {
        shell: Shell::new(request, Some(kind.tool()))?,
        tool: kind.tool(),
        rows: list(&owner, Some(kind), &query, LIST_LIMIT)?,
        query,
    })?)
}

#[derive(Template)]
#[template(path = "trash.html")]
struct TrashPage {
    shell: Shell,
    rows: Vec<Row>,
    days: i64,
}

pub fn trash(request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let now = now_ms();
    purge_expired(&owner, now)?;
    let rows: Vec<Summary> = Database.fetch_all(
        &format!("{SUMMARY} WHERE owner=? AND trashed_at IS NOT NULL ORDER BY trashed_at DESC LIMIT ?"),
        params![owner, LIST_LIMIT],
    )?;
    page(Response::render(TrashPage {
        shell: Shell::new(&request, None)?,
        rows: rows.into_iter().map(|r| Row::new(r, now)).collect(),
        days: TRASH_DAYS,
    })?)
}

fn kind_of(owner: &str, id: &str) -> Result<Kind> {
    #[derive(Deserialize)]
    struct Found {
        kind: Kind,
    }
    let found: Found = Database
        .fetch_optional("SELECT kind FROM files WHERE id=? AND owner=?", params![id, owner])?
        .ok_or_else(not_found)?;
    Ok(found.kind)
}

pub fn rename(request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let id = request.param("id")?;
    let kind = kind_of(&owner, id)?;
    let form = request.form()?;
    let title = clean_title(form.required("title")?, kind)?;
    Database.execute(
        "UPDATE files SET title=?,version=version+1,updated_at=? WHERE id=? AND owner=?",
        params![title, now_ms(), id, owner],
    )?;
    Response::redirect(kind.tool().href)
}

pub fn duplicate(request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let source = request.param("id")?;
    let kind = kind_of(&owner, source)?;
    let id = loki_sdk::random_id();
    let now = now_ms();
    Database.execute(
        "INSERT INTO files(id,owner,kind,title,excerpt,info,version,created_at,updated_at,plain,body)
         SELECT ?,owner,kind,substr('Copy of ' || title,1,?),excerpt,info,1,?,?,plain,body
         FROM files WHERE id=? AND owner=?",
        params![id, MAX_TITLE_CHARS, now, now, source, owner],
    )?;
    Response::redirect(&format!("{}/{id}", kind.tool().href))
}

pub fn move_to_trash(request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let id = request.param("id")?;
    let kind = kind_of(&owner, id)?;
    Database.execute(
        "UPDATE files SET trashed_at=? WHERE id=? AND owner=? AND trashed_at IS NULL",
        params![now_ms(), id, owner],
    )?;
    Response::redirect(kind.tool().href)
}

pub fn restore(request: Request) -> Result<Response> {
    in_trash(
        &request,
        "UPDATE files SET trashed_at=NULL WHERE id=? AND owner=? AND trashed_at IS NOT NULL",
    )
}

pub fn purge(request: Request) -> Result<Response> {
    in_trash(
        &request,
        "DELETE FROM files WHERE id=? AND owner=? AND trashed_at IS NOT NULL",
    )
}

fn in_trash(request: &Request, sql: &str) -> Result<Response> {
    let owner = owner(request)?;
    if Database.execute(sql, params![request.param("id")?, owner])? == 0 {
        return Err(not_found());
    }
    Response::redirect("/trash")
}

pub fn empty_trash(request: Request) -> Result<Response> {
    Database.execute(
        "DELETE FROM files WHERE owner=? AND trashed_at IS NOT NULL",
        params![owner(&request)?],
    )?;
    Response::redirect("/trash")
}

fn like_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `attachment` disposition with an ASCII fallback name and the full UTF-8 title.
pub fn attachment(response: Response, title: &str, extension: &str) -> Result<Response> {
    let ascii: String = title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || " -_.,()".contains(c) { c } else { '_' })
        .take(80)
        .collect();
    let ascii = match ascii.trim() {
        "" => "file",
        name => name,
    };
    let mut encoded = String::new();
    for byte in title.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    response.header(
        "content-disposition",
        &format!("attachment; filename=\"{ascii}.{extension}\"; filename*=UTF-8''{encoded}.{extension}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_host::{context, host};
    use loki_sdk::{Method, testing};
    use serde_json::json;

    fn content() -> Content {
        Content {
            body: "{}".into(),
            excerpt: "Hello".into(),
            info: "1 word".into(),
            plain: "Hello".into(),
        }
    }

    #[test]
    fn save_is_scoped_to_owner_kind_and_version() {
        let (log, fake) = host(|_| json!({"rows": [], "affected": 1}));
        let response = testing::with_host(fake, || {
            save("user-1", "f-1", Kind::Doc, 4, " Report ", content(), 99).unwrap()
        });
        assert_eq!(response.status, 200);
        let body: Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body, json!({"version": 5, "updated_at": 99, "title": "Report"}));
        let log = log.borrow();
        assert_eq!(log.len(), 1);
        assert!(log[0]["sql"].as_str().unwrap().contains("WHERE id=? AND owner=? AND kind=? AND version=?"));
        let params = log[0]["params"].as_array().unwrap();
        assert_eq!(params[0], "Report");
        assert_eq!(params[6..], [json!("f-1"), json!("user-1"), json!("doc"), json!(4)]);
    }

    #[test]
    fn stale_save_is_a_conflict() {
        let (_, fake) = host(|sql| {
            if sql.starts_with("UPDATE") {
                json!({"rows": [], "affected": 0})
            } else {
                json!({"rows": [{"version": 7, "trashed_at": null}], "affected": 0})
            }
        });
        let response = testing::with_host(fake, || save("user-1", "f-1", Kind::Sheet, 4, "x", content(), 99).unwrap());
        assert_eq!(response.status, 409);
        let body: Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!((body["version"].clone(), body["trashed"].clone()), (json!(7), json!(false)));
    }

    #[test]
    fn someone_elses_file_is_not_found() {
        let (_, fake) = host(|_| json!({"rows": [], "affected": 0}));
        let error = testing::with_host(fake, || save("user-2", "f-1", Kind::Doc, 1, "x", content(), 1).unwrap_err());
        assert_eq!(error.status(), 404);
        for path in ["/files/f-1/rename", "/files/f-1/trash", "/files/f-1/duplicate", "/files/f-1/restore"] {
            let (log, fake) = host(|_| json!({"rows": [], "affected": 0}));
            let request = testing::request(Method::Post, path, context(true));
            let error = testing::with_host(fake, || crate::router().handle(request).unwrap_err());
            assert_eq!(error.status(), 404, "{path}");
            assert_eq!(log.borrow()[0]["params"].as_array().unwrap()[..2], [json!("f-1"), json!("user-1")]);
        }
    }

    #[test]
    fn rename_and_trash_are_owner_scoped() {
        let (log, fake) = host(|sql| {
            if sql.starts_with("SELECT kind") {
                json!({"rows": [{"kind": "sheet"}], "affected": 0})
            } else {
                json!({"rows": [], "affected": 1})
            }
        });
        let rename = testing::request(Method::Post, "/files/f-1/rename", context(true));
        let trash = testing::request(Method::Post, "/files/f-1/trash", context(true));
        testing::with_host(fake, || {
            let renamed = crate::router().handle(rename).unwrap();
            assert_eq!(renamed.headers["location"], "/sheets");
            assert_eq!(crate::router().handle(trash).unwrap().status, 303);
        });
        let log = log.borrow();
        assert_eq!(log[1]["params"][0], "New name");
        assert_eq!(log[1]["params"].as_array().unwrap()[2..], [json!("f-1"), json!("user-1")]);
        assert_eq!(log[3]["params"].as_array().unwrap()[1..], [json!("f-1"), json!("user-1")]);
    }

    #[test]
    fn search_filters_by_kind_and_escapes_patterns() {
        let (log, fake) = host(|_| json!({"rows": [], "affected": 0}));
        testing::with_host(fake, || list("user-1", Some(Kind::Sheet), "50%", 10).unwrap());
        let log = log.borrow();
        assert_eq!(log[0]["params"], json!(["user-1", "sheet", r"%50\%%", r"%50\%%", 10]));
        assert_eq!(like_escape(r"50%_a\b"), r"50\%\_a\\b");
    }

    #[test]
    fn cleans_titles() {
        assert_eq!(clean_title("  Quarterly \n report ", Kind::Doc).unwrap(), "Quarterly report");
        assert_eq!(clean_title("   ", Kind::Doc).unwrap(), "Untitled document");
        assert_eq!(clean_title("", Kind::Sheet).unwrap(), "Untitled spreadsheet");
        assert!(clean_title(&"x".repeat(201), Kind::Doc).is_err());
    }

    #[test]
    fn pages_need_a_signed_in_user() {
        for path in ["/", "/trash", "/docs", "/docs/f-1", "/docs/f-1/export.md", "/sheets", "/sheets/f-1", "/sheets/f-1/export.csv"] {
            let (_, fake) = host(|_| json!({"rows": [], "affected": 0}));
            let request = testing::request(Method::Get, path, context(false));
            let error = testing::with_host(fake, || crate::router().handle(request).unwrap_err());
            assert_eq!(error.status(), 401, "{path}");
        }
    }
}
