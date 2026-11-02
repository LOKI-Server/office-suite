//! Shows cell values as text, and exports a spreadsheet as CSV. Number
//! formatting matches `static/sheet.js`, which formats the same values in the
//! browser.
use super::model::{Align, Cell, CellValue, NumFmt, Sheet, col_name};

/// A number as the spreadsheet shows it without a format: at most 12
/// significant digits, so 0.1 + 0.2 shows as 0.3.
pub fn general(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    let rounded: f64 = format!("{n:.11e}").parse().unwrap_or(n);
    let abs = rounded.abs();
    if abs >= 1e21 || abs < 1e-6 {
        // Exponent form, written as JavaScript writes it: 1.5e+21, 2e-7.
        let text = format!("{rounded:e}");
        return match text.split_once('e') {
            Some((mantissa, exp)) if !exp.starts_with('-') => format!("{mantissa}e+{exp}"),
            _ => text,
        };
    }
    format!("{rounded}")
}

/// `n` with `decimals` decimal places and thousands separators.
fn grouped(n: f64, decimals: usize) -> String {
    let fixed = format!("{:.*}", decimals, n.abs());
    let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let mut out = String::new();
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    if !fraction.is_empty() {
        out.push('.');
        out.push_str(fraction);
    }
    let negative = n < 0.0 && out.chars().any(|c| c.is_ascii_digit() && c != '0');
    if negative { format!("-{out}") } else { out }
}

pub fn format_number(n: f64, fmt: Option<NumFmt>) -> String {
    match fmt {
        None => general(n),
        Some(NumFmt::Number) => grouped(n, 2),
        Some(NumFmt::Integer) => grouped(n, 0),
        Some(NumFmt::Percent) => {
            let text = format!("{:.2}", n * 100.0);
            let text = text.trim_end_matches('0').trim_end_matches('.');
            let text = if text == "-0" { "0" } else { text };
            format!("{text}%")
        }
        Some(NumFmt::Currency) => {
            let text = grouped(n, 2);
            match text.strip_prefix('-') {
                Some(rest) => format!("-${rest}"),
                None => format!("${text}"),
            }
        }
    }
}

/// The text a cell shows.
pub fn display(cell: &Cell) -> String {
    match &cell.value {
        Some(CellValue::Number(n)) => format_number(*n, cell.fmt),
        Some(CellValue::Bool(b)) => if *b { "TRUE" } else { "FALSE" }.into(),
        Some(CellValue::Text(text)) => text.clone(),
        Some(CellValue::Error { error }) => error.clone(),
        None => cell.input.clone(),
    }
}

/// CSS classes for a cell in the read-only view: numbers right-aligned, errors
/// and booleans centred, unless the cell sets its own alignment.
pub fn classes(cell: &Cell) -> String {
    let mut classes = Vec::new();
    let natural = match cell.value {
        Some(CellValue::Number(_)) => "right",
        Some(CellValue::Bool(_)) | Some(CellValue::Error { .. }) => "center",
        _ => "left",
    };
    let align = match cell.align {
        Some(Align::Left) => "left",
        Some(Align::Center) => "center",
        Some(Align::Right) => "right",
        None => natural,
    };
    if align != "left" {
        classes.push(format!("align-{align}"));
    }
    if cell.bold {
        classes.push("bold".into());
    }
    if cell.italic {
        classes.push("italic".into());
    }
    if matches!(cell.value, Some(CellValue::Error { .. })) {
        classes.push("error".into());
    }
    classes.join(" ")
}

/// The used part of the spreadsheet as CSV, one line per row, values unformatted.
pub fn to_csv(sheet: &Sheet) -> String {
    let (rows, cols) = sheet.used_size();
    let mut out = String::new();
    for row in 0..rows {
        let line: Vec<String> = (0..cols)
            .map(|col| {
                let key = format!("{}{}", col_name(col), row + 1);
                match sheet.cells.get(&key) {
                    None => String::new(),
                    Some(cell) => csv_field(cell),
                }
            })
            .collect();
        out.push_str(&line.join(","));
        out.push_str("\r\n");
    }
    out
}

fn csv_field(cell: &Cell) -> String {
    let text = match &cell.value {
        Some(CellValue::Number(n)) => return general(*n),
        Some(CellValue::Bool(b)) => return if *b { "TRUE" } else { "FALSE" }.into(),
        Some(CellValue::Error { error }) => error.clone(),
        Some(CellValue::Text(text)) => text.clone(),
        None => cell.input.clone(),
    };
    // Text that looks like a formula would run when the CSV is opened elsewhere.
    let text = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_numbers_like_the_browser() {
        assert_eq!(general(0.1 + 0.2), "0.3");
        assert_eq!(general(42.0), "42");
        assert_eq!(general(-1.5), "-1.5");
        assert_eq!(general(1.0 / 3.0), "0.333333333333");
        assert_eq!(general(1e21), "1e+21");
        assert_eq!(general(2e-7), "2e-7");
        assert_eq!(format_number(1234.5, Some(NumFmt::Number)), "1,234.50");
        assert_eq!(format_number(-1234567.0, Some(NumFmt::Integer)), "-1,234,567");
        assert_eq!(format_number(0.125, Some(NumFmt::Percent)), "12.5%");
        assert_eq!(format_number(0.5, Some(NumFmt::Percent)), "50%");
        assert_eq!(format_number(-1234.5, Some(NumFmt::Currency)), "-$1,234.50");
        assert_eq!(format_number(-0.001, Some(NumFmt::Number)), "0.00");
    }

    #[test]
    fn exports_csv_safely() {
        let sheet: Sheet = serde_json::from_value(json!({"cells": {
            "A1": {"input": "Name", "value": "Name"},
            "B1": {"input": "Note", "value": "Note"},
            "A2": {"input": "=1/3", "value": 0.333333333333333, "fmt": "number"},
            "B2": {"input": "Say \"hi\", ok", "value": "Say \"hi\", ok"},
            "C3": {"input": "'=cmd", "value": "=cmd"},
            "A3": {"input": "TRUE", "value": true}
        }}))
        .unwrap();
        assert_eq!(
            to_csv(&sheet),
            "Name,Note,\r\n0.333333333333,\"Say \"\"hi\"\", ok\",\r\nTRUE,,'=cmd\r\n"
        );
    }

    #[test]
    fn classes_follow_value_type_and_format() {
        let cell: Cell = serde_json::from_value(json!({"input": "1", "value": 1, "bold": true})).unwrap();
        assert_eq!(classes(&cell), "align-right bold");
        let error: Cell = serde_json::from_value(json!({"input": "=x", "value": {"error": "#NAME?"}, "align": "left"})).unwrap();
        assert_eq!(classes(&error), "error");
    }
}
