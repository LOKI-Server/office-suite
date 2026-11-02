//! The rich-text document model. The editor sends this structure, never HTML,
//! and the server validates it before storing it as JSON.
use serde::{Deserialize, Serialize};

pub const MAX_BLOCKS: usize = 5_000;
pub const MAX_TEXT_BYTES: usize = 512 * 1024;
pub const MAX_LINK_BYTES: usize = 2_048;

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
        items: Vec<Vec<Run>>,
    },
    OrderedList {
        items: Vec<Vec<Run>>,
    },
    Divider,
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
                items.iter().map(Vec::as_slice).collect()
            }
            Block::Code { .. } | Block::Divider => Vec::new(),
        }
    }
    fn runs_mut(&mut self) -> Vec<&mut Vec<Run>> {
        match self {
            Block::Paragraph { runs, .. } | Block::Heading { runs, .. } | Block::Quote { runs } => {
                vec![runs]
            }
            Block::BulletList { items } | Block::OrderedList { items } => items.iter_mut().collect(),
            Block::Code { .. } | Block::Divider => Vec::new(),
        }
    }
    /// The block's text with marks dropped; list items and lines are joined by `\n`.
    pub fn plain_text(&self) -> String {
        match self {
            Block::Code { text } => text.clone(),
            Block::Divider => String::new(),
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
                Block::BulletList { items } | Block::OrderedList { items } if items.is_empty() => {
                    return Err("Lists need at least one item.".into());
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
}
