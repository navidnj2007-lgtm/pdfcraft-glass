//! A PDF from an HTML file (Acrobat's Create PDF from a web page, for a page saved as a file).
//!
//! A tolerant reader of HTML's syntax (tags, attributes, character references, comments, raw
//! text) feeds a small layout engine: headings, paragraphs, line breaks, lists, block quotes,
//! preformatted text, rules, tables (columns sized to their content), links (web links and links
//! to `#anchors` in the page) and images embedded as `data:` URLs, in the standard PDF fonts
//! (Helvetica, Courier) with bold, italic, underline, strike-through, colour, size, alignment and
//! sub/superscript from the elements and their `style` attributes. Style sheets, scripts and
//! layout CSS (floats, grids, positioning) are not run: the page reads like the document without
//! its styling. Linked images and other files the HTML refers to are never fetched.
//!
//! Characters the standard fonts can't draw are written as `[U+XXXX]` code points, never as `?`
//! or dropped, and [`HtmlReport`] counts them, with the images left out.
//!
//! Written from the HTML syntax as published (WHATWG HTML §13) and browsers' default styles as
//! observed; no other implementation was consulted.

use std::collections::HashMap;

use pdfcraft_cos::{Dict, Document, ObjRef, Object, PdfString};
use pdfcraft_fonts::{Std14, literal, win_ansi, win_ansi_byte};

use crate::{CreateError, add_page, image_xobject, set_title};

/// Text of a larger file is cut here (32 MiB): a web page is far smaller.
const MAX_TEXT: usize = 32 << 20;
/// Pages a conversion may write.
const MAX_PAGES: usize = 5_000;
/// Elements open at once; deeper ones are read as text of their parent.
const MAX_DEPTH: usize = 256;
/// Table columns laid out; cells past them go into the last column.
const MAX_COLUMNS: usize = 32;
/// Images embedded; later ones show their alternative text.
const MAX_IMAGES: usize = 500;
/// The largest `data:` image read (decoded bytes).
const MAX_IMAGE_BYTES: usize = 24 << 20;
/// Page margins, points (three quarters of an inch).
const MARGIN: f64 = 54.0;
/// The body text size, points (a browser's 16 px).
const BASE_SIZE: f64 = 12.0;
/// One CSS pixel in points.
const PX: f64 = 0.75;
/// Padding inside table cells, points.
const CELL_PAD: f64 = 4.0;

/// What a conversion couldn't carry over.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HtmlReport {
    /// Characters the standard fonts can't draw, written as `[U+XXXX]`.
    pub escaped: usize,
    /// Images not embedded: linked files (never fetched), unreadable data, or past the limit.
    pub images_skipped: usize,
}

/// Whether `bytes` named `name` is an HTML file: an `.html`/`.htm`/`.xhtml` name, or a document
/// that starts with a doctype or `<html>`.
pub fn is_html(name: &str, bytes: &[u8]) -> bool {
    let lower = name.to_ascii_lowercase();
    if [".html", ".htm", ".xhtml", ".shtml"].iter().any(|e| lower.ends_with(e)) {
        return true;
    }
    let head = bytes.get(..bytes.len().min(512)).unwrap_or_default();
    let head = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
    let start = head.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(head.len());
    let head: Vec<u8> = head.get(start..).unwrap_or_default().iter().take(15).map(u8::to_ascii_lowercase).collect();
    head.starts_with(b"<!doctype html") || head.starts_with(b"<html")
}

/// The text of an HTML file: a byte order mark, else a `<meta charset>` (or `http-equiv`
/// content type) near the start, picks the encoding; otherwise UTF-8 when the bytes are valid
/// UTF-8, and Windows-1252 when they aren't.
pub fn decode_html(bytes: &[u8]) -> String {
    if let Some((encoding, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        return encoding.decode_without_bom_handling(bytes.get(bom..).unwrap_or_default()).0.into_owned();
    }
    let head: Vec<u8> = bytes.iter().take(2048).map(u8::to_ascii_lowercase).collect();
    if let Some(label) = meta_charset(&head)
        && let Some(encoding) = encoding_rs::Encoding::for_label(label)
    {
        // A page that says UTF-16 in ASCII can't be UTF-16 (HTML §13.2.3.1).
        let encoding = if encoding == encoding_rs::UTF_16LE || encoding == encoding_rs::UTF_16BE { encoding_rs::UTF_8 } else { encoding };
        return encoding.decode_without_bom_handling(bytes).0.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes).0.into_owned(),
    }
}

/// The charset label in `<meta charset=…>` or `content="…; charset=…"` in `head` (lowercase).
fn meta_charset(head: &[u8]) -> Option<&[u8]> {
    let at = head.windows(8).position(|w| w == b"charset=")?;
    let rest = head.get(at + 8..)?;
    let rest = rest.strip_prefix(b"\"").or_else(|| rest.strip_prefix(b"'")).unwrap_or(rest);
    let end = rest.iter().position(|b| matches!(b, b'"' | b'\'' | b';' | b'>' | b' ' | b'/')).unwrap_or(rest.len());
    rest.get(..end).filter(|l| !l.is_empty())
}

/// Convert an HTML file to a PDF with pages of `page` size (points). `name` gives the title when
/// the page has no `<title>`.
pub fn from_html(name: &str, bytes: &[u8], page: (f64, f64)) -> Result<(Document, HtmlReport), CreateError> {
    let (w, h) = page;
    if !(w.is_finite() && h.is_finite() && (3.0 * MARGIN..=14_400.0).contains(&w) && (3.0 * MARGIN..=14_400.0).contains(&h)) {
        return Err(CreateError::Invalid("invalid page size".into()));
    }
    let mut text = decode_html(bytes);
    if text.len() > MAX_TEXT {
        let mut end = MAX_TEXT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    let mut doc = Document::new_empty();
    let mut builder = Builder::new(&mut doc);
    for token in Tokens::new(&text) {
        builder.token(token);
    }
    let built = builder.finish();
    let title = built.title.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| name.rsplit_once('.').map_or(name, |(s, _)| s).to_string());
    let report = built.report.clone();
    Writer::new(&mut doc, w, h).write(built)?;
    set_title(&mut doc, &title);
    Ok((doc, report))
}

// ── tokens ──────────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Text(String),
    Start { name: String, attrs: Vec<(String, String)>, self_closing: bool },
    End(String),
}

/// Elements whose content is raw text up to their end tag: dropped, or (title, textarea) read
/// as text with character references.
fn raw_text(name: &str) -> Option<bool> {
    match name {
        "script" | "style" | "xmp" | "iframe" | "noembed" | "noframes" | "plaintext" => Some(false),
        "title" | "textarea" => Some(true),
        _ => None,
    }
}

/// HTML's tokens, tolerant of anything: an unclosed tag or comment ends the input.
struct Tokens<'a> {
    rest: &'a str,
    queue: std::collections::VecDeque<Token>,
}

impl<'a> Tokens<'a> {
    fn new(text: &'a str) -> Self {
        Self { rest: text, queue: Default::default() }
    }

    /// Skip past `end` (or to the end of the input).
    fn skip_past(&mut self, end: &str) {
        self.rest = match self.rest.find(end) {
            Some(at) => self.rest.get(at + end.len()..).unwrap_or_default(),
            None => "",
        };
    }

