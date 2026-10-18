//! The web backend's routing of a tablet's pointers (the vendored eframe's `pointer_routing`,
//! a PdfCraft patch): the event sequences Safari sends on an iPad with the Apple Pencil, and what
//! egui may hear of them.

use eframe::pointer_routing::{Down, Move, PointerType, Router, TouchPhase, Up};

const PEN: PointerType = PointerType::Pen;
const FINGER: PointerType = PointerType::Touch;
const MOUSE: PointerType = PointerType::Mouse;

#[test]
fn pointer_types_parse_as_browsers_report_them() {
    assert_eq!(PointerType::parse("pen"), PEN);
    assert_eq!(PointerType::parse("touch"), FINGER);
    assert_eq!(PointerType::parse("mouse"), MOUSE);
    // Anything a browser might invent is a mouse.
    assert_eq!(PointerType::parse(""), MOUSE);
}

#[test]
fn the_pencil_hovering_never_moves_the_egui_pointer() {
    let mut r = Router::default();
    // The Pencil above the screen: pointermove with no buttons, then the compatibility mousemove.
    for _ in 0..5 {
        assert_eq!(r.pointer_move(PEN, 7, 0), Move::Hover);
        assert!(!r.mouse_move(), "its compatibility mousemove is dropped");
    }
    // A trackpad on the same iPad still moves the pointer.
    assert_eq!(r.pointer_move(MOUSE, 1, 0), Move::Ignore);
    assert!(r.mouse_move());
}

#[test]
fn the_pencil_writes_through_its_pointer_events_and_lifts_cleanly() {
    let mut r = Router::default();
    assert_eq!(r.pointer_down(PEN, 7), Down::PenPress { cancel: vec![] });
    // Safari's touch for the Pencil (touchType "stylus") is never egui's.
    assert_eq!(r.touches(TouchPhase::Start, &[(100, true)]), Vec::<i32>::new());
    assert_eq!(r.pointer_move(PEN, 7, 1), Move::Pen);
    assert_eq!(r.touches(TouchPhase::Move, &[(100, true)]), Vec::<i32>::new());
    assert!(!r.mouse_move(), "no mousemove moves the pointer while the Pencil writes");
    assert_eq!(r.pointer_up(PEN, 7), Up::PenRelease);
    assert_eq!(r.touches(TouchPhase::End, &[(100, true)]), Vec::<i32>::new());
    assert!(!r.pen_down());
    // And hovering again afterwards is only a hover.
    assert_eq!(r.pointer_move(PEN, 8, 0), Move::Hover);
}

#[test]
fn a_cancelled_pencil_is_released() {
    let mut r = Router::default();
    r.pointer_down(PEN, 7);
    assert_eq!(r.pointer_cancel(PEN, 7), Up::PenCancel);
    assert!(!r.pen_down(), "the press doesn't stay down for the next hover to drag");
    assert_eq!(r.pointer_move(PEN, 7, 0), Move::Hover);
}

#[test]
fn a_lost_pointerup_doesnt_leave_the_pencil_down() {
    let mut r = Router::default();
    r.pointer_down(PEN, 7);
    // The Pencil hovers again without a pointerup: release it.
    assert_eq!(r.pointer_move(PEN, 7, 0), Move::PenLifted);
    assert!(!r.pen_down());
    assert_eq!(r.pointer_move(PEN, 7, 0), Move::Hover);
    // A second pointerdown without an up in between presses again, rather than being ignored.
    r.pointer_down(PEN, 8);
    assert_eq!(r.pointer_down(PEN, 9), Down::PenPress { cancel: vec![] });
    assert_eq!(r.pointer_move(PEN, 9, 1), Move::Pen);
}

#[test]
fn a_palm_resting_while_the_pencil_writes_is_never_egui_s() {
    let mut r = Router::default();
    r.pointer_down(PEN, 7);
    r.touches(TouchPhase::Start, &[(100, true)]);
    // The palm comes down.
    assert_eq!(r.pointer_down(FINGER, 20), Down::Ignore);
    assert_eq!(r.touches(TouchPhase::Start, &[(101, false)]), Vec::<i32>::new());
    assert!(!r.forwards(101), "it can't become the primary touch that drives the pointer");
    assert!(!r.gesture(), "Pencil and palm are no pinch");
    assert_eq!(r.touches(TouchPhase::Move, &[(101, false)]), Vec::<i32>::new());
    // The Pencil lifts first; the palm stays a palm until it lifts.
    assert_eq!(r.pointer_up(PEN, 7), Up::PenRelease);
    assert_eq!(r.touches(TouchPhase::End, &[(100, true)]), Vec::<i32>::new());
    assert!(!r.gesture(), "the palm still down is no pinch either");
    assert_eq!(r.touches(TouchPhase::Move, &[(101, false)]), Vec::<i32>::new());
    assert_eq!(r.pointer_up(FINGER, 20), Up::Ignore, "a palm lifting releases nothing");
    assert_eq!(r.touches(TouchPhase::End, &[(101, false)]), Vec::<i32>::new());
    assert!(r.gesture());
}

