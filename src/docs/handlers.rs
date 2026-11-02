//! Route handlers. Every query is scoped to the signed-in owner, so another
//! person's document id behaves exactly like one that does not exist.
use super::model::{self, Document};
use super::render;
use askama::Template;
use loki_sdk::{Database, Error, Request, Response, Result, params};
use crate::ui::{Shell, now_ms, page, relative_time, script_json};
use serde::{Deserialize, Serialize};

const EMPTY_BODY: &str = r#"{"blocks":[]}"#;
/// Trashed documents are deleted for good after this many days.
const TRASH_DAYS: i64 = 30;
const DAY_MS: i64 = 86_400_000;
const LIST_LIMIT: i64 = 500;
const EXCERPT_CHARS: usize = 160;
/// An autosave request; generous because JSON adds overhead to the text limit.
const MAX_SAVE_BYTES: usize = 2 * 1024 * 1024;
/// LOKI stores at most 1 MiB per database value.
const MAX_STORED_BYTES: usize = 900 * 1024;

fn shell(request: &Request) -> Result<Shell> {
    Shell::new(request, Some(&super::DOCS))
}
fn not_found() -> Error {
    Error::not_found("This document does not exist, or it isn't yours.")
}
fn owner(request: &Request) -> Result<String> {
    Ok(request.context().require_user()?.id.clone())
}