    fn tag(&mut self) -> Option<Token> {
        // `rest` starts with '<' and an ASCII letter, or "</".
        let closing = self.rest.starts_with("</");
        let body = self.rest.get(if closing { 2 } else { 1 }..).unwrap_or_default();
        let name_len = body.find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>').unwrap_or(body.len());
        let name = body.get(..name_len).unwrap_or_default().to_ascii_lowercase();
        let mut rest = body.get(name_len..).unwrap_or_default();
        if closing {
            self.rest = match rest.find('>') {
                Some(at) => rest.get(at + 1..).unwrap_or_default(),
                None => "",
            };
            return Some(Token::End(name));
        }
        let mut attrs = Vec::new();
        let mut self_closing = false;
        loop {
            rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace());
            if rest.is_empty() {
                break;
            }
            if let Some(after) = rest.strip_prefix("/>") {
                self_closing = true;
                rest = after;
                break;
            }
            if let Some(after) = rest.strip_prefix('>') {
                rest = after;
                break;
            }
            if let Some(after) = rest.strip_prefix('/') {
                rest = after;
                continue;
            }
            let len = rest.find(|c: char| c.is_ascii_whitespace() || c == '=' || c == '>' || c == '/').unwrap_or(rest.len()).max(1);
            let len = (1..=len).rev().find(|&l| rest.is_char_boundary(l)).unwrap_or(rest.len());
            let attr = rest.get(..len).unwrap_or_default().to_ascii_lowercase();
            rest = rest.get(len..).unwrap_or_default().trim_start_matches(|c: char| c.is_ascii_whitespace());
            let mut value = String::new();
            if let Some(after) = rest.strip_prefix('=') {
                let after = after.trim_start_matches(|c: char| c.is_ascii_whitespace());
                if let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') {
                    let inner = after.get(1..).unwrap_or_default();
                    let end = inner.find(quote).unwrap_or(inner.len());
                    value = decode_entities(inner.get(..end).unwrap_or_default());
                    rest = inner.get(end + 1..).unwrap_or_default();
                } else {
                    let end = after.find(|c: char| c.is_ascii_whitespace() || c == '>').unwrap_or(after.len());
                    value = decode_entities(after.get(..end).unwrap_or_default());
                    rest = after.get(end..).unwrap_or_default();
                }
            }
            if attrs.len() < 64 && !attr.is_empty() && !attrs.iter().any(|(a, _): &(String, String)| *a == attr) {
                attrs.push((attr, value));
            }
        }
        self.rest = rest;
        if let Some(keep) = raw_text(&name)
            && !self_closing
        {
            // Raw text runs to the matching end tag, whatever it holds.
            let lower = self.rest.to_ascii_lowercase();
            let end = lower.find(&format!("</{name}")).unwrap_or(lower.len());
            if keep {
                let content = decode_entities(self.rest.get(..end).unwrap_or_default());
                self.queue.push_back(Token::Text(content));
            }
            self.queue.push_back(Token::End(name.clone()));
            self.rest = self.rest.get(end..).unwrap_or_default();
            self.skip_past(">");
        }
        Some(Token::Start { name, attrs, self_closing })
    }
}

impl Iterator for Tokens<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        loop {
            if let Some(t) = self.queue.pop_front() {
                return Some(t);
            }
            if self.rest.is_empty() {
                return None;
            }
            let Some(at) = self.rest.find('<') else {
                let text = decode_entities(self.rest);
                self.rest = "";
                return Some(Token::Text(text));
            };
            if at > 0 {
                let text = decode_entities(self.rest.get(..at).unwrap_or_default());
                self.rest = self.rest.get(at..).unwrap_or_default();
                return Some(Token::Text(text));
            }
            if self.rest.starts_with("<!--") {
                self.rest = self.rest.get(4..).unwrap_or_default();
                self.skip_past("-->");
                continue;
            }
            if self.rest.starts_with("<![CDATA[") {
                let inner = self.rest.get(9..).unwrap_or_default();
                let end = inner.find("]]>").unwrap_or(inner.len());
                let text = inner.get(..end).unwrap_or_default().to_string();
                self.rest = inner.get(end..).unwrap_or_default();
                self.skip_past("]]>");
                return Some(Token::Text(text));
            }
            if self.rest.starts_with("<!") || self.rest.starts_with("<?") {
                self.skip_past(">");
                continue;
            }
            let next = self.rest.chars().nth(1);
            let tag = next.is_some_and(|c| c.is_ascii_alphabetic())
                || (next == Some('/') && self.rest.chars().nth(2).is_some_and(|c| c.is_ascii_alphabetic()));
            if tag {
                if let Some(t) = self.tag() {
                    return Some(t);
                }
                continue;
            }
            if next == Some('/') {
                // `</>` or `</ …>`: a bogus comment.
                self.skip_past(">");
                continue;
            }
            // A '<' that starts no tag is text.
            self.rest = self.rest.get(1..).unwrap_or_default();
            return Some(Token::Text("<".into()));
        }
    }
}

/// Latin-1's named characters, U+00A0 to U+00FF in order.
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf", "laquo", "not", "shy", "reg", "macr", "deg",
    "plusmn", "sup2", "sup3", "acute", "micro", "para", "middot", "cedil", "sup1", "ordm", "raquo", "frac14", "frac12", "frac34", "iquest", "Agrave",
    "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig", "Ccedil", "Egrave", "Eacute", "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH",
    "Ntilde", "Ograve", "Oacute", "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute", "THORN", "szlig",
    "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil", "egrave", "eacute", "ecirc", "euml", "igrave", "iacute", "icirc",
    "iuml", "eth", "ntilde", "ograve", "oacute", "ocirc", "otilde", "ouml", "divide", "oslash", "ugrave", "uacute", "ucirc", "uuml", "yacute",
    "thorn", "yuml",
];

/// Other named characters pages commonly use.
const NAMED: &[(&str, char)] = &[
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("ndash", '\u{2013}'),
    ("mdash", '\u{2014}'),
    ("lsquo", '\u{2018}'),
    ("rsquo", '\u{2019}'),
    ("sbquo", '\u{201A}'),
    ("ldquo", '\u{201C}'),
    ("rdquo", '\u{201D}'),
    ("bdquo", '\u{201E}'),
    ("dagger", '\u{2020}'),
    ("Dagger", '\u{2021}'),
    ("bull", '\u{2022}'),
    ("hellip", '\u{2026}'),
    ("permil", '\u{2030}'),
    ("prime", '\u{2032}'),
    ("Prime", '\u{2033}'),
    ("lsaquo", '\u{2039}'),
    ("rsaquo", '\u{203A}'),
    ("euro", '\u{20AC}'),
    ("trade", '\u{2122}'),
    ("larr", '\u{2190}'),
    ("uarr", '\u{2191}'),
    ("rarr", '\u{2192}'),
    ("darr", '\u{2193}'),
    ("harr", '\u{2194}'),
    ("minus", '\u{2212}'),
    ("le", '\u{2264}'),
    ("ge", '\u{2265}'),
    ("ne", '\u{2260}'),
    ("asymp", '\u{2248}'),
    ("infin", '\u{221E}'),
    ("ensp", '\u{2002}'),
    ("emsp", '\u{2003}'),
    ("thinsp", '\u{2009}'),
    ("zwnj", '\u{200C}'),
    ("zwj", '\u{200D}'),
    ("lrm", '\u{200E}'),
    ("rlm", '\u{200F}'),
    ("OElig", '\u{152}'),
    ("oelig", '\u{153}'),
    ("Scaron", '\u{160}'),
    ("scaron", '\u{161}'),
    ("Yuml", '\u{178}'),
    ("fnof", '\u{192}'),
    ("circ", '\u{2C6}'),
    ("tilde", '\u{2DC}'),
    ("check", '\u{2713}'),
];

/// Named references that old pages write without the semicolon, which browsers accept.
const LEGACY: [&str; 7] = ["amp", "lt", "gt", "quot", "nbsp", "copy", "reg"];

fn named(name: &str) -> Option<char> {
    if let Some(i) = LATIN1.iter().position(|n| *n == name) {
        return char::from_u32(0xA0 + u32::try_from(i).ok()?);
    }
    NAMED.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// A numeric reference's character, as browsers read it: C1 codes as Windows-1252, nothing as
/// U+FFFD.
fn numeric(code: u32) -> char {
    match code {
        0x80..=0x9F => {
            encoding_rs::WINDOWS_1252.decode_without_bom_handling(&[u8::try_from(code).unwrap_or(b'?')]).0.chars().next().unwrap_or('\u{FFFD}')
        }
        0 => '\u{FFFD}',
        _ => char::from_u32(code).unwrap_or('\u{FFFD}'),
    }
}

/// Replace character references (`&amp;`, `&#233;`, `&#x2014;`) in `s`.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).unwrap_or_default());
        let after = rest.get(at + 1..).unwrap_or_default();
        if let Some(num) = after.strip_prefix('#') {
            let (hex, digits) = match num.strip_prefix(['x', 'X']) {
                Some(h) => (true, h),
                None => (false, num),
            };
            let len = digits.find(|c: char| if hex { !c.is_ascii_hexdigit() } else { !c.is_ascii_digit() }).unwrap_or(digits.len());
            if len > 0 {
                let code = digits.get(..len.min(8)).and_then(|d| u32::from_str_radix(d, if hex { 16 } else { 10 }).ok()).unwrap_or(0xFFFD);
                out.push(numeric(if len > 8 { 0xFFFD } else { code }));
                let tail = digits.get(len..).unwrap_or_default();
                rest = tail.strip_prefix(';').unwrap_or(tail);
                continue;
            }
        } else {
            let len = after.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(after.len());
            let name = after.get(..len).unwrap_or_default();
            let tail = after.get(len..).unwrap_or_default();
            if let Some(c) = named(name).filter(|_| tail.starts_with(';') || LEGACY.contains(&name)) {
                out.push(c);
                rest = tail.strip_prefix(';').unwrap_or(tail);
                continue;
            }
        }
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    out
}

