//! Renders the document model as HTML (read-only view, HTML export) and Markdown.
use super::model::{Block, Document, Run};
use std::fmt::Write;

/// The document body as an HTML fragment. All text and links are escaped.
pub fn to_html(doc: &Document) -> String {
    let mut out = String::new();
    for block in &doc.blocks {
        match block {
            Block::Paragraph { runs, align } => {
                open("p", align.class(), &mut out);
                inline_html(runs, &mut out);
                out.push_str("</p>\n");
            }
            Block::Heading { level, runs, align } => {
                let tag = format!("h{level}");
                open(&tag, align.class(), &mut out);
                inline_html(runs, &mut out);
                let _ = writeln!(out, "</{tag}>");
            }
            Block::Quote { runs } => {
                out.push_str("<blockquote>");
                inline_html(runs, &mut out);
                out.push_str("</blockquote>\n");
            }
            Block::Code { text } => {
                out.push_str("<pre><code>");
                escape(text, &mut out);
                out.push_str("</code></pre>\n");
            }
            Block::BulletList { items } | Block::OrderedList { items } => {
                let tag = if matches!(block, Block::BulletList { .. }) { "ul" } else { "ol" };
                let _ = writeln!(out, "<{tag}>");
                for item in items {
                    out.push_str("<li>");
                    inline_html(item, &mut out);
                    out.push_str("</li>\n");
                }
                let _ = writeln!(out, "</{tag}>");
            }
            Block::Divider => out.push_str("<hr>\n"),
        }
    }
    out
}

fn open(tag: &str, class: Option<&str>, out: &mut String) {
    match class {
        Some(class) => {
            let _ = write!(out, "<{tag} class=\"{class}\">");
        }
        None => {
            let _ = write!(out, "<{tag}>");
        }
    }
}

fn inline_html(runs: &[Run], out: &mut String) {
    for run in runs {
        let mut close = Vec::new();
        if let Some(link) = &run.link {
            out.push_str("<a href=\"");
            escape(link, out);
            out.push_str("\" rel=\"noopener noreferrer\">");
            close.push("</a>");
        }
        for (on, open, end) in [
            (run.bold, "<strong>", "</strong>"),
            (run.italic, "<em>", "</em>"),
            (run.underline, "<u>", "</u>"),
            (run.strike, "<s>", "</s>"),
            (run.code, "<code>", "</code>"),
        ] {
            if on {
                out.push_str(open);
                close.push(end);
            }
        }
        for (i, line) in run.text.split('\n').enumerate() {
            if i > 0 {
                out.push_str("<br>");
            }
            escape(line, out);
        }
        for end in close.into_iter().rev() {
            out.push_str(end);
        }
    }
}

pub fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
}

/// A complete, self-contained HTML file for export.
pub fn to_html_file(title: &str, doc: &Document) -> String {
    let mut out = String::from(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n<title>",
    );
    escape(title, &mut out);
    out.push_str("</title>\n<style>\n");
    out.push_str(EXPORT_STYLE);
    out.push_str("</style>\n</head>\n<body>\n<h1 class=\"title\">");
    escape(title, &mut out);
    out.push_str("</h1>\n");
    out.push_str(&to_html(doc));
    out.push_str("</body>\n</html>\n");
    out
}

const EXPORT_STYLE: &str = "body{max-width:42rem;margin:3rem auto;padding:0 1rem;\
font:16px/1.6 Georgia,'Times New Roman',serif;color:#1d2129}\
h1,h2,h3{font-family:system-ui,sans-serif;line-height:1.25}\
.title{font-size:2rem}blockquote{margin:1em 0;padding-left:1em;border-left:3px solid #c4cad3;color:#5f6673}\
pre{background:#f4f5f7;padding:.75em 1em;border-radius:6px;overflow:auto}\
code{font-family:ui-monospace,Consolas,monospace;font-size:.9em}\
hr{border:0;border-top:1px solid #c4cad3;margin:2em 0}\
.align-center{text-align:center}.align-right{text-align:right}.align-justify{text-align:justify}\n";

/// The document as CommonMark, with the title as its first heading.
pub fn to_markdown(title: &str, doc: &Document) -> String {
    let mut out = format!("# {}\n", md_escape(title));
    for block in &doc.blocks {
        out.push('\n');
        match block {
            Block::Paragraph { runs, .. } => {
                out.push_str(&inline_md(runs));
                out.push('\n');
            }
            Block::Heading { level, runs, .. } => {
                // The title takes `#`, so document headings start one level down.
                let _ = writeln!(out, "{} {}", "#".repeat(*level as usize + 1), inline_md(runs).replace("  \n", " "));
            }
            Block::Quote { runs } => {
                for line in inline_md(runs).split('\n') {
                    let _ = writeln!(out, "> {line}");
                }
            }
            Block::Code { text } => {
                let fence = fence_for(text);
                let _ = writeln!(out, "{fence}\n{text}\n{fence}");
            }
            Block::BulletList { items } => {
                for item in items {
                    let _ = writeln!(out, "- {}", inline_md(item).replace('\n', "\n  "));
                }
            }
            Block::OrderedList { items } => {
                for (i, item) in items.iter().enumerate() {
                    let _ = writeln!(out, "{}. {}", i + 1, inline_md(item).replace('\n', "\n   "));
                }
            }
            Block::Divider => out.push_str("---\n"),
        }
    }
    out
}

