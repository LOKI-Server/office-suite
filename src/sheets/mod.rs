//! Sheets, the suite's spreadsheet: formulas evaluated in the browser,
//! formatting, CSV import/export and a read-only view. Everything lives
//! under `/sheets`.
mod handlers;
pub mod model;
pub mod render;

use crate::ui::Tool;
use handlers::*;
use loki_sdk::Router;

pub static SHEETS: Tool = Tool {
    name: "Sheets",
    href: "/sheets",
    noun: "spreadsheet",
    icon: "sheet",
    empty: "Empty spreadsheet",
    exports: &[("CSV", "export.csv")],
};

pub fn routes(router: Router) -> Router {
    router
        .get("/sheets", home)
        .post("/sheets/new", create)
        .get("/sheets/{id}", editor)
        .get("/sheets/{id}/view", view)
        .get("/sheets/{id}/export.csv", export_csv)
        .post("/api/sheets/{id}", save)
}