// ── styles ──────────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Sans,
    Serif,
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align {
    Left,
    Center,
    Right,
}

/// How text is set: inherited down the open elements.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Style {
    family: Family,
    bold: bool,
    italic: bool,
    size: f64,
    color: [u8; 3],
    underline: bool,
    strike: bool,
    /// Index into the page's links.
    link: Option<u32>,
    /// Superscript (positive) or subscript (negative) shift, points.
    raise: f64,
    pre: bool,
    hidden: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            family: Family::Sans,
            bold: false,
            italic: false,
            size: BASE_SIZE,
            color: [0, 0, 0],
            underline: false,
            strike: false,
            link: None,
            raise: 0.0,
            pre: false,
            hidden: false,
        }
    }
}

impl Style {
    fn face(&self) -> Std14 {
        // Two bools index a four-element array, so the subscript is provably 0..=3.
        let pick = |faces: [Std14; 4]| faces[usize::from(self.bold) | usize::from(self.italic) << 1];
        match self.family {
            Family::Sans => pick([Std14::Helvetica, Std14::HelveticaBold, Std14::HelveticaOblique, Std14::HelveticaBoldOblique]),
            Family::Serif => pick([Std14::TimesRoman, Std14::TimesBold, Std14::TimesItalic, Std14::TimesBoldItalic]),
            Family::Mono => pick([Std14::Courier, Std14::CourierBold, Std14::CourierOblique, Std14::CourierBoldOblique]),
        }
    }

    fn set_size(&mut self, size: f64) {
        if size.is_finite() {
            self.size = size.clamp(4.0, 96.0);
        }
    }
}

/// A CSS colour: `#rgb`, `#rrggbb`, `rgb(r, g, b)` or a basic colour name.
fn parse_color(v: &str) -> Option<[u8; 3]> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(hex) = v.strip_prefix('#') {
        let digits: Vec<u8> = hex.chars().map_while(|c| c.to_digit(16).and_then(|d| u8::try_from(d).ok())).collect();
        return match digits.as_slice() {
            [r, g, b] | [r, g, b, _] => Some([r * 17, g * 17, b * 17]),
            [r1, r2, g1, g2, b1, b2, ..] => Some([r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2]),
            _ => None,
        };
    }
    if let Some(args) = v.strip_prefix("rgb(").or_else(|| v.strip_prefix("rgba(")) {
        let parts: Vec<u8> = args
            .trim_end_matches(')')
            .split([',', ' ', '/'])
            .filter(|p| !p.is_empty())
            .take(3)
            .filter_map(|p| p.trim().parse::<f64>().ok().filter(|f| f.is_finite()).map(|f| f.clamp(0.0, 255.0).round() as u8))
            .collect();
        return <[u8; 3]>::try_from(parts.as_slice()).ok();
    }
    Some(match v.as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "gray" | "grey" => [128, 128, 128],
        "silver" => [192, 192, 192],
        "maroon" => [128, 0, 0],
        "navy" => [0, 0, 128],
        "purple" => [128, 0, 128],
        "teal" => [0, 128, 128],
        "olive" => [128, 128, 0],
        "orange" => [255, 165, 0],
        "darkred" => [139, 0, 0],
        "darkgreen" => [0, 100, 0],
        "darkblue" => [0, 0, 139],
        _ => return None,
    })
}

/// A CSS length as a font size relative to `parent` (points).
fn parse_size(v: &str, parent: f64) -> Option<f64> {
    let v = v.trim().to_ascii_lowercase();
    let keyword = match v.as_str() {
        "xx-small" => Some(7.0),
        "x-small" => Some(7.5),
        "small" => Some(10.0),
        "medium" => Some(12.0),
        "large" => Some(13.5),
        "x-large" => Some(18.0),
        "xx-large" => Some(24.0),
        "smaller" => Some(parent * 0.83),
        "larger" => Some(parent * 1.2),
        _ => None,
    };
    if keyword.is_some() {
        return keyword;
    }
    let num_len = v.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(v.len());
    let n: f64 = v.get(..num_len)?.parse().ok().filter(|n: &f64| n.is_finite())?;
    match v.get(num_len..)?.trim() {
        "px" | "" => Some(n * PX),
        "pt" => Some(n),
        "em" | "rem" => Some(n * parent),
        "%" => Some(n / 100.0 * parent),
        _ => None,
    }
}