fn inline_md(runs: &[Run]) -> String {
    let mut out = String::new();
    for run in runs {
        // Markdown cannot put marks around leading or trailing spaces.
        let trimmed = run.text.trim();
        if trimmed.is_empty() {
            out.push_str(&run.text);
            continue;
        }
        let start = run.text.len() - run.text.trim_start().len();
        let end = start + trimmed.len();
        out.push_str(&run.text[..start]);
        let mut text = if run.code {
            let ticks = if trimmed.contains('`') { "`` " } else { "`" };
            let close = if trimmed.contains('`') { " ``" } else { "`" };
            format!("{ticks}{trimmed}{close}")
        } else {
            md_escape(trimmed)
        };
        if run.strike {
            text = format!("~~{text}~~");
        }
        if run.italic {
            text = format!("*{text}*");
        }
        if run.bold {
            text = format!("**{text}**");
        }
        if run.underline {
            text = format!("<u>{text}</u>");
        }
        if let Some(link) = &run.link {
            text = format!("[{text}](<{link}>)");
        }
        out.push_str(&text.replace('\n', "  \n"));
        out.push_str(&run.text[end..]);
    }
    out
}

fn md_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '~' | '|') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn fence_for(text: &str) -> String {
    let mut longest = 0;
    let mut current = 0;
    for c in text.chars() {
        if c == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    "`".repeat((longest + 1).max(3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::model::Align;

    fn doc(value: serde_json::Value) -> Document {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn escapes_text_and_links() {
        let doc = Document {
            blocks: vec![Block::Paragraph {
                runs: vec![Run {
                    text: "<script>alert('x')</script>".into(),
                    bold: true,
                    link: Some("https://example.com/?a=1&b=\"2\"".into()),
                    ..Run::default()
                }],
                align: Align::Left,
            }],
        };
        let html = to_html(&doc);
        assert_eq!(
            html,
            "<p><a href=\"https://example.com/?a=1&amp;b=&quot;2&quot;\" rel=\"noopener noreferrer\">\
             <strong>&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;</strong></a></p>\n"
        );
        let file = to_html_file("<b>Title</b>", &doc);
        assert!(file.contains("<title>&lt;b&gt;Title&lt;/b&gt;</title>"));
        assert!(!file.contains("<script>"));
    }

    #[test]
    fn renders_every_block_as_html() {
        let html = to_html(&doc(serde_json::json!({"blocks": [
            {"type": "heading", "level": 2, "align": "center", "runs": [{"text": "Hi"}]},
            {"type": "paragraph", "runs": [{"text": "a\nb", "italic": true, "underline": true}]},
            {"type": "quote", "runs": [{"text": "q"}]},
            {"type": "code", "text": "x < y"},
            {"type": "ordered_list", "items": [[{"text": "one"}], [{"text": "two", "strike": true}]]},
            {"type": "divider"}
        ]})));
        assert_eq!(
            html,
            "<h2 class=\"align-center\">Hi</h2>\n<p><em><u>a<br>b</u></em></p>\n<blockquote>q</blockquote>\n\
             <pre><code>x &lt; y</code></pre>\n<ol>\n<li>one</li>\n<li><s>two</s></li>\n</ol>\n<hr>\n"
        );
    }

    #[test]
    fn renders_markdown() {
        let md = to_markdown("My *notes*", &doc(serde_json::json!({"blocks": [
            {"type": "heading", "level": 1, "runs": [{"text": "Intro"}]},
            {"type": "paragraph", "runs": [
                {"text": "Say "}, {"text": "hello ", "bold": true}, {"text": "to "},
                {"text": "docs", "italic": true, "link": "https://example.com"}, {"text": " and "},
                {"text": "a`b", "code": true}
            ]},
            {"type": "quote", "runs": [{"text": "line one\nline two"}]},
            {"type": "bullet_list", "items": [[{"text": "x"}], [{"text": "y_z"}]]},
            {"type": "ordered_list", "items": [[{"text": "first"}]]},
            {"type": "code", "text": "```\nnested\n```"},
            {"type": "divider"}
        ]})));
        assert_eq!(
            md,
            "# My \\*notes\\*\n\n## Intro\n\nSay **hello** to [*docs*](<https://example.com>) and `` a`b ``\n\n\
             > line one  \n> line two\n\n- x\n- y\\_z\n\n1. first\n\n````\n```\nnested\n```\n````\n\n---\n"
        );
    }
}