#[derive(Deserialize)]
struct Summary {
    id: String,
    title: String,
    excerpt: String,
    words: i64,
    updated_at: i64,
    trashed_at: Option<i64>,
}
/// A document in a list, ready to display.
pub struct Row {
    pub id: String,
    pub title: String,
    pub excerpt: String,
    pub detail: String,
}
impl Row {
    fn new(summary: Summary, now: i64) -> Self {
        let detail = match summary.trashed_at {
            Some(trashed) => {
                let left = TRASH_DAYS - (now - trashed) / DAY_MS;
                format!(
                    "Trashed {} · deleted in {}",
                    relative_time(trashed, now).to_lowercase(),
                    plural(left.max(1), "day")
                )
            }
            None => format!(
                "Edited {} · {}",
                relative_time(summary.updated_at, now).to_lowercase(),
                plural(summary.words, "word")
            ),
        };
        Self {
            id: summary.id,
            title: summary.title,
            excerpt: summary.excerpt,
            detail,
        }
    }
}
fn plural(count: i64, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

#[derive(Deserialize)]
struct Stored {
    id: String,
    title: String,
    body: String,
    version: i64,
    words: i64,
    updated_at: i64,
    trashed_at: Option<i64>,
}
impl Stored {
    fn load(owner: &str, id: &str) -> Result<Self> {
        Database
            .fetch_optional(
                "SELECT id,title,body,version,words,updated_at,trashed_at FROM documents WHERE id=? AND owner=?",
                params![id, owner],
            )?
            .ok_or_else(not_found)
    }
    fn document(&self) -> Result<Document> {
        serde_json::from_str(&self.body)
            .map_err(|e| Error::internal(format!("Stored document {} is unreadable: {e}", self.id)))
    }
}

#[derive(Template)]
#[template(path = "docs/list.html")]
struct ListPage {
    shell: Shell,
    query: String,
    docs: Vec<Row>,
}
#[derive(Template)]
#[template(path = "docs/trash.html")]
struct TrashPage {
    shell: Shell,
    docs: Vec<Row>,
    days: i64,
}
#[derive(Template)]
#[template(path = "docs/editor.html")]
struct EditorPage {
    shell: Shell,
    id: String,
    title: String,
    data: String,
}
#[derive(Template)]
#[template(path = "docs/view.html")]
struct ViewPage {
    shell: Shell,
    id: String,
    title: String,
    body: String,
    detail: String,
}
/// The owner's most recently edited documents, for the suite's home page.
pub fn recent(owner: &str, limit: i64) -> Result<Vec<Row>> {
    let now = now_ms();
    let docs: Vec<Summary> = Database.fetch_all(
        "SELECT id,title,excerpt,words,updated_at,trashed_at FROM documents
         WHERE owner=? AND trashed_at IS NULL ORDER BY updated_at DESC LIMIT ?",
        params![owner, limit],
    )?;
    Ok(docs.into_iter().map(|d| Row::new(d, now)).collect())
}

pub fn home(request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let now = now_ms();
    purge_expired(&owner, now)?;
    let query = request.query("q").unwrap_or_default().trim().to_string();
    let docs: Vec<Summary> = if query.is_empty() {
        Database.fetch_all(
            "SELECT id,title,excerpt,words,updated_at,trashed_at FROM documents
             WHERE owner=? AND trashed_at IS NULL ORDER BY updated_at DESC LIMIT ?",
            params![owner, LIST_LIMIT],
        )?
    } else {
        let pattern = format!("%{}%", like_escape(&query));
        Database.fetch_all(
            "SELECT id,title,excerpt,words,updated_at,trashed_at FROM documents
             WHERE owner=? AND trashed_at IS NULL
               AND (title LIKE ? ESCAPE '\\' OR plain LIKE ? ESCAPE '\\')
             ORDER BY updated_at DESC LIMIT ?",
            params![owner, pattern, pattern, LIST_LIMIT],
        )?
    };
    page(Response::render(ListPage {
        shell: shell(&request)?,
        query,
        docs: docs.into_iter().map(|d| Row::new(d, now)).collect(),
    })?)
}

pub fn trash(request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let now = now_ms();
    purge_expired(&owner, now)?;
    let docs: Vec<Summary> = Database.fetch_all(
        "SELECT id,title,excerpt,words,updated_at,trashed_at FROM documents
         WHERE owner=? AND trashed_at IS NOT NULL ORDER BY trashed_at DESC LIMIT ?",
        params![owner, LIST_LIMIT],
    )?;
    page(Response::render(TrashPage {
        shell: shell(&request)?,
        docs: docs.into_iter().map(|d| Row::new(d, now)).collect(),
        days: TRASH_DAYS,
    })?)
}

pub fn create(request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let id = loki_sdk::random_id();
    let now = now_ms();
    Database.execute(
        "INSERT INTO documents(id,owner,title,body,plain,version,created_at,updated_at) VALUES(?,?,?,?,'',1,?,?)",
        params![id, owner, model::UNTITLED, EMPTY_BODY, now, now],
    )?;
    Response::redirect(&format!("/docs/{id}"))
}

pub fn editor(request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let stored = Stored::load(&owner, request.param("id")?)?;
    if stored.trashed_at.is_some() {
        return Response::redirect("/docs/trash");
    }
    #[derive(Serialize)]
    struct Data<'a> {
        id: &'a str,
        title: &'a str,
        version: i64,
        csrf: &'a str,
        body: Document,
    }
    let data = script_json(&Data {
        id: &stored.id,
        title: &stored.title,
        version: stored.version,
        csrf: request.context().csrf_token(),
        body: stored.document()?,
    })?;
    page(Response::render(EditorPage {
        shell: shell(&request)?,
        id: stored.id,
        title: stored.title,
        data,
    })?)
}

#[derive(Deserialize)]
pub struct SaveRequest {
    pub title: String,
    pub version: i64,
    pub body: Document,
}

/// Autosave: `POST /api/docs/{id}` with a JSON [`SaveRequest`] and `X-CSRF-Token`.
pub fn save(mut request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let id = request.param("id")?.to_string();
    let payload: SaveRequest = request.json(MAX_SAVE_BYTES)?;
    save_document(&owner, &id, payload, now_ms())
}