/// What an element's `style` attribute and presentational attributes change.
fn apply_style_attrs(style: &mut Style, block: &mut BlockCtx, attrs: &[(String, String)]) {
    for (name, value) in attrs {
        match name.as_str() {
            "hidden" => style.hidden = true,
            "align" => block.align = parse_align(value).unwrap_or(block.align),
            "color" => style.color = parse_color(value).unwrap_or(style.color),
            "style" => {
                for decl in value.split(';') {
                    let Some((prop, v)) = decl.split_once(':') else { continue };
                    let (prop, v) = (prop.trim().to_ascii_lowercase(), v.trim().to_ascii_lowercase());
                    let v = v.trim_end_matches("!important").trim();
                    match prop.as_str() {
                        "display" if v == "none" => style.hidden = true,
                        "visibility" if v == "hidden" || v == "collapse" => style.hidden = true,
                        "font-weight" => {
                            style.bold = matches!(v, "bold" | "bolder" | "600" | "700" | "800" | "900")
                                || (style.bold && v != "normal" && v != "400" && v != "lighter")
                        }
                        "font-style" => style.italic = v == "italic" || v == "oblique",
                        "text-align" => block.align = parse_align(v).unwrap_or(block.align),
                        "color" => style.color = parse_color(v).unwrap_or(style.color),
                        "font-size" => {
                            if let Some(size) = parse_size(v, style.size) {
                                style.set_size(size);
                            }
                        }
                        "font-family" => style.family = parse_family(v).unwrap_or(style.family),
                        "text-decoration" | "text-decoration-line" => {
                            if v.contains("none") {
                                style.underline = false;
                                style.strike = false;
                            }
                            style.underline |= v.contains("underline");
                            style.strike |= v.contains("line-through");
                        }
                        "white-space" => style.pre = v.starts_with("pre"),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

fn parse_align(v: &str) -> Option<Align> {
    match v.trim().to_ascii_lowercase().as_str() {
        "left" | "start" | "justify" => Some(Align::Left),
        "center" | "middle" => Some(Align::Center),
        "right" | "end" => Some(Align::Right),
        _ => None,
    }
}

fn parse_family(v: &str) -> Option<Family> {
    // The first family the standard fonts can stand in for.
    v.split(',').find_map(|f| {
        let f = f.trim().trim_matches(['"', '\'']).to_ascii_lowercase();
        if f.contains("mono") || f.contains("courier") || f.contains("consol") || f.contains("menlo") {
            Some(Family::Mono)
        } else if f.contains("sans") || f.contains("arial") || f.contains("helvetica") || f.contains("verdana") || f.contains("system-ui") {
            Some(Family::Sans)
        } else if f.contains("serif") || f.contains("times") || f.contains("georgia") || f.contains("garamond") {
            Some(Family::Serif)
        } else {
            None
        }
    })
}

/// Characters written as the standard fonts can draw them: WinAnsi as they are, a few others as
/// their plain equivalents, invisible ones dropped, and the rest as `[U+XXXX]`.
fn drawable(s: &str, report: &mut HtmlReport) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{AD}' | '\u{200B}'..='\u{200F}' | '\u{2060}' | '\u{FEFF}' | '\u{202A}'..='\u{202E}' => {}
            '\u{2002}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => out.push(' '),
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2212}' => out.push('-'),
            '\u{2032}' => out.push('\''),
            '\u{2033}' => out.push('"'),
            c if c == '\n' || c == '\t' || win_ansi_byte(c).is_some() => out.push(c),
            c => {
                report.escaped = report.escaped.saturating_add(1);
                out.push_str(&format!("[U+{:04X}]", u32::from(c)));
            }
        }
    }
    out
}

// ── blocks ──────────────────────────────────────────────────────────────────────────────────

/// One piece of a paragraph's inline content.
#[derive(Clone, Debug, PartialEq)]
enum Item {
    /// A word (or, in preformatted text, a line's run of text), with a space before it.
    Word { text: String, style: Style, space: bool },
    /// An embedded image, `index` into the document's images, in points.
    Image { index: usize, width: f64, height: f64, link: Option<u32>, space: bool },
    /// A line break.
    Break,
    /// An `id` (or `<a name>`) the next line carries, for links to `#id`.
    Anchor(String),
}

/// A paragraph's indents and alignment.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BlockCtx {
    left: f64,
    right: f64,
    align: Align,
}

impl Default for BlockCtx {
    fn default() -> Self {
        BlockCtx { left: 0.0, right: 0.0, align: Align::Left }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Cell {
    items: Vec<Item>,
    header: bool,
    span: usize,
    align: Align,
}

#[derive(Clone, Debug, PartialEq)]
enum Block {
    Para { items: Vec<Item>, ctx: BlockCtx, marker: Option<(String, Style)>, space: f64 },
    Rule { ctx: BlockCtx, space: f64 },
    Table { rows: Vec<Vec<Cell>>, ctx: BlockCtx, border: bool, space: f64 },
}

/// An element still open, with what it set.
struct Open {
    name: String,
    style: Style,
    ctx: BlockCtx,
}

struct ListCtx {
    ordered: bool,
    next: i64,
}

#[derive(Default)]
struct TableCtx {
    rows: Vec<Vec<Cell>>,
    row: Option<Vec<Cell>>,
    cell: Option<Cell>,
    caption: Vec<Item>,
    in_caption: bool,
    border: bool,
    ctx: BlockCtx,
}

/// What the reader collected, for the writer.
struct Built {
    blocks: Vec<Block>,
    links: Vec<String>,
    images: Vec<ObjRef>,
    title: Option<String>,
    report: HtmlReport,
}

/// Elements that start and end a block, with the space around them (points; zero inside a list
/// item or a table cell).
fn block_space(name: &str) -> Option<f64> {
    Some(match name {
        "p" | "ul" | "ol" | "dl" | "blockquote" | "pre" | "figure" | "menu" | "dir" | "listing" => BASE_SIZE,
        "h1" => 16.0,
        "h2" => 15.0,
        "h3" => 14.0,
        "h4" => 16.0,
        "h5" => 17.0,
        "h6" => 19.0,
        "table" | "hr" | "fieldset" | "form" => 6.0,
        "div" | "section" | "article" | "header" | "footer" | "main" | "nav" | "aside" | "li" | "dt" | "dd" | "address" | "center" | "details"
        | "summary" | "figcaption" | "caption" | "hgroup" | "legend" | "body" | "html" | "tr" | "td" | "th" | "textarea" => 0.0,
        _ => return None,
    })
}

/// Elements whose content is never shown.
fn hidden_element(name: &str) -> bool {
    matches!(
        name,
        "head"
            | "script"
            | "style"
            | "template"
            | "noscript"
            | "svg"
            | "math"
            | "select"
            | "datalist"
            | "object"
            | "iframe"
            | "video"
            | "audio"
            | "canvas"
            | "map"
            | "noembed"
            | "noframes"
            | "xmp"
            | "dialog"
    )
}

/// Elements without content or an end tag.
fn void_element(name: &str) -> bool {
    matches!(
        name,
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "param" | "source" | "track" | "wbr" | "keygen"
    )
}

struct Builder<'d> {
    doc: &'d mut Document,
    blocks: Vec<Block>,
    items: Vec<Item>,
    /// The paragraph being collected started with these indents and this list marker.
    para: Option<(BlockCtx, Option<(String, Style)>)>,
    marker: Option<(String, Style)>,
    stack: Vec<Open>,
    lists: Vec<ListCtx>,
    tables: Vec<TableCtx>,
    /// Tables inside a table cell, read as text of the cell.
    nested_tables: usize,
    space: bool,
    pending_space: f64,
    links: Vec<String>,
    images: Vec<ObjRef>,
    title: Option<String>,
    in_title: bool,
    /// The next text starts a `<pre>` (whose first newline is dropped).
    pre_start: bool,
    report: HtmlReport,
}

impl<'d> Builder<'d> {
    fn new(doc: &'d mut Document) -> Self {
        Builder {
            doc,
            blocks: Vec::new(),
            items: Vec::new(),
            para: None,
            marker: None,
            stack: Vec::new(),
            lists: Vec::new(),
            tables: Vec::new(),
            nested_tables: 0,
            space: false,
            pending_space: 0.0,
            links: Vec::new(),
            images: Vec::new(),
            title: None,
            in_title: false,
            pre_start: false,
            report: HtmlReport::default(),
        }
    }

    fn style(&self) -> Style {
        self.stack.last().map(|o| o.style).unwrap_or_default()
    }

    fn ctx(&self) -> BlockCtx {
        self.stack.last().map(|o| o.ctx).unwrap_or_default()
    }

    fn in_cell(&self) -> bool {
        self.tables.last().is_some_and(|t| t.cell.is_some() || t.in_caption)
    }

    /// Where inline content goes now: the open table cell (or caption), else the paragraph.
    fn sink(&mut self) -> &mut Vec<Item> {
        let in_cell = self.in_cell();
        if !in_cell && self.para.is_none() {
            let ctx = self.ctx();
            self.para = Some((ctx, self.marker.take()));
        }
        if in_cell && let Some(t) = self.tables.last_mut() {
            if t.in_caption {
                return &mut t.caption;
            }
            if let Some(cell) = t.cell.as_mut() {
                return &mut cell.items;
            }
        }
        &mut self.items
    }

    fn push_item(&mut self, item: Item) {
        self.sink().push(item);
    }

    /// End the paragraph (or, in a table cell, the line) and leave `space` before what follows.
    fn block_break(&mut self, space: f64) {
        self.space = false;
        if self.in_cell() {
            let items = self.sink();
            if items.last().is_some_and(|i| *i != Item::Break) && !items.is_empty() {
                items.push(Item::Break);
            }
            return;
        }
        let space = if self.lists.is_empty() { space } else { space.min(BASE_SIZE / 2.0) };
        let items = std::mem::take(&mut self.items);
        if let Some((ctx, marker)) = self.para.take() {
            let has_content = items.iter().any(|i| !matches!(i, Item::Break | Item::Anchor(_)));
            if has_content || marker.is_some() {
                let mut items = items;
                while items.last() == Some(&Item::Break) {
                    items.pop();
                }
                self.blocks.push(Block::Para { items, ctx, marker, space: std::mem::take(&mut self.pending_space) });
            } else if !items.is_empty() {
                // Anchors alone: kept for the next paragraph.
                self.items = items.into_iter().filter(|i| matches!(i, Item::Anchor(_))).collect();
                if !self.items.is_empty() {
                    self.para = Some((ctx, None));
                }
            }
        }
        self.pending_space = self.pending_space.max(space);
    }

    fn text(&mut self, text: &str) {
        if self.in_title {
            let t = self.title.get_or_insert_with(String::new);
            if t.len() < 1000 {
                t.push_str(text);
            }
            return;
        }
        let style = self.style();
        if style.hidden {
            return;
        }
        let text = drawable(text, &mut self.report);
        if style.pre {
            let text = if std::mem::take(&mut self.pre_start) {
                text.strip_prefix('\n').or_else(|| text.strip_prefix("\r\n")).unwrap_or(&text)
            } else {
                &text
            };
            let mut first = true;
            for line in text.split('\n') {
                if !first {
                    self.push_item(Item::Break);
                }
                first = false;
                let line = expand_tabs(line.trim_end_matches('\r'));
                if !line.is_empty() {
                    self.push_item(Item::Word { text: line, style, space: false });
                }
            }
            return;
        }
        self.pre_start = false;
        let mut words = text.split(|c: char| c.is_ascii_whitespace() || c == '\u{c}').peekable();
        if text.starts_with(|c: char| c.is_ascii_whitespace()) {
            self.space = true;
        }
        while let Some(word) = words.next() {
            if word.is_empty() {
                continue;
            }
            let space = std::mem::take(&mut self.space);
            // Leading space at the start of a paragraph, line or cell is dropped.
            let space = space && self.sink().last().is_some_and(|i| !matches!(i, Item::Break | Item::Anchor(_)));
            self.push_item(Item::Word { text: word.to_string(), style, space });
            self.space = words.peek().is_some();
        }
        if text.ends_with(|c: char| c.is_ascii_whitespace()) {
            self.space = true;
        }
    }

    fn token(&mut self, token: Token) {
        match token {
            Token::Text(t) => self.text(&t),
            Token::Start { name, attrs, self_closing } => self.start(&name, &attrs, self_closing),
            Token::End(name) => self.end(&name),
        }
    }

    fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
        attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    fn start(&mut self, name: &str, attrs: &[(String, String)], self_closing: bool) {
        // An open paragraph, list item, cell or row closes when another starts (HTML's
        // optional end tags).
        let closes: &[&str] = match name {
            "li" => &["li"],
            "dt" | "dd" => &["dt", "dd"],
            "tr" => &["tr", "td", "th"],
            "td" | "th" => &["td", "th"],
            "option" => &["option"],
            _ if block_space(name).is_some() && name != "td" && name != "th" && name != "tr" => &["p"],
            _ => &[],
        };
        for c in closes {
            // Only within the innermost list or table.
            let limit = self.stack.iter().rposition(|o| matches!(o.name.as_str(), "ul" | "ol" | "dl" | "table")).map_or(0, |i| i + 1);
            if self.stack.get(limit..).is_some_and(|s| s.iter().any(|o| o.name == *c)) {
                self.end(c);
            }
        }
        let parent = self.style();
        let mut style = parent;
        let mut ctx = self.ctx();
        match name {
            "b" | "strong" => style.bold = true,
            "i" | "em" | "cite" | "var" | "dfn" | "address" => style.italic = true,
            "code" | "kbd" | "samp" | "tt" => style.family = Family::Mono,
            "pre" | "listing" | "textarea" => {
                style.family = Family::Mono;
                style.pre = true;
                style.set_size(parent.size * 0.9);
                self.pre_start = true;
            }
            "u" | "ins" => style.underline = true,
            "s" | "strike" | "del" => style.strike = true,
            "small" => style.set_size(parent.size * 0.83),
            "big" => style.set_size(parent.size * 1.2),
            "sup" | "sub" => {
                style.set_size(parent.size * 0.75);
                style.raise = parent.raise + if name == "sup" { parent.size * 0.35 } else { -parent.size * 0.2 };
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                style.bold = true;
                let size = match name {
                    "h1" => 24.0,
                    "h2" => 18.0,
                    "h3" => 14.0,
                    "h4" => 12.0,
                    "h5" => 10.0,
                    _ => 8.0,
                };
                style.set_size(size);
            }
            "th" => style.bold = true,
            "summary" | "legend" => style.bold = true,
            "center" => ctx.align = Align::Center,
            "blockquote" | "figure" => {
                ctx.left += 30.0;
                ctx.right += 30.0;
            }
            "ul" | "ol" | "menu" | "dir" => ctx.left += 30.0,
            "dd" => ctx.left += 30.0,
            "a" => {
                if let Some(href) = Self::attr(attrs, "href").map(str::trim).filter(|h| !h.is_empty())
                    && self.links.len() < 10_000
                {
                    style.link = u32::try_from(self.links.len()).ok();
                    self.links.push(href.to_string());
                    style.color = [0, 0, 238];
                    style.underline = true;
                }
            }
            "font" => {
                if let Some(size) = Self::attr(attrs, "size").and_then(|s| s.trim().parse::<i32>().ok()) {
                    const SIZES: [f64; 7] = [7.5, 10.0, 12.0, 13.5, 18.0, 24.0, 36.0];
                    let i = usize::try_from(size.clamp(1, 7) - 1).unwrap_or(2);
                    style.set_size(SIZES.get(i).copied().unwrap_or(BASE_SIZE));
                }
                if let Some(face) = Self::attr(attrs, "face").and_then(parse_family) {
                    style.family = face;
                }
            }
            _ => {}
        }
        if hidden_element(name) {
            style.hidden = true;
        }
        if name == "th" {
            ctx.align = Align::Center;
        }
        apply_style_attrs(&mut style, &mut ctx, attrs);
        if name == "title" {
            self.in_title = true;
        }
        if let Some(space) = block_space(name)
            && !style.hidden
        {
            self.block_break(space);
        }
        // Anchors for links to `#id`.
        if !style.hidden
            && let Some(id) = Self::attr(attrs, "id").or_else(|| if name == "a" { Self::attr(attrs, "name") } else { None })
            && !id.is_empty()
        {
            self.push_item(Item::Anchor(id.to_string()));
        }
        if !style.hidden {
            self.structure_start(name, attrs, &style, &ctx);
        }
        if void_element(name) || self_closing && !matches!(name, "script" | "style" | "title" | "textarea") {
            return;
        }
        if self.stack.len() < MAX_DEPTH {
            self.stack.push(Open { name: name.to_string(), style, ctx });
        }
    }

    /// What a starting element adds besides its style: list markers, table structure, breaks,
    /// rules and images.
    fn structure_start(&mut self, name: &str, attrs: &[(String, String)], style: &Style, ctx: &BlockCtx) {
        match name {
            "ul" | "menu" | "dir" => self.lists.push(ListCtx { ordered: false, next: 1 }),
            "ol" => {
                let start = Self::attr(attrs, "start").and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(1);
                self.lists.push(ListCtx { ordered: true, next: start });
            }
            "li" => {
                let value = Self::attr(attrs, "value").and_then(|s| s.trim().parse::<i64>().ok());
                let nested = self.lists.len() > 1;
                let marker = match self.lists.last_mut() {
                    Some(list) if list.ordered => {
                        let n = value.unwrap_or(list.next);
                        list.next = n.saturating_add(1);
                        format!("{n}.")
                    }
                    Some(_) if nested => "\u{2013}".to_string(),
                    _ => "\u{2022}".to_string(),
                };
                let marker_style = Style { bold: false, italic: false, underline: false, strike: false, link: None, raise: 0.0, ..*style };
                if self.in_cell() {
                    self.push_item(Item::Word { text: marker, style: marker_style, space: false });
                    self.space = true;
                } else {
                    self.marker = Some((marker, marker_style));
                }
            }
            "br" => {
                self.push_item(Item::Break);
                self.space = false;
            }
            "hr" if !self.in_cell() => {
                self.block_break(6.0);
                let space = std::mem::take(&mut self.pending_space);
                self.blocks.push(Block::Rule { ctx: *ctx, space });
                self.pending_space = 6.0;
            }
            "img" => self.image(attrs, style),
            "input" => {
                let kind = Self::attr(attrs, "type").unwrap_or("text").to_ascii_lowercase();
                if let Some(value) = Self::attr(attrs, "value").filter(|v| !v.trim().is_empty())
                    && !matches!(kind.as_str(), "hidden" | "password" | "checkbox" | "radio" | "image" | "file")
                {
                    let text = format!("[{}]", value.trim());
                    self.text(&format!(" {text} "));
                }
            }
            "q" => self.text("\u{201C}"),
            "table" => {
                if self.in_cell() || self.nested_tables > 0 {
                    self.nested_tables += 1;
                } else {
                    let border = Self::attr(attrs, "border").is_some_and(|b| b.trim() != "0");
                    self.tables.push(TableCtx { border, ctx: *ctx, ..Default::default() });
                }
            }
            "caption" if self.nested_tables == 0 => {
                if let Some(t) = self.tables.last_mut() {
                    t.in_caption = true;
                }
            }
            "tr" if self.nested_tables == 0 => {
                if let Some(t) = self.tables.last_mut() {
                    close_row(t);
                    t.row = Some(Vec::new());
                }
            }
            "td" | "th" if self.nested_tables == 0 => {
                let span = Self::attr(attrs, "colspan").and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, MAX_COLUMNS);
                if let Some(t) = self.tables.last_mut() {
                    close_cell(t);
                    t.row.get_or_insert_with(Vec::new);
                    t.cell = Some(Cell { items: Vec::new(), header: name == "th", span, align: ctx.align });
                }
                self.space = false;
            }
            "td" | "th" => self.text(" "),
            _ => {}
        }
    }

    fn image(&mut self, attrs: &[(String, String)], style: &Style) {
        let alt = Self::attr(attrs, "alt").map(str::trim).unwrap_or_default().to_string();
        let embedded = Self::attr(attrs, "src").and_then(data_url).and_then(|bytes| {
            if self.images.len() >= MAX_IMAGES {
                return None;
            }
            // Image decoders read untrusted bytes.
            let (r, natural) = image_xobject(self.doc, "image", &bytes).ok()?;
            Some((r, natural))
        });
        let Some((r, (nw, nh))) = embedded else {
            self.report.images_skipped = self.report.images_skipped.saturating_add(1);
            let label = if alt.is_empty() { "[image]".to_string() } else { format!("[{alt}]") };
            let saved = self.style();
            let mut s = Style { italic: true, color: [110, 110, 110], ..saved };
            s.link = style.link;
            let text = drawable(&label, &mut self.report);
            let space = std::mem::take(&mut self.space);
            for (k, word) in text.split_ascii_whitespace().enumerate() {
                self.push_item(Item::Word { text: word.to_string(), style: s, space: space || k > 0 });
            }
            return;
        };
        let px = |v: &str| v.trim().trim_end_matches("px").parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0).map(|n| n * PX);
        let (aw, ah) = (Self::attr(attrs, "width").and_then(px), Self::attr(attrs, "height").and_then(px));
        // The image's own size is in points at its resolution; a browser shows pixels at 96 dpi.
        let (nw, nh) = (nw * PX, nh * PX);
        let (width, height) = match (aw, ah) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) if nw > 0.0 => (w, w * nh / nw),
            (None, Some(h)) if nh > 0.0 => (h * nw / nh, h),
            _ => (nw, nh),
        };
        if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
            self.report.images_skipped = self.report.images_skipped.saturating_add(1);
            return;
        }
        let index = self.images.len();
        self.images.push(r);
        let space = std::mem::take(&mut self.space);
        self.push_item(Item::Image { index, width, height, link: style.link, space });
    }

    fn end(&mut self, name: &str) {
        let Some(at) = self.stack.iter().rposition(|o| o.name == name) else {
            // `</p>` without a `<p>` is an empty paragraph; `</br>` a line break.
            if name == "br" {
                self.push_item(Item::Break);
            }
            return;
        };
        // Close what was left open inside it first, innermost first.
        while self.stack.len() > at + 1 {
            if let Some(open) = self.stack.pop() {
                self.close(&open);
            }
        }
        if let Some(open) = self.stack.pop() {
            self.close(&open);
        }
    }

    fn close(&mut self, open: &Open) {
        if open.name == "title" {
            self.in_title = false;
        }
        if open.style.hidden {
            return;
        }
        match open.name.as_str() {
            "ul" | "ol" | "menu" | "dir" => {
                self.block_break(0.0);
                self.lists.pop();
            }
            "q" => self.text("\u{201D}"),
            "pre" | "listing" | "textarea" => self.pre_start = false,
            "table" if self.nested_tables > 0 => {
                self.nested_tables -= 1;
                self.block_break(0.0);
            }
            "table" => {
                if let Some(mut t) = self.tables.pop() {
                    close_row(&mut t);
                    let space = std::mem::take(&mut self.pending_space).max(6.0);
                    if !t.caption.is_empty() {
                        let ctx = BlockCtx { align: Align::Center, ..t.ctx };
                        self.blocks.push(Block::Para { items: std::mem::take(&mut t.caption), ctx, marker: None, space });
                    }
                    if !t.rows.is_empty() {
                        let space = if self.blocks.last().is_some_and(|b| matches!(b, Block::Para { .. })) && space == 0.0 { 0.0 } else { space };
                        self.blocks.push(Block::Table { rows: t.rows, ctx: t.ctx, border: t.border, space });
                    }
                    self.pending_space = 6.0;
                }
            }
            "caption" if self.nested_tables == 0 => {
                if let Some(t) = self.tables.last_mut() {
                    t.in_caption = false;
                }
            }
            "tr" if self.nested_tables > 0 => self.block_break(0.0),
            "tr" => {
                if let Some(t) = self.tables.last_mut() {
                    close_row(t);
                }
            }
            "td" | "th" if self.nested_tables == 0 => {
                if let Some(t) = self.tables.last_mut() {
                    close_cell(t);
                }
            }
            _ => {}
        }
        if let Some(space) = block_space(&open.name)
            && !matches!(open.name.as_str(), "td" | "th" | "tr" | "table" | "caption")
        {
            self.block_break(space);
        }
    }

    fn finish(mut self) -> Built {
        while let Some(open) = self.stack.pop() {
            self.close(&open);
        }
        while let Some(mut t) = self.tables.pop() {
            close_row(&mut t);
            if !t.rows.is_empty() {
                self.blocks.push(Block::Table { rows: t.rows, ctx: t.ctx, border: t.border, space: 6.0 });
            }
        }
        self.block_break(0.0);
        let title = self.title.map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "));
        Built { blocks: self.blocks, links: self.links, images: self.images, title, report: self.report }
    }
}

