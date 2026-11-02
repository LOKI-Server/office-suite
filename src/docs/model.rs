//! The rich-text document model. The editor sends this structure, never HTML,
//! and the server validates it before storing it as JSON.
use serde::{Deserialize, Serialize};

pub const MAX_BLOCKS: usize = 5_000;
pub const MAX_TEXT_BYTES: usize = 512 * 1024;
pub const MAX_LINK_BYTES: usize = 2_048;
/// List items nest up to this many levels below the first.
pub const MAX_LIST_LEVEL: u8 = 5;
pub const MAX_TABLE_ROWS: usize = 500;
pub const MAX_TABLE_COLS: usize = 20;
/// Image types Docs accepts, by file extension.
pub const IMAGE_TYPES: &[(&str, &str)] = &[
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Document {
    #[serde(default)]
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Paragraph {
        #[serde(default)]
        runs: Vec<Run>,
        #[serde(default, skip_serializing_if = "Align::is_left")]
        align: Align,
    },
    Heading {
        level: u8,
        #[serde(default)]
        runs: Vec<Run>,
        #[serde(default, skip_serializing_if = "Align::is_left")]
        align: Align,
    },
    Quote {
        #[serde(default)]
        runs: Vec<Run>,
    },
    Code {
        #[serde(default)]
        text: String,
    },
    BulletList {
        items: Vec<Item>,
    },
    OrderedList {
        items: Vec<Item>,
    },
    Divider,
    /// Rows of cells; with `header`, the first row is a heading row.
    Table {
        rows: Vec<Vec<Vec<Run>>>,
        #[serde(default, skip_serializing_if = "is_false")]
        header: bool,
    },
    /// An uploaded image, by its file name (see [`check_image_name`]).
    Image {
        src: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        alt: String,
    },
}

/// A list item; `level` is how deeply it is nested, from 0.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "StoredItem")]
pub struct Item {
    pub runs: Vec<Run>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub level: u8,
}
/// Documents saved before nested lists held items as plain run lists.
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredItem {
    Runs(Vec<Run>),
    Item {
        #[serde(default)]
        runs: Vec<Run>,
        #[serde(default)]
        level: u8,
    },
}
impl From<StoredItem> for Item {
    fn from(stored: StoredItem) -> Self {
        match stored {
            StoredItem::Runs(runs) => Item { runs, level: 0 },
            StoredItem::Item { runs, level } => Item { runs, level },
        }
    }
}
fn is_zero(n: &u8) -> bool {
    *n == 0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}
impl Align {
    fn is_left(&self) -> bool {
        *self == Align::Left
    }
    pub fn class(self) -> Option<&'static str> {
        match self {
            Align::Left => None,
            Align::Center => Some("align-center"),
            Align::Right => Some("align-right"),
            Align::Justify => Some("align-justify"),
        }
    }
}

/// A span of text sharing one set of marks. `\n` is a line break.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strike: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub code: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}
fn is_false(value: &bool) -> bool {
    !value
}
impl Run {
    #[cfg(test)]
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
    fn same_marks(&self, other: &Run) -> bool {
        self.bold == other.bold
            && self.italic == other.italic
            && self.underline == other.underline
            && self.strike == other.strike
            && self.code == other.code
            && self.link == other.link
    }
}

