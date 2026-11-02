//! Docs pages and autosave. Listing, trash and file actions are shared in
//! [`crate::files`].
use super::model::Document;
use super::render;
use crate::files::{self, Content, Kind, Stored, owner};
use crate::ui::{Shell, now_ms, page, script_json};
use askama::Template;
use loki_sdk::{Request, Response, Result};
use serde::{Deserialize, Serialize};

const EMPTY_BODY: &str = r#"{"blocks":[]}"#;
const EXCERPT_CHARS: usize = 160;

fn shell(request: &Request) -> Result<Shell> {
    Shell::new(request, Some(&super::DOCS))
}
fn load(request: &Request) -> Result<Stored> {
    Stored::load(&owner(request)?, request.param("id")?, Kind::Doc)
}
fn words(count: usize) -> String {
    if count == 1 { "1 word".into() } else { format!("{count} words") }
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

pub fn home(request: Request) -> Result<Response> {
    files::list_page(&request, Kind::Doc)
}

pub fn create(request: Request) -> Result<Response> {
    let id = files::create(&owner(&request)?, Kind::Doc, EMPTY_BODY, &words(0))?;
    Response::redirect(&format!("/docs/{id}"))
}

pub fn editor(request: Request) -> Result<Response> {
    let stored = load(&request)?;
    if stored.trashed_at.is_some() {
        return Response::redirect("/trash");
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
        body: stored.body()?,
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
    let payload: SaveRequest = request.json(files::MAX_SAVE_BYTES)?;
    save_document(&owner, &id, payload, now_ms())
}

pub fn save_document(owner: &str, id: &str, payload: SaveRequest, now: i64) -> Result<Response> {
    let mut body = payload.body;
    body.validate()?;
    body.normalize();
    let content = Content {
        body: serde_json::to_string(&body)?,
        excerpt: body.excerpt(EXCERPT_CHARS),
        info: words(body.word_count()),
        plain: body.plain_text(),
    };
    files::save(owner, id, Kind::Doc, payload.version, &payload.title, content, now)
}

pub fn view(request: Request) -> Result<Response> {
    let stored = load(&request)?;
    page(Response::render(ViewPage {
        shell: shell(&request)?,
        body: render::to_html(&stored.body()?),
        detail: stored.detail(),
        id: stored.id,
        title: stored.title,
    })?)
}

pub fn export_markdown(request: Request) -> Result<Response> {
    let stored = load(&request)?;
    let markdown = render::to_markdown(&stored.title, &stored.body()?);
    files::attachment(
        Response::text(markdown).header("content-type", "text/markdown; charset=utf-8")?,
        &stored.title,
        "md",
    )
}

pub fn export_html(request: Request) -> Result<Response> {
    let stored = load(&request)?;
    let html = render::to_html_file(&stored.title, &stored.body()?);
    files::attachment(
        Response::html(html).header("content-security-policy", "sandbox; default-src 'none'; style-src 'unsafe-inline'")?,
        &stored.title,
        "html",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_host::{context, host};
    use loki_sdk::{Method, testing};
    use serde_json::json;

    fn payload(version: i64) -> SaveRequest {
        serde_json::from_value(json!({
            "title": " Report ",
            "version": version,
            "body": {"blocks": [{"type": "paragraph", "runs": [{"text": "Hello"}, {"text": " world"}]}]}
        }))
        .unwrap()
    }

    #[test]
    fn save_stores_the_normalized_model_and_summary() {
        let (log, fake) = host(|_| json!({"rows": [], "affected": 1}));
        let response = testing::with_host(fake, || save_document("user-1", "doc-1", payload(4), 99).unwrap());
        assert_eq!(response.status, 200);
        let params = log.borrow()[0]["params"].clone();
        assert_eq!(params[1], "Hello world");
        assert_eq!(params[2], "2 words");
        assert_eq!(params[4], r#"{"blocks":[{"type":"paragraph","runs":[{"text":"Hello world"}]}]}"#);
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
    fn documents_load_by_owner_and_kind() {
        let (log, fake) = host(|_| json!({"rows": [], "affected": 0}));
        let request = testing::request(Method::Get, "/docs/doc-1", context(true));
        let error = testing::with_host(fake, || crate::router().handle(request).unwrap_err());
        assert_eq!(error.status(), 404);
        assert_eq!(log.borrow()[0]["params"], json!(["doc-1", "user-1", "doc"]));
    }

    #[test]
    fn markdown_export_is_an_attachment() {
        let (_, fake) = host(|_| {
            json!({"rows": [{
                "id": "doc-1", "title": "Plan: Q3 ✓", "version": 1, "info": "1 word", "updated_at": 0, "trashed_at": null,
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
}