fn close_cell(t: &mut TableCtx) {
    if let Some(mut cell) = t.cell.take() {
        while cell.items.last() == Some(&Item::Break) {
            cell.items.pop();
        }
        t.row.get_or_insert_with(Vec::new).push(cell);
    }
}

fn close_row(t: &mut TableCtx) {
    close_cell(t);
    if let Some(row) = t.row.take()
        && !row.is_empty()
        && t.rows.len() < 100_000
    {
        t.rows.push(row);
    }
}

/// Tabs to the next multiple of eight columns.
fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + 8);
    let mut col = 0usize;
    for c in line.chars() {
        if c == '\t' {
            let n = 8 - col % 8;
            out.extend(std::iter::repeat_n(' ', n));
            col += n;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
}

/// The bytes of a base64 `data:` URL (`data:image/png;base64,…`).
fn data_url(src: &str) -> Option<Vec<u8>> {
    let rest = src.trim().strip_prefix("data:").or_else(|| src.trim().strip_prefix("DATA:"))?;
    let (meta, data) = rest.split_once(',')?;
    if !meta.to_ascii_lowercase().ends_with(";base64") {
        return None;
    }
    base64(data)
}

/// Standard or URL-safe base64, ignoring whitespace and padding.
fn base64(data: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in data.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b if b.is_ascii_whitespace() => continue,
            // `%2B` and friends in a URL-encoded data URL aren't worth guessing at.
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xFF).ok()?);
        }
        if out.len() > MAX_IMAGE_BYTES {
            return None;
        }
    }
    Some(out)
}

