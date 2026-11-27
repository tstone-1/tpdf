//! Inline formatting for PDF annotation comments. Ranges use UTF-16 offsets,
//! the selection units of the webview's editable text.
use serde::{Deserialize, Serialize};

const STRIKE: u8 = 8;
const SUBSCRIPT: u8 = 16;
const SUPERSCRIPT: u8 = 32;
const SCRIPTS: u8 = SUBSCRIPT | SUPERSCRIPT;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style(pub usize, pub usize, pub u8);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Body {
    Plain(String),
    Formatted { text: String, styles: Vec<Style> },
}

impl Default for Body {
    fn default() -> Self {
        Self::Plain(String::new())
    }
}
impl From<String> for Body {
    fn from(text: String) -> Self {
        Self::Plain(text)
    }
}
impl From<&str> for Body {
    fn from(text: &str) -> Self {
        Self::Plain(text.into())
    }
}
impl std::ops::Deref for Body {
    type Target = str;
    fn deref(&self) -> &str {
        match self {
            Self::Plain(text) | Self::Formatted { text, .. } => text,
        }
    }
}

impl Body {
    pub fn as_str(&self) -> &str {
        self
    }

    pub fn styles(&self) -> &[Style] {
        match self {
            Self::Plain(_) => &[],
            Self::Formatted { styles, .. } => styles,
        }
    }

    // Validate while converting offsets to UTF-8. Every character is visited
    // at most once, including on notes containing many short styled runs.
    fn segments(&self) -> Option<Vec<(&str, u8)>> {
        let mut rest: &str = self;
        let mut offset = 0;
        let mut parts = Vec::new();
        for &Style(start, end, flags) in self.styles() {
            if !(1..=63).contains(&flags)
                || flags & SCRIPTS == SCRIPTS
                || start < offset
                || end <= start
            {
                return None;
            }
            let (before, after) = split_utf16(rest, start - offset)?;
            let (styled, after) = split_utf16(after, end - start)?;
            parts.push((before, 0));
            parts.push((styled, flags));
            rest = after;
            offset = end;
        }
        parts.push((rest, 0));
        Some(parts)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.styles().is_empty()
            && self.chars().any(|c| {
                (c < ' ' && !matches!(c, '\t' | '\n' | '\r'))
                    || matches!(c, '\u{fffe}' | '\u{ffff}')
            })
        {
            return Err(said!(
                "this comment contains a character that cannot be formatted"
            ));
        }
        if self.styles().is_empty() || self.segments().is_some() {
            Ok(())
        } else {
            Err(said!(
                "the formatting selection is outside the comment or invalid"
            ))
        }
    }

