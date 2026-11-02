//! Sheets pages and autosave. Listing, trash, versions and file actions are
//! shared in [`crate::files`].
use super::model::{Sheet, Workbook, col_name};
use super::render;
use crate::files::{self, Content, Kind, Showing, Stored, VersionItem, owner};
use crate::ui::{Shell, now_ms, page, script_json};
use askama::Template;
use loki_sdk::{Request, Response, Result};
use serde::{Deserialize, Serialize};

const EXCERPT_CHARS: usize = 160;

fn shell(request: &Request) -> Result<Shell> {
    Shell::new(request, Some(&super::SHEETS))
}
fn load(request: &Request) -> Result<Stored> {
    Stored::load(&owner(request)?, request.param("id")?, Kind::Sheet)
}
fn info(cells: usize, tabs: usize) -> String {
    let cells = if cells == 1 { "1 cell".into() } else { format!("{cells} cells") };
    if tabs > 1 { format!("{cells} in {tabs} tabs") } else { cells }
}
/// The tab chosen with `?tab=N`, else the first.
fn chosen_tab(request: &Request, book: &Workbook) -> usize {
    request
        .query("tab")
        .and_then(|t| t.parse::<usize>().ok())
        .filter(|t| *t < book.tabs.len())
        .unwrap_or(0)
}

#[derive(Template)]
#[template(path = "sheets/editor.html")]
struct EditorPage {
    shell: Shell,
    id: String,
    title: String,
    data: String,
}

struct ViewCell {
    text: String,
    class: String,
}
#[derive(Template)]
#[template(path = "sheets/view.html")]
struct ViewPage {
    shell: Shell,
    id: String,
    title: String,
    detail: String,
    /// Tab names, and which one is shown.
    tabs: Vec<String>,
    tab: usize,
    /// Column names and widths.
    columns: Vec<(String, u32)>,
    /// The table's width: the row header and every column.
    width: u32,
    /// Row numbers and cells.
    rows: Vec<(u32, Vec<ViewCell>)>,
    /// Set when showing an earlier version.
    version: Option<VersionItem>,
}

pub fn home(request: Request) -> Result<Response> {
    files::list_page(&request, Kind::Sheet)
}

pub fn create(request: Request) -> Result<Response> {
    let body = serde_json::to_string(&Workbook::default())?;
    let id = files::create(&owner(&request)?, Kind::Sheet, &body, &info(0, 1))?;
    Response::redirect(&format!("/sheets/{id}"))
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
        body: Workbook,
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
    pub body: Workbook,
}

/// Autosave: `POST /api/sheets/{id}` with a JSON [`SaveRequest`] and `X-CSRF-Token`.
pub fn save(mut request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let id = request.param("id")?.to_string();
    let payload: SaveRequest = request.json(files::MAX_SAVE_BYTES)?;
    save_sheet(&owner, &id, payload, now_ms())
}

pub fn save_sheet(owner: &str, id: &str, payload: SaveRequest, now: i64) -> Result<Response> {
    let content = content(payload.body)?;
    files::save(owner, id, Kind::Sheet, payload.version, &payload.title, content, now)
}

/// Validates a workbook and summarizes it: the excerpt comes from the first
/// tab, search covers every tab.
pub fn content(mut book: Workbook) -> Result<Content> {
    book.validate()?;
    book.normalize();
    let mut excerpt = String::new();
    let mut plain = Vec::new();
    for (index, tab) in book.tabs.iter().enumerate() {
        if book.tabs.len() > 1 {
            plain.push(tab.name.clone());
        }
        for (_, cell) in tab.filled() {
            let text = render::display(cell);
            if index == 0 && !text.trim().is_empty() && !excerpt.ends_with('…') {
                let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if excerpt.chars().count() + text.chars().count() + 3 > EXCERPT_CHARS {
                    excerpt.push_str(" …");
                } else {
                    if !excerpt.is_empty() {
                        excerpt.push_str(" · ");
                    }
                    excerpt.push_str(&text);
                }
            }
            plain.push(text);
        }
    }
    Ok(Content {
        body: serde_json::to_string(&book)?,
        excerpt,
        info: info(book.filled_cells(), book.tabs.len()),
        plain: plain.join("\n"),
    })
}

pub fn view(request: Request) -> Result<Response> {
    show(&request, load(&request)?.showing())
}

/// Column names and widths, the table width, and numbered rows of cells.
type ViewTable = (Vec<(String, u32)>, u32, Vec<(u32, Vec<ViewCell>)>);

fn view_rows(sheet: &Sheet) -> ViewTable {
    let (used_rows, used_cols) = sheet.used_size();
    let (rows, cols) = (used_rows.max(1), used_cols.max(1));
    let shown = render::shown_rows(sheet, rows);
    let cells = shown
        .into_iter()
        .map(|row| {
            let line = (0..cols)
                .map(|col| match sheet.cells.get(&format!("{}{}", col_name(col), row + 1)) {
                    Some(cell) => ViewCell {
                        text: render::display(cell),
                        class: render::classes(cell),
                    },
                    None => ViewCell {
                        text: String::new(),
                        class: String::new(),
                    },
                })
                .collect();
            (row + 1, line)
        })
        .collect();
    let columns = (0..cols).map(|col| (col_name(col), sheet.width(col))).collect();
    let width = 48 + (0..cols).map(|col| sheet.width(col)).sum::<u32>();
    (columns, width, cells)
}