// ── lines ───────────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum Placed {
    Text { x: f64, text: String, style: Style, width: f64 },
    Image { x: f64, index: usize, width: f64, height: f64, link: Option<u32> },
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Line {
    placed: Vec<Placed>,
    width: f64,
    /// Above and below the baseline, points.
    ascent: f64,
    descent: f64,
    anchors: Vec<String>,
}

impl Line {
    fn height(&self) -> f64 {
        self.ascent + self.descent
    }
}

fn measure(text: &str, style: &Style) -> f64 {
    let w = style.face().text_width(text, style.size);
    if w.is_finite() { w } else { 0.0 }
}

/// The widest unbreakable piece of `items` (a word, an image) and their width on one line.
fn extent(items: &[Item]) -> (f64, f64) {
    let (mut widest, mut line, mut longest) = (0.0f64, 0.0f64, 0.0f64);
    for item in items {
        match item {
            Item::Word { text, style, space } => {
                let w = measure(text, style);
                widest = widest.max(w);
                if *space {
                    line += measure(" ", style);
                }
                line += w;
            }
            Item::Image { width, space, .. } => {
                widest = widest.max(*width);
                line += *width + if *space { 3.0 } else { 0.0 };
            }
            Item::Break => {
                longest = longest.max(line);
                line = 0.0;
            }
            Item::Anchor(_) => {}
        }
    }
    (widest, longest.max(line))
}