    pub fn xhtml(&self) -> Option<String> {
        if self.styles().is_empty() {
            return None;
        }
        // ISO 32000-1 table 224 specifies the attributes of the XHTML root.
        let mut out = format!(
            r#"<body xmlns="http://www.w3.org/1999/xhtml" xmlns:xfa="http://www.xfa.org/schema/xfa-data/1.0/" xfa:contentType="text/html" xfa:APIVersion="tpdf:{}" xfa:spec="2.4"><p>"#,
            env!("CARGO_PKG_VERSION")
        );
        for (text, flags) in self.segments()? {
            if flags != 0 {
                out.push_str("<span style=\"");
                if flags & 1 != 0 {
                    out.push_str("font-weight:bold;");
                }
                if flags & 2 != 0 {
                    out.push_str("font-style:italic;");
                }
                if flags & SUBSCRIPT != 0 {
                    out.push_str("vertical-align:-3pt;font-size:8pt;");
                } else if flags & SUPERSCRIPT != 0 {
                    out.push_str("vertical-align:4pt;font-size:8pt;");
                }
                if flags & 4 != 0 {
                    out.push_str("text-decoration:underline;");
                } else if flags & STRIKE != 0 {
                    out.push_str("text-decoration:line-through;");
                }
                out.push_str("\">");
                // Table 225 specifies one decoration per value. Nest the
                // strike so an underline on the outer span still applies.
                if flags & (4 | STRIKE) == (4 | STRIKE) {
                    out.push_str(r#"<span style="text-decoration:line-through;">"#);
                }
            }
            out.push_str(&quick_xml::escape::escape(text).replace('\n', "<br/>"));
            if flags & (4 | STRIKE) == (4 | STRIKE) {
                out.push_str("</span>");
            }
            if flags != 0 {
                out.push_str("</span>");
            }
        }
        out.push_str("</p></body>");
        Some(out)
    }
}

fn split_utf16(text: &str, units: usize) -> Option<(&str, &str)> {
    let mut offset = 0;
    for (byte, ch) in text.char_indices() {
        if offset == units {
            return Some(text.split_at(byte));
        }
        offset += ch.len_utf16();
        if offset > units {
            return None;
        }
    }
    (offset == units).then(|| text.split_at(text.len()))
}

// ISO 32000-1, Table 225 uses signed point offsets for raised/lowered text.
fn baseline_style(value: &str) -> Option<u8> {
    match value {
        "sub" => Some(SUBSCRIPT),
        "super" => Some(SUPERSCRIPT),
        "baseline" => Some(0),
        _ => {
            let points: f64 = value.strip_suffix("pt")?.parse().ok()?;
            match points.partial_cmp(&0.0)? {
                std::cmp::Ordering::Less if points.is_finite() => Some(SUBSCRIPT),
                std::cmp::Ordering::Greater if points.is_finite() => Some(SUPERSCRIPT),
                std::cmp::Ordering::Equal => Some(0),
                _ => None,
            }
        }
    }
}

/// Read the supported subset as text and numeric styles. Only accept it if
/// the independently read Contents agrees, so stale rich text cannot replace
/// newer plain text. No XML or CSS crosses into the webview.
pub fn read_styles(xml: &str, plain: &str) -> Option<Vec<Style>> {
    parse_styles(xml, plain, false).or_else(|| parse_styles(xml, plain, true))
}

fn parse_styles(xml: &str, plain: &str, ignore_indentation: bool) -> Option<Vec<Style>> {
    use quick_xml::{events::Event, Reader};
    let mut reader = Reader::from_str(xml);
    let mut stack = Vec::new();
    let mut seen_paragraph = false;
    let mut text = String::new();
    let mut units = 0;
    let mut styles = Vec::new();
    loop {
        let flags = stack.last().map_or(0, |&(style, _)| style);
        let piece = match reader.read_event().ok()? {
            Event::Start(tag) => {
                if stack.len() >= 32 {
                    return None;
                }
                let mut style = flags;
                match tag.local_name().as_ref() {
                    "body" => (),
                    "p" => {
                        if seen_paragraph {
                            text.push('\n');
                            units += 1;
                        }
                        seen_paragraph = true;
                    }
                    "span" => (),
                    "b" | "strong" => style |= 1,
                    "i" | "em" => style |= 2,
                    "u" => style |= 4,
                    "s" | "strike" | "del" => style |= STRIKE,
                    "sub" => style = (style & !SCRIPTS) | SUBSCRIPT,
                    "sup" => style = (style & !SCRIPTS) | SUPERSCRIPT,
                    _ => return None,
                }
                for (index, attr) in tag.attributes().enumerate() {
                    if index >= 64 {
                        return None;
                    }
                    let attr = attr.ok()?;
                    if attr.key.as_ref() == "style" {
                        for rule in attr.value.split(';') {
                            if let Some((key, value)) = rule.split_once(':') {
                                match (key.trim(), value.trim()) {
                                    ("font-weight", "bold" | "700") => style |= 1,
                                    ("font-weight", "normal" | "400") => style &= !1,
                                    ("font-style", "italic") => style |= 2,
                                    ("font-style", "normal") => style &= !2,
                                    ("text-decoration", "underline") => style |= 4,
                                    ("text-decoration", "line-through") => style |= STRIKE,
                                    (
                                        "text-decoration",
                                        "underline line-through" | "line-through underline",
                                    ) => style |= 4 | STRIKE,
                                    ("text-decoration", "none") => style &= !(4 | STRIKE),
                                    ("vertical-align", value) => {
                                        if let Some(script) = baseline_style(value) {
                                            style = (style & !SCRIPTS) | script;
                                        }
                                    }
                                    _ => (),
                                }
                            }
                        }
                    }
                }
                stack.push((style, tag.local_name().as_ref() == "body"));
                continue;
            }
            Event::End(_) => {
                stack.pop()?;
                continue;
            }
            Event::Empty(tag) if tag.local_name().as_ref() == "br" => "\n".into(),
            Event::Text(chunk)
                if ignore_indentation
                    && stack.last().is_none_or(|&(_, body)| body)
                    && chunk
                        .bytes()
                        .all(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r')) =>
            {
                continue
            }
            Event::Text(chunk) => chunk.to_string(),
            Event::GeneralRef(entity) => quick_xml::escape::unescape(&format!("&{};", &*entity))
                .ok()?
                .into_owned(),
            Event::Decl(_) => continue,
            Event::Eof => break,
            _ => return None,
        };
        if text.len() + piece.len() > plain.len() {
            return None;
        }
        let start = units;
        units += piece.encode_utf16().count();
        text.push_str(&piece);
        if flags != 0 && units > start {
            if let Some(Style(_, end, previous)) = styles.last_mut() {
                if *previous == flags && *end == start {
                    *end = units;
                    continue;
                }
            }
            styles.push(Style(start, units, flags));
        }
    }
    (stack.is_empty() && text == plain).then_some(styles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_validates_offsets_and_rejects_stale_or_unsupported_markup() {
        for styles in [
            vec![Style(1, 2, 1)],
            vec![Style(2, 1, 1)],
            vec![Style(0, 9, 7)],
            vec![Style(0, 2, 64)],
            vec![Style(0, 2, SUBSCRIPT | SUPERSCRIPT)],
            vec![Style(0, 2, 1), Style(0, 2, 2)],
        ] {
            assert!(Body::Formatted {
                text: "𐐷A".into(),
                styles
            }
            .validate()
            .is_err());
        }
        let xml = "<body><p><b>A &amp; <i>𐐷</i></b></p><p>B</p></body>";
        assert_eq!(
            read_styles(xml, "A & 𐐷\nB"),
            Some(vec![Style(0, 4, 1), Style(4, 6, 3)])
        );
        let indented = "<?xml version=\"1.0\"?>\n<body>\n  <p><b> A </b></p>\n <p>B</p>\n</body>\n";
        assert_eq!(read_styles(indented, " A \nB"), Some(vec![Style(0, 3, 1)]));
        assert_eq!(read_styles(indented, "A\nB"), None);
        assert_eq!(read_styles(xml, "newer words"), None);
        assert_eq!(
            read_styles("<body><script>words</script></body>", "words"),
            None
        );
    }
}
