//! Sheets pages and autosave. Listing, trash and file actions are shared in
//! [`crate::files`].
use super::model::{Sheet, col_name};
use super::render;
use crate::files::{self, Content, Kind, Stored, owner};
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
fn cells(count: usize) -> String {
    if count == 1 { "1 cell".into() } else { format!("{count} cells") }
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
    /// Column names and widths.
    columns: Vec<(String, u32)>,
    /// The table's width: the row header and every column.
    width: u32,
    rows: Vec<Vec<ViewCell>>,
}

pub fn home(request: Request) -> Result<Response> {
    files::list_page(&request, Kind::Sheet)
}

pub fn create(request: Request) -> Result<Response> {
    let body = serde_json::to_string(&Sheet::default())?;
    let id = files::create(&owner(&request)?, Kind::Sheet, &body, &cells(0))?;
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
        body: Sheet,
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
    pub body: Sheet,
}

/// Autosave: `POST /api/sheets/{id}` with a JSON [`SaveRequest`] and `X-CSRF-Token`.
pub fn save(mut request: Request) -> Result<Response> {
    let owner = owner(&request)?;
    let id = request.param("id")?.to_string();
    let payload: SaveRequest = request.json(files::MAX_SAVE_BYTES)?;
    save_sheet(&owner, &id, payload, now_ms())
}

pub fn save_sheet(owner: &str, id: &str, payload: SaveRequest, now: i64) -> Result<Response> {
    let mut sheet = payload.body;
    sheet.validate()?;
    sheet.normalize();
    let filled = sheet.filled();
    let texts: Vec<String> = filled.iter().map(|(_, cell)| render::display(cell)).collect();
    let mut excerpt = String::new();
    for text in texts.iter().filter(|t| !t.trim().is_empty()) {
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if excerpt.chars().count() + text.chars().count() + 3 > EXCERPT_CHARS {
            excerpt.push_str(" …");
            break;
        }
        if !excerpt.is_empty() {
            excerpt.push_str(" · ");
        }
        excerpt.push_str(&text);
    }
    let content = Content {
        body: serde_json::to_string(&sheet)?,
        excerpt,
        info: cells(filled.len()),
        plain: texts.join("\n"),
    };
    files::save(owner, id, Kind::Sheet, payload.version, &payload.title, content, now)
}

pub fn view(request: Request) -> Result<Response> {
    let stored = load(&request)?;
    let sheet: Sheet = stored.body()?;
    let (used_rows, used_cols) = sheet.used_size();
    let (rows, cols) = (used_rows.max(1), used_cols.max(1));
    let cells = (0..rows)
        .map(|row| {
            (0..cols)
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
                .collect()
        })
        .collect();
    page(Response::render(ViewPage {
        shell: shell(&request)?,
        detail: stored.detail(),
        columns: (0..cols).map(|col| (col_name(col), sheet.width(col))).collect(),
        width: 48 + (0..cols).map(|col| sheet.width(col)).sum::<u32>(),
        rows: cells,
        id: stored.id,
        title: stored.title,
    })?)
}

pub fn export_csv(request: Request) -> Result<Response> {
    let stored = load(&request)?;
    let csv = render::to_csv(&stored.body()?);
    files::attachment(
        Response::text(csv).header("content-type", "text/csv; charset=utf-8")?,
        &stored.title,
        "csv",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_host::{context, host};
    use loki_sdk::{Method, testing};
    use serde_json::json;

    fn payload(cells: serde_json::Value) -> SaveRequest {
        serde_json::from_value(json!({"title": "Budget", "version": 2, "body": {"rows": 10, "cols": 5, "cells": cells}}))
            .unwrap()
    }

    #[test]
    fn save_summarizes_cells_in_reading_order() {
        let (log, fake) = host(|_| json!({"rows": [], "affected": 1}));
        let request = payload(json!({
            "B1": {"input": "=A1*2", "value": 0.5, "fmt": "percent"},
            "A1": {"input": "0.25", "value": 0.25},
            "A2": {"input": "Rent", "value": "Rent"},
            "C9": {"bold": true}
        }));
        let response = testing::with_host(fake, || save_sheet("user-1", "s-1", request, 5).unwrap());
        assert_eq!(response.status, 200);
        let params = log.borrow()[0]["params"].clone();
        assert_eq!(params[1], "0.25 · 50% · Rent");
        assert_eq!(params[2], "3 cells");
        assert_eq!(params[3], "0.25\n50%\nRent");
        assert!(params[4].as_str().unwrap().contains(r#""C9":{"bold":true}"#));
        assert_eq!(params[8], "sheet");
    }

    #[test]
    fn invalid_sheets_are_rejected_before_the_database() {
        let (log, fake) = host(|_| json!({"rows": [], "affected": 1}));
        let request = payload(json!({"Z1": {"input": "outside"}}));
        let error = testing::with_host(fake, || save_sheet("user-1", "s-1", request, 5).unwrap_err());
        assert_eq!(error.status(), 400);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn csv_export_is_an_attachment() {
        let (log, fake) = host(|_| {
            json!({"rows": [{
                "id": "s-1", "title": "Q3", "version": 1, "info": "1 cell", "updated_at": 0, "trashed_at": null,
                "body": r#"{"rows":10,"cols":5,"cells":{"B2":{"input":"=2+2","value":4}}}"#
            }], "affected": 0})
        });
        let request = testing::request(Method::Get, "/sheets/s-1/export.csv", context(true));
        let response = testing::with_host(fake, || crate::router().handle(request).unwrap());
        assert_eq!(String::from_utf8(response.body).unwrap(), ",\r\n,4\r\n");
        assert_eq!(response.headers["content-type"], "text/csv; charset=utf-8");
        assert_eq!(log.borrow()[0]["params"], json!(["s-1", "user-1", "sheet"]));
    }
}