/// Break `items` into lines no wider than `width` (points). Words wider than a line are split
/// between characters; images wider than a line are scaled to fit.
fn lines(items: &[Item], width: f64) -> Vec<Line> {
    let width = width.max(12.0);
    let mut out = Vec::new();
    let mut line = Line::default();
    let mut x = 0.0f64;
    let mut last_size = BASE_SIZE;
    let finish = |line: &mut Line, x: &mut f64, out: &mut Vec<Line>, size: f64| {
        if line.placed.is_empty() {
            // An empty line (`<br><br>`) is as tall as the text around it.
            line.ascent = line.ascent.max(size * 0.95);
            line.descent = line.descent.max(size * 0.3);
        }
        line.width = *x;
        out.push(std::mem::take(line));
        *x = 0.0;
    };
    for item in items {
        match item {
            Item::Break => finish(&mut line, &mut x, &mut out, last_size),
            Item::Anchor(a) => line.anchors.push(a.clone()),
            Item::Word { text, style, space } => {
                last_size = style.size;
                let mut space_w = if *space && x > 0.0 { measure(" ", style) } else { 0.0 };
                let w = measure(text, style);
                if x > 0.0 && x + space_w + w > width {
                    finish(&mut line, &mut x, &mut out, last_size);
                    space_w = 0.0;
                }
                let pieces = if w > width { split_word(text, style, width) } else { vec![text.clone()] };
                let count = pieces.len();
                for (k, piece) in pieces.into_iter().enumerate() {
                    let pw = measure(&piece, style);
                    let gap = if k == 0 { space_w } else { 0.0 };
                    place_text(&mut line, x, gap, piece, *style, pw);
                    x += gap + pw;
                    if k + 1 < count {
                        finish(&mut line, &mut x, &mut out, last_size);
                    }
                }
            }
            Item::Image { index, width: iw, height: ih, link, space } => {
                let scale = if *iw > width { width / iw } else { 1.0 };
                let (iw, ih) = (iw * scale, ih * scale);
                let gap = if *space && x > 0.0 { 3.0 } else { 0.0 };
                if x > 0.0 && x + gap + iw > width {
                    finish(&mut line, &mut x, &mut out, last_size);
                }
                let gap = if x > 0.0 { gap } else { 0.0 };
                line.placed.push(Placed::Image { x: x + gap, index: *index, width: iw, height: ih, link: *link });
                line.ascent = line.ascent.max(ih);
                line.descent = line.descent.max(2.0);
                x += gap + iw;
            }
        }
    }
    if !line.placed.is_empty() || !line.anchors.is_empty() {
        finish(&mut line, &mut x, &mut out, last_size);
    }
    out
}

fn place_text(line: &mut Line, x: f64, gap: f64, text: String, style: Style, width: f64) {
    line.ascent = line.ascent.max(style.size * 0.95 + style.raise.max(0.0));
    line.descent = line.descent.max(style.size * 0.3 - style.raise.min(0.0));
    // The same style as the text before it, without a gap or with one space: one run.
    if let Some(Placed::Text { text: t, style: s, width: w, x: px }) = line.placed.last_mut()
        && *s == style
        && (*px + *w + gap - x - gap).abs() < 0.01
    {
        if gap > 0.0 {
            t.push(' ');
        }
        t.push_str(&text);
        *w += gap + width;
        return;
    }
    line.placed.push(Placed::Text { x: x + gap, text, style, width });
}

/// `word` in pieces no wider than `width`, broken between characters.
fn split_word(word: &str, style: &Style, width: f64) -> Vec<String> {
    let mut out = Vec::new();
    let mut piece = String::new();
    for c in word.chars() {
        piece.push(c);
        if piece.chars().count() > 1 && measure(&piece, style) > width {
            piece.pop();
            out.push(std::mem::take(&mut piece));
            piece.push(c);
        }
    }
    if !piece.is_empty() {
        out.push(piece);
    }
    out
}

// ── pages ───────────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct PageOut {
    content: Vec<u8>,
    fonts: Vec<Std14>,
    images: Vec<usize>,
    /// Link areas: rect and link index.
    links: Vec<([f64; 4], u32)>,
}

struct Writer<'d> {
    doc: &'d mut Document,
    w: f64,
    h: f64,
    pages: Vec<PageOut>,
    /// The top of the free space on the current page, points from the bottom.
    y: f64,
    anchors: HashMap<String, (usize, f64)>,
}

fn rgb(c: [u8; 3]) -> String {
    format!("{:.3} {:.3} {:.3}", f64::from(c[0]) / 255.0, f64::from(c[1]) / 255.0, f64::from(c[2]) / 255.0)
}

impl<'d> Writer<'d> {
    fn new(doc: &'d mut Document, w: f64, h: f64) -> Self {
        Writer { doc, w, h, pages: vec![PageOut::default()], y: h - MARGIN, anchors: HashMap::new() }
    }

    fn top(&self) -> f64 {
        self.h - MARGIN
    }

    fn content_width(&self) -> f64 {
        self.w - 2.0 * MARGIN
    }

    fn page(&mut self) -> &mut PageOut {
        if self.pages.is_empty() {
            self.pages.push(PageOut::default());
        }
        let last = self.pages.len() - 1;
        &mut self.pages[last]
    }

    fn new_page(&mut self) -> Result<(), CreateError> {
        if self.pages.len() >= MAX_PAGES {
            return Err(CreateError::Invalid(format!("the page would need more than {MAX_PAGES} PDF pages")));
        }
        self.pages.push(PageOut::default());
        self.y = self.top();
        Ok(())
    }

    /// Room for `height` points below the cursor, starting a new page if there isn't (unless the
    /// page is still empty: something taller than a page starts at its top).
    fn room(&mut self, height: f64) -> Result<(), CreateError> {
        if self.y - height < MARGIN && self.y < self.top() - 0.5 {
            self.new_page()?;
        }
        Ok(())
    }

    fn space(&mut self, space: f64) {
        // Space at the top of a page is dropped, as when a browser prints.
        if self.y < self.top() - 0.5 {
            self.y -= space.min(self.y - MARGIN).max(0.0);
        }
    }

    fn write(mut self, built: Built) -> Result<(), CreateError> {
        for block in &built.blocks {
            match block {
                Block::Para { items, ctx, marker, space } => {
                    self.space(*space);
                    let left = MARGIN + ctx.left;
                    let width = (self.content_width() - ctx.left - ctx.right).max(36.0);
                    let lines = lines(items, width);
                    let lines = if lines.is_empty() && marker.is_some() {
                        vec![Line { ascent: BASE_SIZE * 0.95, descent: BASE_SIZE * 0.3, ..Default::default() }]
                    } else {
                        lines
                    };
                    for (k, line) in lines.iter().enumerate() {
                        self.room(line.height())?;
                        let baseline = self.y - line.ascent;
                        let offset = match ctx.align {
                            Align::Left => 0.0,
                            Align::Center => ((width - line.width) / 2.0).max(0.0),
                            Align::Right => (width - line.width).max(0.0),
                        };
                        self.line(line, left + offset, baseline);
                        if k == 0
                            && let Some((text, style)) = marker
                        {
                            let mw = measure(text, style);
                            self.text_op(left - mw - 6.0, baseline, text, style);
                        }
                        self.y -= line.height();
                    }
                }
                Block::Rule { ctx, space } => {
                    self.space(*space);
                    self.room(2.0)?;
                    let (x0, x1) = (MARGIN + ctx.left, self.w - MARGIN - ctx.right);
                    let y = self.y - 1.0;
                    let op = format!("q 0.6 0.6 0.6 RG 1 w {x0:.2} {y:.2} m {x1:.2} {y:.2} l S Q\n");
                    self.page().content.extend_from_slice(op.as_bytes());
                    self.y -= 2.0;
                }
                Block::Table { rows, ctx, border, space } => {
                    self.space(*space);
                    self.table(rows, ctx, *border)?;
                }
            }
        }
        self.finish(built)
    }

    /// Draw one laid-out line with its left edge at `x` and its baseline at `baseline`.
    fn line(&mut self, line: &Line, x: f64, baseline: f64) {
        let page = self.pages.len().saturating_sub(1);
        for a in &line.anchors {
            self.anchors.entry(a.clone()).or_insert((page, self.y));
        }
        for p in &line.placed {
            match p {
                Placed::Text { x: px, text, style, width } => {
                    let (tx, ty) = (x + px, baseline + style.raise);
                    self.text_op(tx, ty, text, style);
                    let c = rgb(style.color);
                    let thick = (style.size / 18.0).max(0.5);
                    if style.underline {
                        let op = format!("q {c} rg {tx:.2} {:.2} {width:.2} {thick:.2} re f Q\n", ty - style.size * 0.12);
                        self.page().content.extend_from_slice(op.as_bytes());
                    }
                    if style.strike {
                        let op = format!("q {c} rg {tx:.2} {:.2} {width:.2} {thick:.2} re f Q\n", ty + style.size * 0.28);
                        self.page().content.extend_from_slice(op.as_bytes());
                    }
                    if let Some(link) = style.link {
                        let rect = [tx, ty - style.size * 0.25, tx + width, ty + style.size * 0.9];
                        self.page().links.push((rect, link));
                    }
                }
                Placed::Image { x: px, index, width, height, link } => {
                    let (ix, iy) = (x + px, baseline);
                    let op = format!("q {width:.3} 0 0 {height:.3} {ix:.2} {iy:.2} cm /Im{index} Do Q\n");
                    let page = self.page();
                    page.content.extend_from_slice(op.as_bytes());
                    if !page.images.contains(index) {
                        page.images.push(*index);
                    }
                    if let Some(link) = link {
                        page.links.push(([ix, iy, ix + width, iy + height], *link));
                    }
                }
            }
        }
    }

