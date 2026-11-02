//! Office Suite: one LOKI app holding the suite's tools. Each tool is a module
//! with its own routes under its own path; `ui` holds the shared page chrome.
mod docs;
mod schema;
mod ui;

use askama::Template;
use loki_sdk::{Request, Response, Result, Router};

/// Every tool in the suite, in the order the home page shows them.
static TOOLS: [&ui::Tool; 1] = [&docs::DOCS];
const RECENT_LIMIT: i64 = 8;

pub fn router() -> Router {
    let router = Router::new()
        .get("/health", health)
        .get("/", home)
        .get("/static/office.css", |_| loki_sdk::asset("static/office.css"))
        .get("/static/docs.css", |_| loki_sdk::asset("static/docs.css"))
        .get("/static/editor.js", |_| loki_sdk::asset("static/editor.js"))
        .get("/static/app.js", |_| loki_sdk::asset("static/app.js"));
    docs::routes(router)
}

fn health(_: Request) -> Result<Response> {
    schema::migrate()?;
    Ok(Response::text("ready"))
}

#[derive(Template)]
#[template(path = "home.html")]
struct HomePage {
    shell: ui::Shell,
    tools: &'static [&'static ui::Tool],
    recent: Vec<docs::Row>,
}

fn home(request: Request) -> Result<Response> {
    let owner = request.context().require_user()?.id.clone();
    ui::page(Response::render(HomePage {
        shell: ui::Shell::new(&request, None)?,
        tools: &TOOLS,
        recent: docs::recent(&owner, RECENT_LIMIT)?,
    })?)
}

fn handle(request: Request) -> Result<Response> {
    router().handle(request)
}
loki_sdk::export!(handle);
