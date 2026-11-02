use loki_sdk::Result;
use serde::Serialize;

/// Serializes `value` for a `<script type="application/json">` element.
///
/// `<`, `>` and `&` only ever occur inside JSON strings, where their `\u`
/// escapes decode to the same text, so the payload can never close the
/// script element or open a comment.
pub fn script_json(value: &impl Serialize) -> Result<String> {
    let json = serde_json::to_string(value)?;
    let mut out = String::with_capacity(json.len() + 16);
    for c in json.chars() {
        match c {
            '<' => out.push_str(r"\u003c"),
            '>' => out.push_str(r"\u003e"),
            '&' => out.push_str(r"\u0026"),
            '\u{2028}' => out.push_str(r"\u2028"),
            '\u{2029}' => out.push_str(r"\u2029"),
            c => out.push(c),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cannot_close_the_script_element() {
        let json = script_json(&serde_json::json!({"t": "</script><!-- & \u{2028}"})).unwrap();
        assert!(!json.contains('<') && !json.contains('>') && !json.contains('&'));
        let back: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back["t"], "</script><!-- & \u{2028}");
    }
}
