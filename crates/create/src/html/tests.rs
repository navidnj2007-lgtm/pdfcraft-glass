use std::sync::Arc;

use pdfcraft_cos::{SaveOptions, write_full};

use super::*;

fn reopen(doc: &Document) -> Document {
    let bytes = write_full(doc, &SaveOptions::default()).unwrap();
    hayro_syntax::Pdf::new(bytes.clone()).expect("parses");
    Document::open(Arc::new(bytes)).unwrap()
}

fn pages(doc: &Document) -> Vec<Dict> {
    let pages = doc.get(doc.root().unwrap()).as_dict().unwrap().reference(b"Pages").unwrap();
    let kids = doc.get(pages).as_dict().unwrap().get(b"Kids").unwrap().as_array().unwrap().clone();
    kids.iter().map(|k| doc.resolve(k).as_dict().cloned().unwrap()).collect()
}

/// Every page's content stream, decoded, as text.
fn contents(doc: &Document) -> Vec<String> {
    pages(doc)
        .iter()
        .map(|p| match p.get(b"Contents") {
            Some(c) => match &*doc.resolve(c) {
                // Latin-1, so each WinAnsi byte reads back as one character.
                Object::Stream(stream) => stream.decoded().unwrap().iter().map(|&b| char::from(b)).collect(),
                other => panic!("contents: {other:?}"),
            },
            None => String::new(),
        })
        .collect()
}

fn convert(html: &str) -> (Document, HtmlReport) {
    let (doc, report) = from_html("page.html", html.as_bytes(), crate::LETTER).unwrap();
    (reopen(&doc), report)
}

fn title(doc: &Document) -> String {
    let info = doc.trailer().get(b"Info").unwrap().clone();
    doc.resolve(&info).as_dict().unwrap().get(b"Title").unwrap().as_string().unwrap().to_text()
}

#[test]
fn recognises_html_files() {
    assert!(is_html("Page.HTML", b""));
    assert!(is_html("saved.htm", b"anything"));
    assert!(is_html("noext", b"\xEF\xBB\xBF  <!DOCTYPE html><p>x"));
    assert!(is_html("index", b"\n<html lang=en>"));
    assert!(!is_html("notes.txt", b"plain <b>text</b>"));
    assert!(!is_html("a.pdf", b"%PDF-1.7"));
}

#[test]
fn tokens_read_tags_attributes_and_references() {
    let tokens: Vec<Token> =
        Tokens::new("<P class=x id='a b' data-x=\"&lt;\">A&amp;B &eacute;&#233;&#x2014;&copy &bogus;<!-- c --><br/></p>").collect();
    assert_eq!(
        tokens,
        vec![
            Token::Start {
                name: "p".into(),
                attrs: vec![("class".into(), "x".into()), ("id".into(), "a b".into()), ("data-x".into(), "<".into())],
                self_closing: false
            },
            Token::Text("A&B é\u{e9}\u{2014}\u{a9} &bogus;".into()),
            Token::Start { name: "br".into(), attrs: vec![], self_closing: true },
            Token::End("p".into()),
        ]
    );
    // Scripts and styles are dropped whole, even when they hold tags; a title is text.
    let tokens: Vec<Token> = Tokens::new("<script>if (a<b) document.write('<p>x</p>')</SCRIPT><title>T &amp; U</title>").collect();
    assert_eq!(
        tokens,
        vec![
            Token::Start { name: "script".into(), attrs: vec![], self_closing: false },
            Token::End("script".into()),
            Token::Start { name: "title".into(), attrs: vec![], self_closing: false },
            Token::Text("T & U".into()),
            Token::End("title".into()),
        ]
    );
    // C1 numeric references are Windows-1252, as browsers read them; nonsense is U+FFFD.
    assert_eq!(decode_entities("&#150;&#0;&#x110000;&#99999999999;"), "\u{2013}\u{FFFD}\u{FFFD}\u{FFFD}");
    // A lone '<' is text, and an unclosed tag or comment ends the input without a panic.
    assert_eq!(Tokens::new("a < b").collect::<Vec<_>>(), vec![Token::Text("a ".into()), Token::Text("<".into()), Token::Text(" b".into())]);
    assert_eq!(Tokens::new("<!-- never closed").count(), 0);
    assert_eq!(Tokens::new("<p class=\"never").count(), 1);
}

#[test]
fn encodings_come_from_the_bom_or_the_meta_charset() {
    assert_eq!(decode_html(b"<meta charset=\"windows-1252\"><p>caf\xE9"), "<meta charset=\"windows-1252\"><p>caf\u{e9}");
    assert_eq!(
        decode_html(b"<meta http-equiv=Content-Type content='text/html; charset=ISO-8859-1'>\xFC"),
        "<meta http-equiv=Content-Type content='text/html; charset=ISO-8859-1'>\u{fc}"
    );
    assert_eq!(decode_html("<p>caf\u{e9}".as_bytes()), "<p>caf\u{e9}");
    assert_eq!(decode_html(b"<p>caf\xE9"), "<p>caf\u{e9}", "not UTF-8: Windows-1252");
    assert_eq!(decode_html(b"\xFF\xFEh\0i\0"), "hi");
}