/// Saves when `payload.version` is still current, else answers 409 with the
/// current version so the editor can offer to reload or overwrite.
pub fn save_document(owner: &str, id: &str, payload: SaveRequest, now: i64) -> Result<Response> {
    let title = model::clean_title(&payload.title)?;
    let mut body = payload.body;
    body.validate()?;
    body.normalize();
    let json = serde_json::to_string(&body)?;
    if json.len() > MAX_STORED_BYTES {
        return Err(Error::new(413, "This document is too long. Split it into several documents."));
    }
    let affected = Database.execute(
        "UPDATE documents SET title=?,body=?,excerpt=?,words=?,plain=?,version=version+1,updated_at=?
         WHERE id=? AND owner=? AND version=? AND trashed_at IS NULL",
        params![
            title,
            json,
            body.excerpt(EXCERPT_CHARS),
            body.word_count(),
            body.plain_text(),
            now,
            id,
            owner,
            payload.version
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
            version: payload.version + 1,
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
            "SELECT version,trashed_at FROM documents WHERE id=? AND owner=?",
            params![id, owner],
        )?
        .ok_or_else(not_found)?;
    #[derive(Serialize)]
    struct Conflict {
        error: &'static str,
        version: i64,
        trashed: bool,
    }
    Ok(Response::json(&Conflict {
        error: if current.trashed_at.is_some() {
            "This document was moved to the trash."
        } else {
            "This document was changed somewhere else."
        },
        version: current.version,
        trashed: current.trashed_at.is_some(),
    })?
    .with_status(409))
}

pub fn view(request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let stored = Stored::load(&owner, request.param("id")?)?;
    let now = now_ms();
    page(Response::render(ViewPage {
        shell: shell(&request)?,
        body: render::to_html(&stored.document()?),
        detail: format!(
            "Edited {} · {}",
            relative_time(stored.updated_at, now).to_lowercase(),
            plural(stored.words, "word")
        ),
        id: stored.id,
        title: stored.title,
    })?)
}

pub fn export_markdown(request: Request) -> Result<Response> {
    let stored = Stored::load(&owner(&request)?, request.param("id")?)?;
    let markdown = render::to_markdown(&stored.title, &stored.document()?);
    attachment(
        Response::text(markdown).header("content-type", "text/markdown; charset=utf-8")?,
        &stored.title,
        "md",
    )
}

pub fn export_html(request: Request) -> Result<Response> {
    let stored = Stored::load(&owner(&request)?, request.param("id")?)?;
    let html = render::to_html_file(&stored.title, &stored.document()?);
    attachment(
        Response::html(html).header("content-security-policy", "sandbox; default-src 'none'; style-src 'unsafe-inline'")?,
        &stored.title,
        "html",
    )
}

pub fn duplicate(request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let stored = Stored::load(&owner, request.param("id")?)?;
    let title: String = format!("Copy of {}", stored.title)
        .chars()
        .take(model::MAX_TITLE_CHARS)
        .collect();
    let id = loki_sdk::random_id();
    let now = now_ms();
    Database.execute(
        "INSERT INTO documents(id,owner,title,body,excerpt,words,plain,version,created_at,updated_at)
         SELECT ?,owner,?,body,excerpt,words,plain,1,?,? FROM documents WHERE id=? AND owner=?",
        params![id, title, now, now, stored.id, owner],
    )?;
    Response::redirect(&format!("/docs/{id}"))
}

pub fn rename(request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let form = request.form()?;
    let title = model::clean_title(form.required("title")?)?;
    let affected = Database.execute(
        "UPDATE documents SET title=?,version=version+1,updated_at=? WHERE id=? AND owner=?",
        params![title, now_ms(), request.param("id")?, owner],
    )?;
    if affected == 0 {
        return Err(not_found());
    }
    Response::redirect("/docs")
}

pub fn move_to_trash(request: Request) -> Result<Response> {
    change(
        &request,
        "UPDATE documents SET trashed_at=? WHERE id=? AND owner=? AND trashed_at IS NULL",
        true,
        "/docs",
    )
}

pub fn restore(request: Request) -> Result<Response> {
    change(
        &request,
        "UPDATE documents SET trashed_at=NULL WHERE id=? AND owner=? AND trashed_at IS NOT NULL",
        false,
        "/docs/trash",
    )
}

pub fn purge(request: Request) -> Result<Response> {
    change(
        &request,
        "DELETE FROM documents WHERE id=? AND owner=? AND trashed_at IS NOT NULL",
        false,
        "/docs/trash",
    )
}

pub fn empty_trash(request: Request) -> Result<Response> {
    Database.execute(
        "DELETE FROM documents WHERE owner=? AND trashed_at IS NOT NULL",
        params![owner(&request)?],
    )?;
    Response::redirect("/docs/trash")
}

/// Runs one statement on the routed document, with the current time first when
/// `stamped`, and redirects to `next`.
fn change(request: &Request, sql: &str, stamped: bool, next: &str) -> Result<Response> {
    let owner = owner(request)?;
    let id = request.param("id")?;
    let affected = if stamped {
        Database.execute(sql, params![now_ms(), id, owner])?
    } else {
        Database.execute(sql, params![id, owner])?
    };
    if affected == 0 {
        return Err(not_found());
    }
    Response::redirect(next)
}

fn purge_expired(owner: &str, now: i64) -> Result<()> {
    Database.execute(
        "DELETE FROM documents WHERE owner=? AND trashed_at IS NOT NULL AND trashed_at<?",
        params![owner, now - TRASH_DAYS * DAY_MS],
    )?;
    Ok(())
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
fn attachment(response: Response, title: &str, extension: &str) -> Result<Response> {
    let ascii: String = title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || " -_.,()".contains(c) { c } else { '_' })
        .take(80)
        .collect();
    let ascii = match ascii.trim() {
        "" => "document",
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
    use loki_sdk::{Context, Method, testing};
    use serde_json::{Value, json};
    use std::{cell::RefCell, rc::Rc};

    type Log = Rc<RefCell<Vec<Value>>>;

    fn context(signed_in: bool) -> Context {
        serde_json::from_value(json!({
            "app_id": "office",
            "origin": "http://office.localhost",
            "issuer": "http://localhost",
            "csrf": "token",
            "user": signed_in.then(|| json!({
                "id": "user-1", "username": "ada", "name": "Ada", "email": "", "roles": ["access"]
            })),
        }))
        .unwrap()
    }

    /// A fake host whose database answers each statement with `answer(sql)`
    /// and records every statement it receives.
    fn host(answer: impl Fn(&str) -> Value + 'static) -> (Log, impl FnMut(&str, &str) -> std::result::Result<String, String>) {
        let log: Log = Rc::default();
        let seen = log.clone();
        let fake = move |operation: &str, input: &str| {
            let input: Value = serde_json::from_str(input).unwrap();
            match operation {
                "database" => {
                    let results: Vec<Value> = input["statements"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|statement| {
                            seen.borrow_mut().push(statement.clone());
                            answer(statement["sql"].as_str().unwrap())
                        })
                        .collect();
                    Ok(Value::Array(results).to_string())
                }
                "form" => Ok(json!({"fields": {"title": "  New name "}, "files": []}).to_string()),
                "log" => Ok("null".into()),
                other => Err(format!("unexpected host call {other}")),
            }
        };
        (log, fake)
    }

    fn payload(version: i64) -> SaveRequest {
        serde_json::from_value(json!({
            "title": " Report ",
            "version": version,
            "body": {"blocks": [{"type": "paragraph", "runs": [{"text": "Hello"}, {"text": " world"}]}]}
        }))
        .unwrap()
    }

    #[test]
    fn save_is_scoped_to_owner_and_version() {
        let (log, fake) = host(|_| json!({"rows": [], "affected": 1}));
        let response = testing::with_host(fake, || save_document("user-1", "doc-1", payload(4), 99).unwrap());
        assert_eq!(response.status, 200);
        let body: Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body, json!({"version": 5, "updated_at": 99, "title": "Report"}));
        let log = log.borrow();
        assert_eq!(log.len(), 1);
        assert!(log[0]["sql"].as_str().unwrap().contains("WHERE id=? AND owner=? AND version=?"));
        let params = log[0]["params"].as_array().unwrap();
        assert_eq!(params[0], "Report");
        assert_eq!(params[1], r#"{"blocks":[{"type":"paragraph","runs":[{"text":"Hello world"}]}]}"#);
        assert_eq!(params[3], 2);
        assert_eq!(params[6..], [json!("doc-1"), json!("user-1"), json!(4)]);
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
        let response = testing::with_host(fake, || save_document("user-1", "doc-1", payload(4), 99).unwrap());
        assert_eq!(response.status, 409);
        let body: Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["version"], 7);
        assert_eq!(body["trashed"], false);
    }

    #[test]
    fn someone_elses_document_is_not_found() {
        let (_, fake) = host(|_| json!({"rows": [], "affected": 0}));
        let error = testing::with_host(fake, || save_document("user-2", "doc-1", payload(1), 1).unwrap_err());
        assert_eq!(error.status(), 404);

        let (log, fake) = host(|_| json!({"rows": [], "affected": 0}));
        let request = testing::request(Method::Get, "/docs/doc-1", context(true));
        let error = testing::with_host(fake, || crate::router().handle(request).unwrap_err());
        assert_eq!(error.status(), 404);
        assert_eq!(log.borrow()[0]["params"], json!(["doc-1", "user-1"]));
    }

    #[test]
    fn invalid_documents_are_rejected_before_the_database() {
        let (log, fake) = host(|_| json!({"rows": [], "affected": 1}));
        let mut bad = payload(1);
        bad.body.blocks.push(serde_json::from_value(json!({"type": "heading", "level": 9})).unwrap());
        let error = testing::with_host(fake, || save_document("user-1", "doc-1", bad, 1).unwrap_err());
        assert_eq!(error.status(), 400);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn pages_need_a_signed_in_user() {
        for path in ["/", "/docs", "/docs/trash", "/docs/doc-1", "/docs/doc-1/export.md"] {
            let (_, fake) = host(|_| json!({"rows": [], "affected": 0}));
            let request = testing::request(Method::Get, path, context(false));
            let error = testing::with_host(fake, || crate::router().handle(request).unwrap_err());
            assert_eq!(error.status(), 401, "{path}");
        }
    }

    #[test]
    fn rename_and_trash_are_owner_scoped() {
        let (log, fake) = host(|_| json!({"rows": [], "affected": 1}));
        let rename = testing::request(Method::Post, "/docs/doc-1/rename", context(true));
        let trash = testing::request(Method::Post, "/docs/doc-1/trash", context(true));
        testing::with_host(fake, || {
            assert_eq!(crate::router().handle(rename).unwrap().status, 303);
            assert_eq!(crate::router().handle(trash).unwrap().status, 303);
        });
        let log = log.borrow();
        assert_eq!(log[0]["params"][0], "New name");
        assert_eq!(log[0]["params"].as_array().unwrap()[2..], [json!("doc-1"), json!("user-1")]);
        assert_eq!(log[1]["params"].as_array().unwrap()[1..], [json!("doc-1"), json!("user-1")]);
    }

    #[test]
    fn markdown_export_is_an_attachment() {
        let (_, fake) = host(|_| {
            json!({"rows": [{
                "id": "doc-1", "title": "Plan: Q3 ✓", "version": 1, "words": 1, "updated_at": 0, "trashed_at": null,
                "body": r#"{"blocks":[{"type":"paragraph","runs":[{"text":"Hi"}]}]}"#
            }], "affected": 0})
        });
        let request = testing::request(Method::Get, "/docs/doc-1/export.md", context(true));
        let response = testing::with_host(fake, || crate::router().handle(request).unwrap());
        assert_eq!(String::from_utf8(response.body).unwrap(), "# Plan: Q3 ✓\n\nHi\n");
        assert_eq!(
            response.headers["content-disposition"],
            "attachment; filename=\"Plan_ Q3 _.md\"; filename*=UTF-8''Plan%3A%20Q3%20%E2%9C%93.md"
        );
    }

    #[test]
    fn like_patterns_are_escaped() {
        assert_eq!(like_escape(r"50%_a\b"), r"50\%\_a\\b");
    }
}
