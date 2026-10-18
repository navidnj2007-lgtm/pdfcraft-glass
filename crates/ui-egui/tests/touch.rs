//! Fingers and the Apple Pencil on a tablet: a finger drag pans the pages with Select, and with
//! the pens while only the Pencil draws, and gives the tool back when it lifts; otherwise a
//! finger draws and two fingers pan. The Pencil uses the tool and never scrolls.

use egui::{Event, Pos2, TouchDeviceId, TouchId, TouchPhase, pos2, vec2};
use egui_kittest::Harness;
use pdfcraft_ui_egui::{PdfCraftApp, QuickTool};

/// `n` pages of 200×300 points, each saying "Page i".
fn fixture(n: usize) -> Vec<u8> {
    let mut objs: Vec<String> = vec!["<< /Type /Catalog /Pages 2 0 R >>".into()];
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 200 300] >>", kids.join(" ")));
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    for i in 0..n {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>", 5 + 2 * i));
        let body = format!("BT /F1 18 Tf 20 150 Td (Page {}) Tj ET", i + 1);
        objs.push(format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// An iPad-sized window with a long document, shown in the immersive layout.
fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(vec2(1024.0, 768.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.immersive = true;
        app.open_bytes("long.pdf", None, fixture(8)).unwrap();
        app
    });
    h.run_steps(6);
    h
}

fn page_top(h: &Harness<'static, PdfCraftApp>) -> f32 {
    h.state().views[0].page_screen_rect(0).expect("page 1 laid out").top()
}

fn annotations(h: &Harness<'static, PdfCraftApp>) -> usize {
    let s = h.state();
    s.session.get(s.views[0].id).map_or(0, |d| d.info.annotations.len())
}

/// A stroke by one touch along `path`, as the browser reports it: `pointer_type` is what
/// `PointerEvent.pointerType` says ("touch" for a finger, "pen" for the Pencil), and egui gets
/// the press, moves and release eframe derives from the touch events.
fn stroke(h: &mut Harness<'static, PdfCraftApp>, pointer_type: &str, path: &[Pos2]) {
    strokes(h, pointer_type, &[path]);
}

/// Touches moving together along `paths` (one per finger, of equal length), the first being the
/// one that drives the egui pointer.
fn strokes(h: &mut Harness<'static, PdfCraftApp>, pointer_type: &str, paths: &[&[Pos2]]) {
    let Some(path) = paths.first() else { return };
    let (Some(&first), Some(&last)) = (path.first(), path.last()) else { return };
    h.state().pointer_kind.pointer_down(pointer_type);
    let touch = |id: usize, phase, pos| Event::Touch { device_id: TouchDeviceId(1), id: TouchId(id as u64 + 1), phase, pos, force: None };
    let primary = |pos, pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    h.input_mut().events.extend([Event::PointerMoved(first), primary(first, true)]);
    for (k, p) in paths.iter().enumerate() {
        h.input_mut().events.push(touch(k, TouchPhase::Start, p[0]));
    }
    h.run_steps(1);
    for step in 1..path.len() {
        h.input_mut().events.push(Event::PointerMoved(path[step]));
        for (k, p) in paths.iter().enumerate() {
            h.input_mut().events.push(touch(k, TouchPhase::Move, p[step]));
        }
        h.run_steps(1);
    }
    h.input_mut().events.push(primary(last, false));
    for (k, p) in paths.iter().enumerate() {
        h.input_mut().events.push(touch(k, TouchPhase::End, p[p.len() - 1]));
    }
    h.input_mut().events.push(Event::PointerGone);
    h.run_steps(1);
    h.state().pointer_kind.pointer_up(pointer_type);
    h.run_steps(3);
}

/// Points from `from` to `to` in `steps` equal moves.
fn path(from: Pos2, to: Pos2, steps: usize) -> Vec<Pos2> {
    (0..=steps).map(|k| from + (to - from) * (k as f32 / steps as f32)).collect()
}

#[test]
fn a_finger_scrolls_the_pages_and_gives_the_tool_back() {
    let mut h = harness();
    assert_eq!(h.state().quick_tool, QuickTool::Select);
    let before = page_top(&h);
    // Drag up across the middle of the page, as a finger scrolls down.
    stroke(&mut h, "touch", &path(pos2(512.0, 600.0), pos2(512.0, 250.0), 8));
    let after = page_top(&h);
    assert!(after < before - 200.0, "the pages moved up with the finger: {before} → {after}");
    assert_eq!(h.state().quick_tool, QuickTool::Select, "the Select tool is back once the finger lifts");
}

#[test]
fn with_pencil_only_a_finger_never_draws_with_the_pen() {
    let mut h = harness();
    h.state_mut().pencil_only = pdfcraft_ui_egui::touch::PencilOnly::On;
    assert!(h.state_mut().execute("comment.ink"));
    let (before, notes) = (page_top(&h), annotations(&h));
    stroke(&mut h, "touch", &path(pos2(512.0, 600.0), pos2(512.0, 250.0), 8));
    assert!(page_top(&h) < before - 200.0, "the finger scrolled");
    assert_eq!(annotations(&h), notes, "and drew nothing");
    assert!(matches!(h.state().quick_tool, QuickTool::Comment(_)), "the pen is still chosen");
}

#[test]
fn the_pencil_draws_and_never_scrolls() {
    let mut h = harness();
    assert!(h.state_mut().execute("comment.ink"));
    let (before, notes) = (page_top(&h), annotations(&h));
    // Within the part of page 1 the window shows (the page is taller than the window).
    let page = h.state().views[0].page_screen_rect(0).expect("page 1 on screen");
    let (from, to) = (pos2(page.center().x - 80.0, 400.0), pos2(page.center().x + 80.0, 460.0));
    assert!(page.contains(from) && page.contains(to));
    stroke(&mut h, "pen", &path(from, to, 10));
    assert_eq!(annotations(&h), notes + 1, "the Pencil drew one ink stroke");
    assert!((page_top(&h) - before).abs() < 0.5, "and the page stayed put: {before} → {}", page_top(&h));
}

#[test]
fn a_palm_resting_while_the_pencil_writes_stays_the_pencils() {
    let h = harness();
    let kind = &h.state().pointer_kind;
    kind.pointer_down("pen");
    kind.pointer_down("touch");
    assert_eq!(kind.get(), pdfcraft_ui_egui::touch::Kind::Pen);
    kind.pointer_up("touch");
    kind.pointer_up("pen");
    assert_eq!(kind.get(), pdfcraft_ui_egui::touch::Kind::Mouse);
}

#[test]
fn without_pencil_only_a_finger_draws_and_two_fingers_scroll() {
    let mut h = harness();
    // An iPhone, or an iPad without a Pencil: Automatic is off until a Pencil draws.
    assert!(!h.state().pencil_only_active());
    assert!(h.state_mut().execute("comment.ink"));
    let page = h.state().views[0].page_screen_rect(0).expect("page 1 on screen");
    let (before, notes) = (page_top(&h), annotations(&h));
    let (from, to) = (pos2(page.center().x - 80.0, 400.0), pos2(page.center().x + 80.0, 460.0));
    stroke(&mut h, "touch", &path(from, to, 10));
    assert_eq!(annotations(&h), notes + 1, "the finger drew one ink stroke");
    assert!((page_top(&h) - before).abs() < 0.5, "and the page stayed put");
    // Two fingers dragging up scroll, and draw nothing.
    let a = path(pos2(450.0, 600.0), pos2(450.0, 250.0), 8);
    let b = path(pos2(560.0, 600.0), pos2(560.0, 250.0), 8);
    strokes(&mut h, "touch", &[&a, &b]);
    assert!(page_top(&h) < before - 150.0, "two fingers scrolled: {before} → {}", page_top(&h));
    assert_eq!(annotations(&h), notes + 1, "the pinch drew nothing");
    assert!(matches!(h.state().quick_tool, QuickTool::Comment(_)), "the pen is still chosen");
}

#[test]
fn the_pencil_turns_automatic_pencil_only_on() {
    let mut h = harness();
    assert!(h.state_mut().execute("comment.ink"));
    let page = h.state().views[0].page_screen_rect(0).expect("page 1 on screen");
    stroke(&mut h, "pen", &path(pos2(page.center().x - 60.0, 400.0), pos2(page.center().x + 60.0, 420.0), 8));
    assert!(h.state().pencil_seen && h.state().pencil_only_active());
    // From now on a finger scrolls.
    let (before, notes) = (page_top(&h), annotations(&h));
    stroke(&mut h, "touch", &path(pos2(512.0, 600.0), pos2(512.0, 250.0), 8));
    assert!(page_top(&h) < before - 200.0, "the finger scrolled");
    assert_eq!(annotations(&h), notes);
}

#[test]
fn a_two_finger_tap_undoes_and_draws_nothing() {
    for pencil_only in [pdfcraft_ui_egui::touch::PencilOnly::On, pdfcraft_ui_egui::touch::PencilOnly::Off] {
        let mut h = harness();
        h.state_mut().pencil_only = pencil_only;
        assert!(h.state_mut().execute("comment.ink"));
        let page = h.state().views[0].page_screen_rect(0).expect("page 1 on screen");
        let notes = annotations(&h);
        stroke(&mut h, "pen", &path(pos2(page.center().x - 60.0, 400.0), pos2(page.center().x + 60.0, 420.0), 8));
        assert_eq!(annotations(&h), notes + 1);
        let a = [pos2(450.0, 500.0), pos2(450.0, 500.0)];
        let b = [pos2(560.0, 500.0), pos2(560.0, 500.0)];
        strokes(&mut h, "touch", &[&a, &b]);
        assert_eq!(annotations(&h), notes, "{pencil_only:?}: the two-finger tap undid the stroke and left no dot");
    }
}