#[test]
fn a_page_becomes_text_in_the_standard_fonts() {
    let (doc, report) = convert(
        "<!doctype html><html><head><title>My  page</title><style>p { color: red }</style><script>alert(1)</script></head>\
         <body><h1>Heading</h1><p>Hello <b>bold</b> and <i>italic</i> <code>code()</code>.</p></body></html>",
    );
    assert_eq!(report, HtmlReport::default());
    assert_eq!(title(&doc), "My page");
    let text = contents(&doc).join("\n");
    for expected in ["(Heading) Tj", "(Hello) Tj", "(bold) Tj", "(and) Tj", "(italic) Tj", "(code\\(\\)) Tj"] {
        assert!(text.contains(expected), "{expected} in {text}");
    }
    assert!(!text.contains("alert") && !text.contains("color"), "scripts and styles aren't text");
    // Helvetica for text, bold for the heading and <b>, oblique for <i>, Courier for code.
    let page = &pages(&doc)[0];
    let fonts = page.get(b"Resources").unwrap().as_dict().unwrap().get(b"Font").unwrap().as_dict().unwrap().clone();
    let base: Vec<String> =
        fonts.iter().map(|(_, f)| String::from_utf8_lossy(doc.resolve(f).as_dict().unwrap().name(b"BaseFont").unwrap()).into_owned()).collect();
    for face in ["Helvetica", "Helvetica-Bold", "Helvetica-Oblique", "Courier"] {
        assert!(base.iter().any(|b| b == face), "{face} in {base:?}");
    }
}

#[test]
fn the_title_falls_back_to_the_file_name() {
    let (doc, _) = from_html("Report.final.html", b"<p>x</p>", crate::LETTER).unwrap();
    assert_eq!(title(&reopen(&doc)), "Report.final");
}

#[test]
fn long_pages_flow_onto_more_pages() {
    let html: String = (0..400).map(|i| format!("<p>Paragraph number {i} with a few words in it.</p>")).collect();
    let (doc, _) = convert(&html);
    let texts = contents(&doc);
    assert!(texts.len() > 5, "{} pages", texts.len());
    assert!(texts[0].contains("(Paragraph number 0 with a few words in it.) Tj"));
    assert!(texts.last().unwrap().contains("(Paragraph number 399 with a few words in it.) Tj"));
    // Nothing is drawn in the margins.
    for t in &texts {
        for line in t.lines().filter(|l| l.contains(" Td ")) {
            let before = line.split(" Td ").next().unwrap();
            let y: f64 = before.split_whitespace().last().unwrap().parse().unwrap();
            assert!((MARGIN - 1.0..=792.0 - MARGIN).contains(&y), "{line}");
        }
    }
}

#[test]
fn lines_wrap_within_the_margins() {
    let words = "word ".repeat(300);
    let (doc, _) = convert(&format!("<p>{words}</p><pre>{}</pre>", "x".repeat(400)));
    let text = contents(&doc).join("\n");
    let lines: Vec<&str> = text.lines().filter(|l| l.contains(" Tj")).collect();
    assert!(lines.len() > 10, "wrapped into many lines");
    for l in &lines {
        let inner = l.split_once('(').unwrap().1.rsplit_once(')').unwrap().0;
        let size: f64 = l.split_whitespace().nth(2).unwrap().parse().unwrap();
        let face = if l.contains("/F8 ") { Std14::Courier } else { Std14::Helvetica };
        assert!(face.text_width(inner, size) <= 612.0 - 2.0 * MARGIN + 0.01, "{l}");
    }
}

#[test]
fn links_become_link_annotations() {
    let (doc, _) = convert(
        "<p><a href='https://example.com/a?b=1'>Web</a> <a href='#later'>Jump</a> <a href='relative.html'>Rel</a> \
         <a href='javascript:alert(1)'>JS</a></p>\
         <div style='height:10px'></div><h2 id=later>Later</h2>",
    );
    let page = &pages(&doc)[0];
    let annots = page.get(b"Annots").unwrap().as_array().unwrap().clone();
    assert_eq!(annots.len(), 2, "web and anchor links; relative and script links lead nowhere");
    let uri = doc.resolve(&annots[0]).as_dict().unwrap().get(b"A").unwrap().as_dict().unwrap().get(b"URI").unwrap().as_string().unwrap().to_text();
    assert_eq!(uri, "https://example.com/a?b=1");
    let dest = doc.resolve(&annots[1]).as_dict().unwrap().get(b"Dest").unwrap().as_array().unwrap().clone();
    assert!(matches!(dest[0], Object::Ref(_)) && dest.len() == 5);
}