#[test]
fn a_palm_already_down_is_cancelled_when_the_pencil_comes() {
    let mut r = Router::default();
    // The writing hand rests first: a finger, as far as anyone can tell.
    assert_eq!(r.pointer_down(FINGER, 20), Down::Press);
    assert_eq!(r.touches(TouchPhase::Start, &[(101, false)]), vec![101]);
    assert!(r.forwards(101));
    // The Pencil comes down: egui stops hearing the palm, and the Pencil presses.
    assert_eq!(r.pointer_down(PEN, 7), Down::PenPress { cancel: vec![101] });
    assert!(!r.forwards(101));
    assert_eq!(r.touches(TouchPhase::Move, &[(101, false)]), Vec::<i32>::new());
    assert_eq!(r.touches(TouchPhase::End, &[(101, false)]), Vec::<i32>::new());
    assert_eq!(r.pointer_up(FINGER, 20), Up::Ignore, "the palm lifting doesn't release the Pencil");
    assert_eq!(r.pointer_move(PEN, 7, 1), Move::Pen);
    assert_eq!(r.pointer_up(PEN, 7), Up::PenRelease);
}

#[test]
fn a_cancelled_touch_ends_without_staying_pressed() {
    let mut r = Router::default();
    assert_eq!(r.pointer_down(FINGER, 20), Down::Press);
    assert_eq!(r.touches(TouchPhase::Start, &[(101, false)]), vec![101]);
    // iPadOS decides it was a palm: both cancels arrive.
    assert_eq!(r.pointer_cancel(FINGER, 20), Up::Ignore, "touchcancel releases it, without a click");
    assert_eq!(r.touches(TouchPhase::Cancel, &[(101, false)]), vec![101], "egui hears the cancel");
    r.no_touches();
    assert!(!r.forwards(101));
    // The Pencil hovering afterwards is a hover, not a drag of a stuck press.
    assert_eq!(r.pointer_move(PEN, 7, 0), Move::Hover);
    assert!(!r.mouse_move());
}

#[test]
fn only_the_first_finger_presses_and_the_last_one_releases() {
    let mut r = Router::default();
    assert_eq!(r.pointer_down(FINGER, 1), Down::Press);
    assert_eq!(r.touches(TouchPhase::Start, &[(11, false)]), vec![11]);
    // A second finger for a pinch.
    assert_eq!(r.pointer_down(FINGER, 2), Down::Ignore);
    assert_eq!(r.touches(TouchPhase::Start, &[(12, false)]), vec![12], "egui's pinch hears both");
    assert!(r.gesture());
    assert_eq!(r.pointer_up(FINGER, 2), Up::Ignore);
    assert_eq!(r.pointer_up(FINGER, 1), Up::Release);
    // Fingers' compatibility mousemoves never move the pointer.
    assert!(!r.mouse_move());
}

#[test]
fn stylus_touches_are_never_forwarded_even_before_the_pointer_event() {
    let mut r = Router::default();
    // Some browser might send the stylus's touchstart before its pointerdown.
    assert_eq!(r.touches(TouchPhase::Start, &[(100, true)]), Vec::<i32>::new());
    assert_eq!(r.pointer_down(PEN, 7), Down::PenPress { cancel: vec![] });
}

#[test]
fn tracking_is_bounded() {
    let mut r = Router::default();
    // A browser that starts touches and never ends them can't grow the lists without bound.
    for id in 0..1000 {
        r.pointer_down(FINGER, id);
        r.touches(TouchPhase::Start, &[(id, false)]);
    }
    assert!(!r.forwards(0), "the oldest touches are forgotten");
    assert!(r.forwards(999));
    r.no_touches();
    assert!(!r.forwards(999));
    // Everything is forgotten once no touch is left, and the next finger presses again.
    assert_eq!(r.pointer_down(FINGER, 5000), Down::Press);
}