impl Block {
    /// Every run of text in the block, in order.
    pub fn runs(&self) -> Vec<&[Run]> {
        match self {
            Block::Paragraph { runs, .. } | Block::Heading { runs, .. } | Block::Quote { runs } => {
                vec![runs]
            }
            Block::BulletList { items } | Block::OrderedList { items } => {
                items.iter().map(|item| item.runs.as_slice()).collect()
            }
            Block::Table { rows, .. } => rows.iter().flatten().map(Vec::as_slice).collect(),
            Block::Code { .. } | Block::Divider | Block::Image { .. } => Vec::new(),
        }
    }
    fn runs_mut(&mut self) -> Vec<&mut Vec<Run>> {
        match self {
            Block::Paragraph { runs, .. } | Block::Heading { runs, .. } | Block::Quote { runs } => {
                vec![runs]
            }
            Block::BulletList { items } | Block::OrderedList { items } => {
                items.iter_mut().map(|item| &mut item.runs).collect()
            }
            Block::Table { rows, .. } => rows.iter_mut().flatten().collect(),
            Block::Code { .. } | Block::Divider | Block::Image { .. } => Vec::new(),
        }
    }
    /// The block's text with marks dropped; list items, table cells and lines
    /// are joined by `\n`.
    pub fn plain_text(&self) -> String {
        match self {
            Block::Code { text } => text.clone(),
            Block::Divider => String::new(),
            Block::Image { alt, .. } => alt.clone(),
            _ => self
                .runs()
                .iter()
                .map(|runs| runs.iter().map(|r| r.text.as_str()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

impl Document {
    /// Checks limits and content rules. Returns a message fit to show the writer.
    pub fn validate(&self) -> Result<(), String> {
        if self.blocks.len() > MAX_BLOCKS {
            return Err(format!(
                "Documents can have at most {MAX_BLOCKS} paragraphs and other blocks."
            ));
        }
        let mut bytes = 0;
        for block in &self.blocks {
            match block {
                Block::Heading { level, .. } if !(1..=3).contains(level) => {
                    return Err("Headings must be level 1, 2 or 3.".into());
                }
                Block::BulletList { items } | Block::OrderedList { items } => {
                    if items.is_empty() {
                        return Err("Lists need at least one item.".into());
                    }
                    // Each item is at most one level deeper than the one before.
                    let mut previous = 0;
                    for (i, item) in items.iter().enumerate() {
                        if item.level > MAX_LIST_LEVEL || (i == 0 && item.level > 0) || item.level > previous + 1 {
                            return Err("List items can only nest one level at a time, up to six levels.".into());
                        }
                        previous = item.level;
                    }
                }
                Block::Table { rows, .. } => {
                    let cols = rows.first().map_or(0, Vec::len);
                    if rows.is_empty() || rows.len() > MAX_TABLE_ROWS || cols == 0 || cols > MAX_TABLE_COLS {
                        return Err(format!(
                            "Tables can have 1 to {MAX_TABLE_ROWS} rows and 1 to {MAX_TABLE_COLS} columns."
                        ));
                    }
                    if rows.iter().any(|row| row.len() != cols) {
                        return Err("Every row of a table needs the same number of cells.".into());
                    }
                }
                Block::Image { src, alt } => {
                    check_image_name(src)?;
                    check_text(alt)?;
                    if alt.len() > 1000 {
                        return Err("Image descriptions can be at most 1,000 characters.".into());
                    }
                }
                Block::Code { text } => {
                    check_text(text)?;
                    bytes += text.len();
                }
                _ => {}
            }
            for run in block.runs().into_iter().flatten() {
                check_text(&run.text)?;
                bytes += run.text.len();
                if let Some(link) = &run.link {
                    check_link(link)?;
                }
            }
        }
        if bytes > MAX_TEXT_BYTES {
            return Err("This document is too long. Split it into several documents.".into());
        }
        Ok(())
    }

    /// Merges neighbouring runs with the same marks and drops empty ones.
    pub fn normalize(&mut self) {
        for block in &mut self.blocks {
            for runs in block.runs_mut() {
                let mut merged: Vec<Run> = Vec::with_capacity(runs.len());
                for run in runs.drain(..) {
                    if run.text.is_empty() {
                        continue;
                    }
                    match merged.last_mut() {
                        Some(last) if last.same_marks(&run) => last.text.push_str(&run.text),
                        _ => merged.push(run),
                    }
                }
                *runs = merged;
            }
        }
    }

    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(Block::plain_text)
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn word_count(&self) -> usize {
        self.blocks
            .iter()
            .map(|b| b.plain_text().split_whitespace().count())
            .sum()
    }

    /// The opening text, on one line, for document lists.
    pub fn excerpt(&self, max_chars: usize) -> String {
        let text = self.plain_text();
        let mut words = text.split_whitespace();
        let mut out = String::new();
        for word in words.by_ref() {
            if out.chars().count() + word.chars().count() + 1 > max_chars {
                out.push('…');
                break;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(word);
        }
        out
    }
}

/// Uploaded image names: a random id and a known extension, such as
/// `0f8c…-4a1b.png`. Images live under the owner's own upload folder.
pub fn check_image_name(name: &str) -> Result<(), String> {
    let (id, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let id_ok = id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if !id_ok || !IMAGE_TYPES.iter().any(|(_, ext)| *ext == extension) {
        return Err("That image is not one of this document's uploads.".into());
    }
    Ok(())
}

fn check_text(text: &str) -> Result<(), String> {
    if text.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err("Text cannot contain control characters.".into());
    }
    Ok(())
}

/// Links may only be web or email addresses, so a document can never carry a
/// `javascript:` or `data:` URL.
pub fn check_link(link: &str) -> Result<(), String> {
    let lower = link.to_ascii_lowercase();
    let scheme_ok = ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme) && lower.len() > scheme.len());
    if !scheme_ok
        || link.len() > MAX_LINK_BYTES
        || link.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '<' || c == '>')
    {
        return Err(format!("“{}” is not a web or email link.", truncate(link, 60)));
    }
    Ok(())
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.into()
    } else {
        text.chars().take(max).chain(['…']).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Document {
        serde_json::from_value(serde_json::json!({"blocks": [
            {"type": "heading", "level": 1, "runs": [{"text": "Plan"}]},
            {"type": "paragraph", "align": "center", "runs": [
                {"text": "Read "}, {"text": "the docs", "bold": true, "link": "https://example.com"}
            ]},
            {"type": "bullet_list", "items": [[{"text": "one"}], [{"text": "two", "italic": true}]]},
            {"type": "code", "text": "fn main() {}"},
            {"type": "divider"}
        ]}))
        .unwrap()
    }

    #[test]
    fn round_trips_through_json() {
        let doc = sample();
        assert!(doc.validate().is_ok());
        let json = serde_json::to_value(&doc).unwrap();
        assert_eq!(serde_json::from_value::<Document>(json.clone()).unwrap(), doc);
        // Unset marks and left alignment stay out of the stored JSON.
        assert_eq!(json["blocks"][0]["runs"][0], serde_json::json!({"text": "Plan"}));
        assert_eq!(json["blocks"][1]["align"], "center");
    }

    #[test]
    fn rejects_unsafe_links() {
        for link in ["javascript:alert(1)", "data:text/html,x", "https://", "https://a b", "/relative"] {
            assert!(check_link(link).is_err(), "{link}");
        }
        assert!(check_link("MAILTO:someone@example.com").is_ok());
        let mut doc = sample();
        if let Block::Paragraph { runs, .. } = &mut doc.blocks[1] {
            runs[1].link = Some("JavaScript:alert(1)".into());
        }
        assert!(doc.validate().is_err());
    }

    #[test]
    fn rejects_bad_structure_and_size() {
        let heading: Document = serde_json::from_value(serde_json::json!({"blocks": [
            {"type": "heading", "level": 4, "runs": []}
        ]}))
        .unwrap();
        assert!(heading.validate().is_err());
        let control = Document {
            blocks: vec![Block::Paragraph { runs: vec![Run::plain("a\u{0}b")], align: Align::Left }],
        };
        assert!(control.validate().is_err());
        let long = Document {
            blocks: vec![Block::Code { text: "x".repeat(MAX_TEXT_BYTES + 1) }],
        };
        assert!(long.validate().is_err());
        let many = Document { blocks: vec![Block::Divider; MAX_BLOCKS + 1] };
        assert!(many.validate().is_err());
        assert!(serde_json::from_str::<Document>(r#"{"blocks":[{"type":"script"}]}"#).is_err());
    }

    #[test]
    fn normalizes_runs_and_summarizes() {
        let mut doc = Document {
            blocks: vec![Block::Paragraph {
                runs: vec![Run::plain("Hello "), Run::plain(""), Run::plain("there, friend")],
                align: Align::Left,
            }],
        };
        doc.normalize();
        assert_eq!(doc.blocks[0].runs()[0], [Run::plain("Hello there, friend")]);
        assert_eq!(doc.word_count(), 3);
        assert_eq!(doc.excerpt(12), "Hello there,…");
        assert_eq!(sample().word_count(), 9);
    }

    #[test]
    fn checks_lists_tables_and_images() {
        let check = |block: serde_json::Value| -> Result<(), String> {
            serde_json::from_value::<Document>(serde_json::json!({"blocks": [block]})).unwrap().validate()
        };
        // Lists saved before nesting still read.
        assert!(check(serde_json::json!({"type": "bullet_list", "items": [[{"text": "a"}]]})).is_ok());
        let nested = |levels: &[u8]| {
            let items: Vec<_> = levels.iter().map(|l| serde_json::json!({"runs": [{"text": "x"}], "level": l})).collect();
            check(serde_json::json!({"type": "ordered_list", "items": items}))
        };
        assert!(nested(&[0, 1, 2, 1, 0]).is_ok());
        assert!(nested(&[1]).is_err());
        assert!(nested(&[0, 2]).is_err());
        assert!(nested(&[0, 1, 2, 3, 4, 5, 6]).is_err());
        let row = |n: usize| serde_json::json!((0..n).map(|_| serde_json::json!([{"text": "c"}])).collect::<Vec<_>>());
        assert!(check(serde_json::json!({"type": "table", "rows": [row(2), row(2)]})).is_ok());
        assert!(check(serde_json::json!({"type": "table", "rows": [row(2), row(3)]})).is_err());
        assert!(check(serde_json::json!({"type": "table", "rows": [row(21)]})).is_err());
        assert!(check(serde_json::json!({"type": "table", "rows": []})).is_err());
        let image = |src: &str| check(serde_json::json!({"type": "image", "src": src}));
        assert!(image("0f8c2c4e-1b2a-4c3d-9e8f-001122334455.webp").is_ok());
        for bad in ["../x.png", "0f8c2c4e-1b2a-4c3d-9e8f-001122334455.svg", "x.png", "https://evil/x.png"] {
            assert!(image(bad).is_err(), "{bad}");
        }
    }
}
