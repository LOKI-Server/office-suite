//! Docs, the suite's word processor: private rich-text documents with
//! autosave, a trash, and Markdown/HTML export. Everything lives under `/docs`.
mod handlers;
pub mod model;
pub mod render;

use crate::ui::Tool;
use handlers::*;
use loki_sdk::Router;

pub use handlers::{Row, recent};

pub static DOCS: Tool = Tool {
    name: "Docs",
    href: "/docs",
};

pub fn routes(router: Router) -> Router {
    // Fixed paths come before `/docs/{id}`, which would match them too.
    router
        .get("/docs", home)
        .post("/docs/new", create)
        .get("/docs/trash", trash)
        .post("/docs/trash/empty", empty_trash)
        .get("/docs/{id}", editor)
        .get("/docs/{id}/view", view)
        .get("/docs/{id}/export.md", export_markdown)
        .get("/docs/{id}/export.html", export_html)
        .post("/docs/{id}/duplicate", duplicate)
        .post("/docs/{id}/rename", rename)
        .post("/docs/{id}/trash", move_to_trash)
        .post("/docs/{id}/restore", restore)
        .post("/docs/{id}/purge", purge)
        .post("/api/docs/{id}", save)
}
