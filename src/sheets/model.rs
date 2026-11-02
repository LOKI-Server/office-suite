//! The spreadsheet model. The browser evaluates formulas as you type and sends
//! each cell's input together with its last calculated value, as spreadsheet
//! files do; the server validates both and uses the values for exports and the
//! read-only view.
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
/// Error values the formula engine produces.
pub const ERRORS: &[&str] = &["#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A", "#ERROR!", "#CYCLE!"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
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
            rows: DEFAULT_ROWS,
            cols: DEFAULT_COLS,
            widths: BTreeMap::new(),
            cells: BTreeMap::new(),
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

impl Sheet {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=MAX_ROWS).contains(&self.rows) || !(1..=MAX_COLS).contains(&self.cols) {
            return Err(format!("Spreadsheets can have up to {MAX_ROWS} rows and {MAX_COLS} columns."));
        }
        if self.cells.len() > MAX_CELLS {
            return Err(format!("Spreadsheets can have up to {MAX_CELLS} filled cells."));
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

    /// Drops cells with neither content nor formatting, and values of empty cells.
    pub fn normalize(&mut self) {
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
        let value = json!({"rows": 100, "cols": 26, "widths": {"B": 160}, "cells": {
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
}
