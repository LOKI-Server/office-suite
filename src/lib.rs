//! Office Suite: one LOKI app holding the suite's tools. Each tool is a module
//! with its own routes under its own path; their files share one table
//! (`files`), and `ui` holds the shared page chrome.
mod cleanup;
mod docs;
mod files;
mod schema;
mod sharing;
mod sheets;
#[cfg(test)]
mod test_host;
mod ui;

use askama::Template;
use loki_sdk::{Request, Response, Result, Router};

/// Every tool in the suite, in the order the home page shows them.
static TOOLS: [&ui::Tool; 2] = [&docs::DOCS, &sheets::SHEETS];
const RECENT_LIMIT: i64 = 12;
const SEARCH_LIMIT: i64 = 100;
/// Files served from `static/`.
const ASSETS: &[&str] = &[
    "office.css",
    "app.js",
    "autosave.js",
    "docs.css",
    "editor.js",
    "sheets.css",
    "formula.js",
    "sheet.js",
];

pub fn router() -> Router {
    let router = Router::new()
        .get("/health", health)
        .schedule(cleanup::NAME, cleanup::run)
        .get("/", home)
        .get("/trash", files::trash)
        .post("/trash/empty", files::empty_trash)
        .post("/files/{id}/rename", files::rename)
        .post("/files/{id}/duplicate", files::duplicate)
        .post("/files/{id}/trash", files::move_to_trash)
        .post("/files/{id}/restore", files::restore)
        .post("/files/{id}/purge", files::purge)
        .get("/files/{id}/versions", files::versions)
        .get("/files/{id}/versions/{version}", files::show_version)
        .post("/files/{id}/versions/{version}/restore", files::restore_version)
        .get("/files/{id}/share", sharing::settings)
        .post("/files/{id}/share", sharing::update)
        .get("/shared/{token}", sharing::open_link)
        .post("/shared/{id}/forget", sharing::forget)
        .get("/static/{file}", asset);
    sheets::routes(docs::routes(router))
}

fn health(_: Request) -> Result<Response> {
    schema::migrate()?;
    Ok(Response::text("ready"))
}

fn asset(request: Request) -> Result<Response> {
    let file = request.param("file")?;
    if !ASSETS.contains(&file) {
        return Ok(Response::text("Not found.").with_status(404));
    }
    loki_sdk::asset(&format!("static/{file}"))
}

#[derive(Template)]
#[template(path = "home.html")]
struct HomePage {
    shell: ui::Shell,
    tools: &'static [&'static ui::Tool],
    query: String,
    rows: Vec<files::Row>,
    /// Files others shared with this person, when not searching.
    shared: Vec<files::Row>,
}

/// The suite's home: a "New" button per tool, then recent files or search results.
fn home(request: Request) -> Result<Response> {
    let owner = files::owner(&request)?;
    let query = request.query("q").unwrap_or_default().trim().to_string();
    let limit = if query.is_empty() { RECENT_LIMIT } else { SEARCH_LIMIT };
    ui::page(Response::render(HomePage {
        shell: ui::Shell::new(&request, None)?,
        tools: &TOOLS,
        rows: files::list(&owner, None, &query, limit)?,
        shared: if query.is_empty() { files::shared_with(&owner, RECENT_LIMIT)? } else { Vec::new() },
        query,
    })?)
}

fn handle(request: Request) -> Result<Response> {
    router().handle(request)
}
loki_sdk::export!(handle);