    fn text_op(&mut self, x: f64, y: f64, text: &str, style: &Style) {
        let face = style.face();
        let mut op = format!("BT /F{} {:.2} Tf {} rg {x:.2} {y:.2} Td ", face as usize, style.size, rgb(style.color)).into_bytes();
        op.extend(literal(&win_ansi(text)));
        op.extend_from_slice(b" Tj ET\n");
        let page = self.page();
        page.content.extend(op);
        if !page.fonts.contains(&face) {
            page.fonts.push(face);
        }
    }

    fn table(&mut self, rows: &[Vec<Cell>], ctx: &BlockCtx, border: bool) -> Result<(), CreateError> {
        let columns = rows.iter().map(|r| r.iter().map(|c| c.span).sum::<usize>()).max().unwrap_or(1).clamp(1, MAX_COLUMNS);
        let avail = (self.content_width() - ctx.left - ctx.right).max(36.0);
        // Each column's narrowest (its widest word) and natural (its longest line) widths.
        let (mut min, mut natural) = (vec![0.0f64; columns], vec![0.0f64; columns]);
        for row in rows {
            let mut col = 0usize;
            for cell in row {
                let (widest, longest) = extent(&cell.items);
                let span = cell.span.min(columns.saturating_sub(col)).max(1);
                let pad = 2.0 * CELL_PAD;
                if let (Some(m), Some(n)) = (min.get_mut(col), natural.get_mut(col))
                    && span == 1
                {
                    *m = m.max(widest + pad);
                    *n = n.max(longest + pad);
                }
                col = (col + span).min(columns - 1);
            }
        }
        let (sum_min, sum_nat): (f64, f64) = (min.iter().sum(), natural.iter().sum());
        let widths: Vec<f64> = if sum_nat <= avail {
            natural.iter().map(|n| n.max(2.0 * CELL_PAD + 12.0)).collect()
        } else if sum_min < avail && sum_nat > sum_min {
            let extra = avail - sum_min;
            min.iter().zip(&natural).map(|(m, n)| m + extra * (n - m) / (sum_nat - sum_min)).collect()
        } else {
            vec![avail / columns as f64; columns]
        };
        let total: f64 = widths.iter().sum::<f64>().min(avail);
        let left = MARGIN
            + ctx.left
            + match ctx.align {
                Align::Left => 0.0,
                Align::Center => ((avail - total) / 2.0).max(0.0),
                Align::Right => (avail - total).max(0.0),
            };
        for row in rows {
            // Lay out every cell of the row in its columns.
            let mut cells = Vec::new();
            let mut col = 0usize;
            for cell in row {
                let span = cell.span.min(columns.saturating_sub(col)).max(1);
                let x = left + widths.get(..col).map_or(0.0, |w| w.iter().sum());
                let w: f64 = widths.get(col..col + span).map_or(avail / columns as f64, |w| w.iter().sum());
                let laid = lines(&cell.items, (w - 2.0 * CELL_PAD).max(8.0));
                cells.push((x, w, cell, laid));
                col = (col + span).min(columns - 1);
            }
            let height = cells.iter().map(|(_, _, _, l)| l.iter().map(Line::height).sum::<f64>()).fold(0.0, f64::max) + 2.0 * CELL_PAD;
            let page_height = self.top() - MARGIN;
            if height > page_height {
                // Taller than a page: its cells one after the other, as paragraphs.
                for (x, w, cell, laid) in &cells {
                    for line in laid {
                        self.room(line.height())?;
                        let baseline = self.y - line.ascent;
                        let offset = align_offset(cell.align, w - 2.0 * CELL_PAD, line.width);
                        self.line(line, x + CELL_PAD + offset, baseline);
                        self.y -= line.height();
                    }
                }
                continue;
            }
            self.room(height)?;
            let top = self.y;
            for (x, w, cell, laid) in &cells {
                if cell.header {
                    let op = format!("q 0.94 0.94 0.94 rg {x:.2} {:.2} {w:.2} {height:.2} re f Q\n", top - height);
                    self.page().content.extend_from_slice(op.as_bytes());
                }
                let mut y = top - CELL_PAD;
                for line in laid {
                    let baseline = y - line.ascent;
                    let offset = align_offset(cell.align, w - 2.0 * CELL_PAD, line.width);
                    let saved = self.y;
                    self.y = y;
                    self.line(line, x + CELL_PAD + offset, baseline);
                    self.y = saved;
                    y -= line.height();
                }
                let stroke = if border { "0.5 0.5 0.5" } else { "0.85 0.85 0.85" };
                let op = format!("q {stroke} RG 0.5 w {x:.2} {:.2} {w:.2} {height:.2} re S Q\n", top - height);
                self.page().content.extend_from_slice(op.as_bytes());
            }
            self.y = top - height;
        }
        Ok(())
    }

    /// Add the pages, their fonts, images and links to the document.
    fn finish(self, built: Built) -> Result<(), CreateError> {
        let Writer { doc, w, h, pages, anchors, .. } = self;
        let mut fonts: HashMap<usize, ObjRef> = HashMap::new();
        let mut refs = Vec::with_capacity(pages.len());
        for page in &pages {
            let mut font_dict = Dict::new();
            for face in &page.fonts {
                let r = *fonts.entry(*face as usize).or_insert_with(|| {
                    let mut f = Dict::new();
                    f.set(b"Type".to_vec(), Object::name("Font"));
                    f.set(b"Subtype".to_vec(), Object::name("Type1"));
                    f.set(b"BaseFont".to_vec(), Object::name(face.base_font()));
                    f.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
                    doc.add(f)
                });
                font_dict.set(format!("F{}", *face as usize).into_bytes(), Object::Ref(r));
            }
            let mut xobjects = Dict::new();
            for index in &page.images {
                if let Some(r) = built.images.get(*index) {
                    xobjects.set(format!("Im{index}").into_bytes(), Object::Ref(*r));
                }
            }
            let mut resources = Dict::new();
            resources.set(b"Font".to_vec(), Object::Dict(font_dict));
            if !page.images.is_empty() {
                resources.set(b"XObject".to_vec(), Object::Dict(xobjects));
            }
            let content = if page.content.is_empty() { None } else { Some(page.content.clone()) };
            refs.push(add_page(doc, w, h, resources, content)?);
        }
        for (page, r) in pages.iter().zip(&refs) {
            let mut annots = Vec::new();
            for (rect, link) in &page.links {
                let Some(href) = built.links.get(*link as usize) else { continue };
                let mut a = Dict::new();
                a.set(b"Type".to_vec(), Object::name("Annot"));
                a.set(b"Subtype".to_vec(), Object::name("Link"));
                a.set(b"Rect".to_vec(), Object::Array(rect.iter().map(|v| Object::Real(*v)).collect()));
                a.set(b"Border".to_vec(), Object::Array(vec![0.into(), 0.into(), 0.into()]));
                if let Some(id) = href.strip_prefix('#') {
                    let Some((p, y)) = anchors.get(id) else { continue };
                    let Some(target) = refs.get(*p) else { continue };
                    a.set(
                        b"Dest".to_vec(),
                        Object::Array(vec![Object::Ref(*target), Object::name("XYZ"), Object::Null, Object::Real(*y), Object::Null]),
                    );
                } else if href.contains(':') && !href.to_ascii_lowercase().starts_with("javascript:") && href.is_ascii() {
                    let mut action = Dict::new();
                    action.set(b"S".to_vec(), Object::name("URI"));
                    action.set(b"URI".to_vec(), Object::String(PdfString::literal(href.as_bytes().to_vec())));
                    a.set(b"A".to_vec(), Object::Dict(action));
                } else {
                    // A link relative to the page's own address leads nowhere from a PDF.
                    continue;
                }
                annots.push(Object::Ref(doc.add(a)));
            }
            if !annots.is_empty() {
                let _ = doc.update_dict(*r, |d| d.set(b"Annots".to_vec(), Object::Array(annots)));
            }
        }
        Ok(())
    }
}

fn align_offset(align: Align, width: f64, line: f64) -> f64 {
    match align {
        Align::Left => 0.0,
        Align::Center => ((width - line) / 2.0).max(0.0),
        Align::Right => (width - line).max(0.0),
    }
}

#[cfg(test)]
mod tests;
