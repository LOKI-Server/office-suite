//! The spreadsheet model: a workbook of tabs. The browser evaluates formulas
//! as you type and sends each cell's input together with its last calculated
//! value, as spreadsheet files do; the server validates both and uses the
//! values for exports and the read-only view.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_ROWS: u32 = 100;
pub const DEFAULT_COLS: u32 = 26;
pub const MAX_ROWS: u32 = 2_000;
pub const MAX_COLS: u32 = 52;
pub const MAX_CELLS: usize = 50_000;
pub const MAX_INPUT_BYTES: usize = 8 * 1024;
pub const MAX_TEXT_BYTES: usize = 32 * 1024;
pub const MIN_WIDTH: u32 = 24;
pub const MAX_WIDTH: u32 = 800;
pub const MAX_TABS: usize = 20;
pub const MAX_NAME_CHARS: usize = 40;
pub const MAX_FROZEN_ROWS: u32 = 10;
pub const MAX_FROZEN_COLS: u32 = 5;
pub const MAX_FILTER_CHARS: usize = 200;
/// Characters a tab name cannot hold, so `'Name'!A1` never needs escaping.
const NAME_FORBIDDEN: &[char] = &['\'', '!', ':', '\\', '/', '?', '*', '[', ']'];
/// Error values the formula engine produces.
pub const ERRORS: &[&str] = &["#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A", "#ERROR!", "#CYCLE!"];

/// A spreadsheet: one or more named tabs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "Stored")]
pub struct Workbook {
    pub tabs: Vec<Sheet>,
}
/// Spreadsheets saved before tabs existed hold a single sheet.
#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    Tabs { tabs: Vec<Sheet> },
    Single(Sheet),
}
impl From<Stored> for Workbook {
    fn from(stored: Stored) -> Self {
        match stored {
            Stored::Tabs { tabs } => Self { tabs },
            Stored::Single(sheet) => Self { tabs: vec![sheet] },
        }
    }
}
impl Default for Workbook {
    fn default() -> Self {
        Self {
            tabs: vec![Sheet::default()],
        }
    }
}

/// One tab.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_rows")]
    pub rows: u32,
    #[serde(default = "default_cols")]
    pub cols: u32,
    /// Column widths in pixels by column name, for columns that are not the default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub widths: BTreeMap<String, u32>,
    /// Cells by name, such as `B7`.
    #[serde(default)]
    pub cells: BTreeMap<String, Cell>,
    /// Rows and columns that stay in view while scrolling.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub frozen_rows: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub frozen_cols: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
}

/// Shows only the rows whose cell in `col` contains `query`, ignoring case.
/// The header rows (the frozen rows, or else the first row) always show.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub col: u32,
    pub query: String,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}
