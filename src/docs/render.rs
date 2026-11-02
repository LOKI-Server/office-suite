//! Renders the document model as HTML (read-only view, HTML export) and Markdown.
use super::model::{Block, Document, Item, Run};
use std::fmt::Write;

/// The document body as an HTML fragment. All text and links are escaped.
/// Image file names are appended to `images`, such as `/docs/images/`.
pub fn to_html(doc: &Document, images: &str) -> String {
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
                list_html(tag, items, &mut out);
            }
            Block::Divider => out.push_str("<hr>\n"),
            Block::Table { rows, header } => {
                out.push_str("<table>\n");
                for (r, row) in rows.iter().enumerate() {
                    let cell = if *header && r == 0 { "th" } else { "td" };
                    out.push_str("<tr>");
                    for runs in row {
                        let _ = write!(out, "<{cell}>");
                        inline_html(runs, &mut out);
                        let _ = write!(out, "</{cell}>");
                    }
                    out.push_str("</tr>\n");
                }
                out.push_str("</table>\n");
            }
            Block::Image { src, alt } => {
                out.push_str("<figure class=\"image\"><img src=\"");
                escape(images, &mut out);
                escape(src, &mut out);
                out.push_str("\" alt=\"");
                escape(alt, &mut out);
                out.push_str("\"></figure>\n");
            }
        }
    }
    out
}

/// Nested lists from items with levels: a deeper item opens a list inside the
/// item before it.
fn list_html(tag: &str, items: &[Item], out: &mut String) {
    let _ = write!(out, "<{tag}>");
    let mut level = 0;
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            if item.level > level {
                let _ = write!(out, "<{tag}>");
            } else {
                out.push_str("</li>");
                for _ in item.level..level {
                    let _ = write!(out, "</{tag}></li>");
                }
            }
        }
        level = item.level;
        out.push_str("<li>");
        inline_html(&item.runs, out);
    }
    out.push_str("</li>");
    for _ in 0..level {
        let _ = write!(out, "</{tag}></li>");
    }
    let _ = writeln!(out, "</{tag}>");
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

/// A complete HTML file for export. Images link back to `images`.
pub fn to_html_file(title: &str, doc: &Document, images: &str) -> String {
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
    out.push_str(&to_html(doc, images));
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
table{border-collapse:collapse;margin:1em 0}th,td{border:1px solid #c4cad3;padding:.3em .6em;vertical-align:top}\
th{background:#f4f5f7;text-align:left}figure{margin:1em 0}img{max-width:100%}\
.align-center{text-align:center}.align-right{text-align:right}.align-justify{text-align:justify}\n";

/// The document as Markdown (CommonMark with GitHub tables), with the title as
/// its first heading. Images link back to `images`.
pub fn to_markdown(title: &str, doc: &Document, images: &str) -> String {
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
            Block::BulletList { items } | Block::OrderedList { items } => {
                let ordered = matches!(block, Block::OrderedList { .. });
                // Numbers restart in each nested list; nested items indent
                // under their parent's text.
                let mut numbers = [0usize; super::model::MAX_LIST_LEVEL as usize + 1];
                let mut indents = vec![0usize; super::model::MAX_LIST_LEVEL as usize + 2];
                for item in items {
                    let level = item.level as usize;
                    numbers[level] += 1;
                    for deeper in numbers.iter_mut().skip(level + 1) {
                        *deeper = 0;
                    }
                    let marker = if ordered { format!("{}. ", numbers[level]) } else { "- ".into() };
                    indents[level + 1] = indents[level] + marker.len();
                    let pad = " ".repeat(indents[level]);
                    let text = inline_md(&item.runs).replace('\n', &format!("\n{}", " ".repeat(indents[level + 1])));
                    let _ = writeln!(out, "{pad}{marker}{text}");
                }
            }
            Block::Divider => out.push_str("---\n"),
            Block::Table { rows, .. } => {
                for (r, row) in rows.iter().enumerate() {
                    let cells: Vec<String> = row
                        .iter()
                        .map(|runs| inline_md(runs).replace("  \n", "<br>"))
                        .collect();
                    let _ = writeln!(out, "| {} |", cells.join(" | "));
                    if r == 0 {
                        let _ = writeln!(out, "|{}", " --- |".repeat(row.len()));
                    }
                }
            }
            Block::Image { src, alt } => {
                let _ = writeln!(out, "![{}](<{images}{src}>)", md_escape(alt));
            }
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
        let html = to_html(&doc, "/i/");
        assert_eq!(
            html,
            "<p><a href=\"https://example.com/?a=1&amp;b=&quot;2&quot;\" rel=\"noopener noreferrer\">\
             <strong>&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;</strong></a></p>\n"
        );
        let file = to_html_file("<b>Title</b>", &doc, "/i/");
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
        ]})), "/i/");
        assert_eq!(
            html,
            "<h2 class=\"align-center\">Hi</h2>\n<p><em><u>a<br>b</u></em></p>\n<blockquote>q</blockquote>\n\
             <pre><code>x &lt; y</code></pre>\n<ol><li>one</li><li><s>two</s></li></ol>\n<hr>\n"
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
        ]})), "/i/");
        assert_eq!(
            md,
            "# My \\*notes\\*\n\n## Intro\n\nSay **hello** to [*docs*](<https://example.com>) and `` a`b ``\n\n\
             > line one  \n> line two\n\n- x\n- y\\_z\n\n1. first\n\n````\n```\nnested\n```\n````\n\n---\n"
        );
    }

    #[test]
    fn renders_nested_lists() {
        let doc = doc(serde_json::json!({"blocks": [
            {"type": "ordered_list", "items": [
                {"runs": [{"text": "a"}]},
                {"runs": [{"text": "a1"}], "level": 1},
                {"runs": [{"text": "a1x"}], "level": 2},
                {"runs": [{"text": "b"}]},
                {"runs": [{"text": "b1"}], "level": 1}
            ]}
        ]}));
        assert_eq!(
            to_html(&doc, "/i/"),
            "<ol><li>a<ol><li>a1<ol><li>a1x</li></ol></li></ol></li><li>b<ol><li>b1</li></ol></li></ol>\n"
        );
        assert_eq!(to_markdown("T", &doc, "/i/"), "# T\n\n1. a\n   1. a1\n      1. a1x\n2. b\n   1. b1\n");
    }

    #[test]
    fn renders_tables_and_images() {
        let doc = doc(serde_json::json!({"blocks": [
            {"type": "table", "header": true, "rows": [
                [[{"text": "Item"}], [{"text": "Cost", "bold": true}]],
                [[{"text": "Rent|tax"}], [{"text": "1200"}]]
            ]},
            {"type": "image", "src": "0f8c2c4e-1b2a-4c3d-9e8f-001122334455.png", "alt": "A \"chart\""}
        ]}));
        assert_eq!(
            to_html(&doc, "/i/"),
            "<table>\n<tr><th>Item</th><th><strong>Cost</strong></th></tr>\n<tr><td>Rent|tax</td><td>1200</td></tr>\n</table>\n\
             <figure class=\"image\"><img src=\"/i/0f8c2c4e-1b2a-4c3d-9e8f-001122334455.png\" alt=\"A &quot;chart&quot;\"></figure>\n"
        );
        assert_eq!(
            to_markdown("T", &doc, "https://x/i/"),
            "# T\n\n| Item | **Cost** |\n| --- | --- |\n| Rent\\|tax | 1200 |\n\n\
             ![A \"chart\"](<https://x/i/0f8c2c4e-1b2a-4c3d-9e8f-001122334455.png>)\n"
        );
    }
}