/// The read-only page, for the spreadsheet now or an earlier version.
pub fn show(request: &Request, showing: Showing) -> Result<Response> {
    let book: Workbook = showing.body()?;
    let tab = chosen_tab(request, &book);
    let (columns, width, rows) = view_rows(&book.tabs[tab]);
    page(Response::render(ViewPage {
        shell: shell(request)?,
        tabs: book.tabs.iter().map(|t| t.name.clone()).collect(),
        tab,
        columns,
        width,
        rows,
        detail: showing.detail,
        id: showing.id,
        title: showing.title,
        version: showing.version,
    })?)
}

/// `GET /sheets/{id}/export.csv?tab=N`: one tab as CSV, the first by default.
pub fn export_csv(request: Request) -> Result<Response> {
    let stored = load(&request)?;
    let book: Workbook = stored.body()?;
    let tab = chosen_tab(&request, &book);
    let csv = render::to_csv(&book.tabs[tab]);
    let name = if book.tabs.len() > 1 {
        format!("{} - {}", stored.title, book.tabs[tab].name)
    } else {
        stored.title.clone()
    };
    files::attachment(
        Response::text(csv).header("content-type", "text/csv; charset=utf-8")?,
        &name,
        "csv",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_host::{context, host};
    use loki_sdk::{Method, testing};
    use serde_json::json;

    fn payload(tabs: serde_json::Value) -> SaveRequest {
        serde_json::from_value(json!({"title": "Budget", "version": 2, "body": {"tabs": tabs}})).unwrap()
    }

    #[test]
    fn save_summarizes_cells_in_reading_order() {
        let (log, fake) = host(|_| json!({"rows": [], "affected": 1}));
        let request = payload(json!([
            {"name": "Q1", "rows": 10, "cols": 5, "cells": {
                "B1": {"input": "=A1*2", "value": 0.5, "fmt": "percent"},
                "A1": {"input": "0.25", "value": 0.25},
                "A2": {"input": "Rent", "value": "Rent"},
                "C9": {"bold": true}
            }},
            {"name": "Notes", "cells": {"A1": {"input": "later", "value": "later"}}}
        ]));
        let response = testing::with_host(fake, || save_sheet("user-1", "s-1", request, 5).unwrap());
        assert_eq!(response.status, 200);
        let params = log.borrow()[1]["params"].clone();
        assert_eq!(params[1], "0.25 · 50% · Rent");
        assert_eq!(params[2], "4 cells in 2 tabs");
        assert_eq!(params[3], "Q1\n0.25\n50%\nRent\nNotes\nlater");
        assert!(params[4].as_str().unwrap().contains(r#""C9":{"bold":true}"#));
        assert_eq!(params[8], "sheet");
    }

    #[test]
    fn invalid_sheets_are_rejected_before_the_database() {
        let (log, fake) = host(|_| json!({"rows": [], "affected": 1}));
        let request = payload(json!([{"rows": 10, "cols": 5, "cells": {"Z1": {"input": "outside"}}}]));
        let error = testing::with_host(fake, || save_sheet("user-1", "s-1", request, 5).unwrap_err());
        assert_eq!(error.status(), 400);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn csv_export_picks_a_tab() {
        let body = r#"{"tabs":[{"name":"A","cells":{"A1":{"input":"1","value":1}}},
                               {"name":"B","cells":{"B2":{"input":"=2+2","value":4}}}]}"#;
        let (log, fake) = host(move |_| {
            json!({"rows": [{
                "id": "s-1", "title": "Q3", "version": 1, "info": "", "updated_at": 0, "trashed_at": null, "body": body
            }], "affected": 0})
        });
        let request = testing::request(Method::Get, "/sheets/s-1/export.csv?tab=1", context(true));
        let response = testing::with_host(fake, || crate::router().handle(request).unwrap());
        assert_eq!(String::from_utf8(response.body).unwrap(), ",\r\n,4\r\n");
        assert_eq!(response.headers["content-type"], "text/csv; charset=utf-8");
        assert!(response.headers["content-disposition"].contains("Q3 - B.csv"));
        assert_eq!(log.borrow()[0]["params"], json!(["s-1", "user-1", "sheet"]));
    }

    #[test]
    fn spreadsheets_from_before_tabs_still_export() {
        let (_, fake) = host(|_| {
            json!({"rows": [{
                "id": "s-1", "title": "Old", "version": 1, "info": "", "updated_at": 0, "trashed_at": null,
                "body": r#"{"rows":10,"cols":5,"cells":{"A1":{"input":"x","value":"x"}}}"#
            }], "affected": 0})
        });
        let request = testing::request(Method::Get, "/sheets/s-1/export.csv", context(true));
        let response = testing::with_host(fake, || crate::router().handle(request).unwrap());
        assert_eq!(String::from_utf8(response.body).unwrap(), "x\r\n");
    }
}