#[test]
fn lists_tables_and_rules() {
    let (doc, _) = convert(
        "<ul><li>One<li>Two</ul><ol start=3><li>Three</li><li>Four</ol><hr>\
         <table border=1><tr><th>Name<th>Value</tr><tr><td>Alpha<td>1</tr><tr><td colspan=2>Wide cell</td></tr></table>",
    );
    let text = contents(&doc).join("\n");
    for expected in ["(\u{95}) Tj", "(One) Tj", "(Two) Tj", "(3.) Tj", "(4.) Tj", "(Name) Tj", "(Value) Tj", "(Alpha) Tj", "(1) Tj", "(Wide cell) Tj"]
    {
        assert!(text.contains(expected), "{expected} in {text}");
    }
    assert!(text.contains(" l S Q"), "the rule");
    assert!(text.matches(" re S Q").count() >= 5, "cell borders");
}

#[test]
fn characters_the_fonts_cant_draw_are_written_as_code_points() {
    let (doc, report) = convert("<p>caf\u{e9} \u{2014} \u{65e5}\u{672c} \u{1F600}\u{200B}</p>");
    assert_eq!(report.escaped, 3);
    let text = contents(&doc).join("\n");
    assert!(text.contains("[U+65E5][U+672C]") && text.contains("[U+1F600]"), "{text}");
    assert!(text.contains("(caf\u{e9} \u{97} [U+65E5]"), "WinAnsi as it is: {text}");
}

#[test]
fn data_url_images_are_embedded_and_linked_ones_are_not() {
    let mut png = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png, 4, 2);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[90u8; 24]).unwrap();
    }
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut b64 = String::new();
    for chunk in png.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for k in 0..4 {
            if k <= chunk.len() {
                b64.push(B64[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                b64.push('=');
            }
        }
    }
    assert_eq!(base64(&b64).unwrap(), png);
    let (doc, report) = convert(&format!("<p><img src='data:image/png;base64,{b64}' width=40> <img src='photo.jpg' alt='A photo'></p>"));
    assert_eq!(report.images_skipped, 1);
    let page = &pages(&doc)[0];
    let xobjects = page.get(b"Resources").unwrap().as_dict().unwrap().get(b"XObject").unwrap().as_dict().unwrap().clone();
    assert_eq!(xobjects.iter().count(), 1);
    let text = contents(&doc).join("\n");
    assert!(text.contains("/Im0 Do") && text.contains("([A photo]) Tj"), "{text}");
}

#[test]
fn styles_from_attributes() {
    let (doc, _) = convert(
        "<p style='text-align:center; color:#c00; font-weight:bold'>Centered</p><p style='display:none'>Hidden</p>\
         <p hidden>Also hidden</p><font size=7 color=blue face=Courier>Big</font><p>x<sup>2</sup></p>",
    );
    let text = contents(&doc).join("\n");
    assert!(!text.contains("Hidden"));
    assert!(text.contains("0.800 0.000 0.000 rg") && text.contains("(Centered) Tj"));
    assert!(text.contains("36.00 Tf 0.000 0.000 1.000 rg"), "{text}");
}

/// Malformed, hostile and huge input: never a panic, always a document.
#[test]
fn any_input_converts_without_panicking() {
    let samples: Vec<String> = vec![
        String::new(),
        "<".into(),
        "</".into(),
        "<a".into(),
        "&".into(),
        "&#".into(),
        "&#x".into(),
        "<table><tr><td colspan=99999999>x".into(),
        "<td>outside a table</td></tr></table>".into(),
        "<ol start=9223372036854775807><li>a<li>b".into(),
        "<b>".repeat(5000),
        "<div>".repeat(5000) + "deep",
        "<table><tr><td><table><tr><td>nested</table></table>".into(),
        "<pre>\t\ta\tb\n</pre>".into(),
        "<img src='data:image/png;base64,!!!!'><img src='data:image/png;base64,AAAA'>".into(),
        "<p style='font-size:1e308px;color:rgb(999,-5,nan)'>x</p>".into(),
        "<li>orphan item".into(),
        "<html><body>\u{0}\u{FFFF}\u{10FFFF}</body>".into(),
        "<a href='#missing'>nowhere</a>".into(),
    ];
    for html in &samples {
        let (doc, _) = from_html("x.html", html.as_bytes(), crate::LETTER).unwrap_or_else(|e| panic!("{html:.60}: {e}"));
        assert!(!pages(&reopen(&doc)).is_empty(), "{html:.60}");
    }
    // Pseudo-random bytes and tag soup.
    let mut seed = 0x2545F4914F6CDD1Du64;
    let atoms = [
        "<p>",
        "</p>",
        "<li>",
        "<ul>",
        "</ul>",
        "<table>",
        "<tr>",
        "<td>",
        "</table>",
        "<b>",
        "</b>",
        "<br>",
        "&amp;",
        "x ",
        "<pre>",
        "\n",
        "<a href=#a>",
        "</a>",
        "<h1 id=a>",
        "<img src=x>",
    ];
    for _ in 0..200 {
        let mut html = String::new();
        for _ in 0..60 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            html.push_str(atoms[(seed % atoms.len() as u64) as usize]);
        }
        assert!(from_html("x.html", html.as_bytes(), crate::LETTER).is_ok(), "{html}");
    }
    assert!(from_html("x.html", b"<p>x", (10.0, 10.0)).is_err(), "a page too small for the margins");
}
