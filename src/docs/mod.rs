//! Docs, the suite's word processor: rich-text documents with autosave,
//! a read-only view and Markdown/HTML export. Everything lives under `/docs`.
mod handlers;
pub mod model;
pub mod render;

use crate::ui::Tool;
use handlers::*;
use loki_sdk::Router;

pub static DOCS: Tool = Tool {
    name: "Docs",
    href: "/docs",
    noun: "document",
    icon: "doc",
    empty: "Empty document",
    exports: &[("Markdown", "export.md"), ("HTML", "export.html")],
};

pub fn routes(router: Router) -> Router {
    // Fixed paths come before `/docs/{id}`, which would match them too.
    router
        .get("/docs", home)
        .post("/docs/new", create)
        .get("/docs/{id}", editor)
        .get("/docs/{id}/view", view)
        .get("/docs/{id}/export.md", export_markdown)
        .get("/docs/{id}/export.html", export_html)
        .post("/api/docs/{id}", save)
}