fn default_name() -> String {
    "Sheet1".into()
}
fn default_rows() -> u32 {
    DEFAULT_ROWS
}
fn default_cols() -> u32 {
    DEFAULT_COLS
}
impl Default for Sheet {
    fn default() -> Self {
        Self {
            name: default_name(),
            rows: DEFAULT_ROWS,
            cols: DEFAULT_COLS,
            widths: BTreeMap::new(),
            cells: BTreeMap::new(),
            frozen_rows: 0,
            frozen_cols: 0,
            filter: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    /// What was typed: a number, text, `TRUE`/`FALSE`, or a formula starting with `=`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<CellValue>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fmt: Option<NumFmt>,
}
fn is_false(value: &bool) -> bool {
    !value
}
impl Cell {
    fn is_blank(&self) -> bool {
        self.input.is_empty() && !self.bold && !self.italic && self.align.is_none() && self.fmt.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CellValue {
    Number(f64),
    Bool(bool),
    Text(String),
    Error { error: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NumFmt {
    /// 1,234.50
    Number,
    /// 1,235
    Integer,
    /// 12.5%
    Percent,
    /// $1,234.50
    Currency,
}

/// The name of a 0-based column: A … Z, AA … AZ.
pub fn col_name(col: u32) -> String {
    let mut name = String::new();
    let mut n = col + 1;
    while n > 0 {
        let rem = (n - 1) % 26;
        name.insert(0, (b'A' + rem as u8) as char);
        n = (n - 1) / 26;
    }
    name
}

fn parse_col(letters: &str) -> Option<u32> {
    if letters.is_empty() || letters.len() > 2 || !letters.bytes().all(|b| b.is_ascii_uppercase()) {
        return None;
    }
    Some(letters.bytes().fold(0, |n, b| n * 26 + u32::from(b - b'A') + 1) - 1)
}

/// Parses a cell name such as `B7` into a 0-based (column, row).
pub fn parse_key(key: &str) -> Option<(u32, u32)> {
    let split = key.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = key.split_at(split);
    if digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) || digits.len() > 6 {
        return None;
    }
    let row: u32 = digits.parse().ok()?;
    Some((parse_col(letters)?, row - 1))
}

impl Workbook {
    pub fn validate(&self) -> Result<(), String> {
        if self.tabs.is_empty() || self.tabs.len() > MAX_TABS {
            return Err(format!("Spreadsheets can have 1 to {MAX_TABS} tabs."));
        }
        if self.tabs.iter().map(|tab| tab.cells.len()).sum::<usize>() > MAX_CELLS {
            return Err(format!("Spreadsheets can have up to {MAX_CELLS} filled cells."));
        }
        let mut names = std::collections::BTreeSet::new();
        for tab in &self.tabs {
            check_name(&tab.name)?;
            if !names.insert(tab.name.to_lowercase()) {
                return Err(format!("Two tabs are named “{}”.", tab.name));
            }
            tab.validate()?;
        }
        Ok(())
    }

    pub fn normalize(&mut self) {
        for tab in &mut self.tabs {
            tab.normalize();
        }
    }

    pub fn filled_cells(&self) -> usize {
        self.tabs.iter().map(|tab| tab.filled().len()).sum()
    }
}

/// Tab names: 1–40 characters, none of `' ! : \ / ? * [ ]`, no control characters.
pub fn check_name(name: &str) -> Result<(), String> {
    let count = name.chars().count();
    if count == 0 || count > MAX_NAME_CHARS || name.trim() != name {
        return Err(format!("Tab names need 1 to {MAX_NAME_CHARS} characters, without spaces at the ends."));
    }
    if name.chars().any(|c| c.is_control() || NAME_FORBIDDEN.contains(&c)) {
        return Err(format!("“{name}” can't be a tab name: avoid ' ! : \\ / ? * [ ]."));
    }
    Ok(())
}

impl Sheet {
    fn validate(&self) -> Result<(), String> {
        if !(1..=MAX_ROWS).contains(&self.rows) || !(1..=MAX_COLS).contains(&self.cols) {
            return Err(format!("Spreadsheets can have up to {MAX_ROWS} rows and {MAX_COLS} columns."));
        }
        if self.frozen_rows > MAX_FROZEN_ROWS.min(self.rows - 1) || self.frozen_cols > MAX_FROZEN_COLS.min(self.cols - 1) {
            return Err(format!("Up to {MAX_FROZEN_ROWS} rows and {MAX_FROZEN_COLS} columns can be frozen."));
        }
        if let Some(filter) = &self.filter
            && (filter.col >= self.cols || filter.query.chars().count() > MAX_FILTER_CHARS || filter.query.chars().any(char::is_control))
        {
            return Err("The filter is invalid.".into());
        }
        for (name, width) in &self.widths {
            match parse_col(name) {
                Some(col) if col < self.cols && (MIN_WIDTH..=MAX_WIDTH).contains(width) => {}
                _ => return Err(format!("Column {name} has an invalid width.")),
            }
        }
        for (key, cell) in &self.cells {
            match parse_key(key) {
                Some((col, row)) if col < self.cols && row < self.rows => {}
                _ => return Err(format!("{key} is outside the spreadsheet.")),
            }
            if cell.input.len() > MAX_INPUT_BYTES {
                return Err(format!("{key} holds more than {MAX_INPUT_BYTES} bytes."));
            }
            if cell.input.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
                return Err(format!("{key} contains control characters."));
            }
            match &cell.value {
                Some(CellValue::Number(n)) if !n.is_finite() => {
                    return Err(format!("{key} holds an invalid number."));
                }
                Some(CellValue::Text(text)) if text.len() > MAX_TEXT_BYTES => {
                    return Err(format!("{key} holds too much text."));
                }
                Some(CellValue::Error { error }) if !ERRORS.contains(&error.as_str()) => {
                    return Err(format!("{key} holds an unknown error value."));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Drops cells with neither content nor formatting, and values of empty
    /// cells, and an empty filter.
    pub fn normalize(&mut self) {
        if self.filter.as_ref().is_some_and(|f| f.query.is_empty()) {
            self.filter = None;
        }
        self.cells.retain(|_, cell| !cell.is_blank());
        for cell in self.cells.values_mut() {
            if cell.input.is_empty() {
                cell.value = None;
            }
        }
    }

    /// Filled cells in reading order: row by row, left to right.
    pub fn filled(&self) -> Vec<((u32, u32), &Cell)> {
        let mut cells: Vec<_> = self
            .cells
            .iter()
            .filter(|(_, cell)| !cell.input.is_empty())
            .filter_map(|(key, cell)| parse_key(key).map(|(col, row)| ((row, col), cell)))
            .collect();
        cells.sort_by_key(|(position, _)| *position);
        cells
    }

    /// The smallest (rows, columns) holding every filled cell.
    pub fn used_size(&self) -> (u32, u32) {
        self.filled()
            .iter()
            .fold((0, 0), |(rows, cols), ((row, col), _)| (rows.max(row + 1), cols.max(col + 1)))
    }

    pub fn width(&self, col: u32) -> u32 {
        self.widths.get(&col_name(col)).copied().unwrap_or(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sheet(value: serde_json::Value) -> Sheet {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn names_columns_and_cells() {
        assert_eq!((col_name(0), col_name(25), col_name(26), col_name(51)), ("A".into(), "Z".into(), "AA".into(), "AZ".into()));
        assert_eq!(parse_key("A1"), Some((0, 0)));
        assert_eq!(parse_key("AZ2000"), Some((51, 1999)));
        for bad in ["A0", "a1", "1A", "A01", "ABC1", "A", "A1B"] {
            assert_eq!(parse_key(bad), None, "{bad}");
        }
    }

    #[test]
    fn round_trips_values_and_formats() {
        let value = json!({"name": "Sheet1", "rows": 100, "cols": 26, "widths": {"B": 160}, "cells": {
            "A1": {"input": "Total", "value": "Total", "bold": true},
            "B1": {"input": "=SUM(B2:B3)", "value": 3.5, "fmt": "currency", "align": "center"},
            "C1": {"input": "TRUE", "value": true},
            "D1": {"input": "=1/0", "value": {"error": "#DIV/0!"}}
        }});
        let parsed = sheet(value.clone());
        assert!(parsed.validate().is_ok());
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
        assert_eq!(parsed.cells["D1"].value, Some(CellValue::Error { error: "#DIV/0!".into() }));
    }

    #[test]
    fn rejects_cells_outside_or_invalid() {
        for value in [
            json!({"rows": 10, "cols": 2, "cells": {"C1": {"input": "x"}}}),
            json!({"rows": 10, "cols": 2, "cells": {"A11": {"input": "x"}}}),
            json!({"rows": 10, "cols": 2, "cells": {"A1": {"input": "x", "value": {"error": "#BOOM"}}}}),
            json!({"rows": 10, "cols": 2, "cells": {"A1": {"input": "a\u{0}"}}}),
            json!({"rows": 10, "cols": 2, "widths": {"A": 5}}),
            json!({"rows": 0, "cols": 2}),
            json!({"rows": 10, "cols": 53}),
        ] {
            assert!(sheet(value.clone()).validate().is_err(), "{value}");
        }
        let long = json!({"cells": {"A1": {"input": "x".repeat(MAX_INPUT_BYTES + 1)}}});
        assert!(sheet(long).validate().is_err());
    }

    #[test]
    fn normalizes_and_orders_cells() {
        let mut parsed = sheet(json!({"cells": {
            "B2": {"input": "b"}, "A10": {"input": "c"}, "A2": {"input": "a"},
            "C3": {}, "D4": {"bold": true, "value": 4}
        }}));
        parsed.normalize();
        assert!(!parsed.cells.contains_key("C3"));
        assert_eq!(parsed.cells["D4"].value, None);
        let order: Vec<_> = parsed.filled().iter().map(|(_, c)| c.input.clone()).collect();
        assert_eq!(order, ["a", "b", "c"]);
        assert_eq!(parsed.used_size(), (10, 2));
    }

    #[test]
    fn reads_spreadsheets_from_before_tabs() {
        let book: Workbook = serde_json::from_value(json!({"rows": 5, "cols": 3, "cells": {"A1": {"input": "x"}}})).unwrap();
        assert_eq!(book.tabs.len(), 1);
        assert_eq!(book.tabs[0].name, "Sheet1");
        assert_eq!(book.tabs[0].rows, 5);
        let saved = serde_json::to_value(&book).unwrap();
        assert_eq!(saved["tabs"][0]["cells"]["A1"]["input"], "x");
        let again: Workbook = serde_json::from_value(saved).unwrap();
        assert_eq!(again, book);
    }

    #[test]
    fn checks_tabs_names_and_totals() {
        let book = |tabs: serde_json::Value| -> Workbook { serde_json::from_value(json!({ "tabs": tabs })).unwrap() };
        assert!(book(json!([{"name": "Q1"}, {"name": "Q 2 (draft)"}])).validate().is_ok());
        assert!(book(json!([{"name": "Q1"}, {"name": "q1"}])).validate().is_err());
        assert!(book(json!([])).validate().is_err());
        for bad in ["", " x", "a'b", "a!b", "x".repeat(41).as_str(), "a/b"] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
        let many: Vec<_> = (0..21).map(|i| json!({"name": format!("T{i}")})).collect();
        assert!(book(json!(many)).validate().is_err());
        // 2,600 cells per tab: 19 tabs fit under the limit, 20 do not.
        let cells: serde_json::Map<String, serde_json::Value> = (0..26)
            .flat_map(|c| (1..=100).map(move |r| (format!("{}{r}", col_name(c)), json!({"input": "x"}))))
            .collect();
        let tabs = |n: usize| -> serde_json::Value {
            json!((0..n).map(|i| json!({"name": format!("T{i}"), "cells": cells})).collect::<Vec<_>>())
        };
        assert!(book(tabs(19)).validate().is_ok());
        assert!(book(tabs(20)).validate().is_err());
    }

    #[test]
    fn checks_frozen_panes_and_filters() {
        let ok = sheet(json!({"rows": 10, "cols": 5, "frozen_rows": 1, "frozen_cols": 2, "filter": {"col": 4, "query": "rent"}}));
        assert!(ok.validate().is_ok());
        for value in [
            json!({"rows": 10, "cols": 5, "frozen_rows": 11}),
            json!({"rows": 2, "cols": 5, "frozen_rows": 2}),
            json!({"rows": 10, "cols": 5, "frozen_cols": 6}),
            json!({"rows": 10, "cols": 5, "filter": {"col": 5, "query": "x"}}),
        ] {
            assert!(sheet(value.clone()).validate().is_err(), "{value}");
        }
        let mut empty = sheet(json!({"filter": {"col": 0, "query": ""}}));
        empty.normalize();
        assert_eq!(empty.filter, None);
    }
}
