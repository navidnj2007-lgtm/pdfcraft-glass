//! Fingers and Apple Pencil on a tablet, as iPad apps treat them: a finger pans and pinch-zooms
//! the pages whatever tool is chosen, and the Pencil uses the tool (draws, highlights, selects)
//! and never scrolls. A palm resting on the screen while the Pencil is down stays the Pencil's.
//!
//! The browser tells a Pencil from a finger (`PointerEvent.pointerType` is "pen" or "touch"),
//! but egui sees both as touches. The web app reports each pointer's kind through
//! [`PointerKind`]; [`PdfCraftApp::touch_tools`] turns a finger on the page into a temporary
//! Hand, as holding Space does.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::{PdfCraftApp, QuickTool};

/// What is pressing the screen now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A mouse or trackpad, or nothing reported (desktop builds).
    Mouse,
    Finger,
    Pen,
}

impl Kind {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Kind::Finger,
            2 => Kind::Pen,
            _ => Kind::Mouse,
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            Kind::Mouse => 0,
            Kind::Finger => 1,
            Kind::Pen => 2,
        }
    }
}

/// The kind of the pointer pressing the screen, shared with the page's event listeners.
#[derive(Clone, Debug, Default)]
pub struct PointerKind(Arc<AtomicU8>);

impl PointerKind {
    pub fn get(&self) -> Kind {
        Kind::from_u8(self.0.load(Ordering::Relaxed))
    }

    /// A pointer went down; `pointer_type` is the browser's ("mouse", "pen" or "touch").
    pub fn pointer_down(&self, pointer_type: &str) {
        self.0.store(after_down(self.get(), pointer_type).to_u8(), Ordering::Relaxed);
    }

    /// A pointer went up or was cancelled.
    pub fn pointer_up(&self, pointer_type: &str) {
        self.0.store(after_up(self.get(), pointer_type).to_u8(), Ordering::Relaxed);
    }
}

/// The kind after a pointer of `pointer_type` goes down. A touch while the Pencil is down is the
/// writing hand's palm: it stays the Pencil's.
fn after_down(current: Kind, pointer_type: &str) -> Kind {
    match pointer_type {
        "pen" => Kind::Pen,
        "touch" if current == Kind::Pen => Kind::Pen,
        "touch" => Kind::Finger,
        _ => Kind::Mouse,
    }
}

/// The kind after a pointer of `pointer_type` goes up: lifting the Pencil or a finger ends its
/// kind; a palm lifted while the Pencil writes changes nothing.
fn after_up(current: Kind, pointer_type: &str) -> Kind {
    match (current, pointer_type) {
        (Kind::Pen, "pen") | (Kind::Finger, "touch") => Kind::Mouse,
        (current, _) => current,
    }
}

/// What the temporary Hand should do this frame: `Some(true)` take it, `Some(false)` give the
/// tool back, `None` nothing.
pub(crate) fn finger_hand_step(holding: bool, touching: bool, kind: Kind, on_page: bool, tool_is_hand: bool) -> Option<bool> {
    match (holding, touching) {
        (true, false) => Some(false),
        (false, true) if kind == Kind::Finger && on_page && !tool_is_hand => Some(true),
        _ => None,
    }
}

impl PdfCraftApp {
    /// A finger on the page pans it (a temporary Hand, given back when every finger lifts); the
    /// Pencil keeps the chosen tool. `page` is where the pages are shown on screen.
    pub(crate) fn touch_tools(&mut self, ctx: &egui::Context, page: egui::Rect) {
        let (touching, at) = ctx.input(|i| (i.any_touches(), i.pointer.press_origin().or(i.pointer.latest_pos())));
        let on_page = at.is_some_and(|p| page.contains(p));
        let step = finger_hand_step(self.finger_hand.is_some(), touching, self.pointer_kind.get(), on_page, self.quick_tool == QuickTool::Hand);
        match step {
            Some(true) => {
                self.finger_hand = Some(self.quick_tool);
                self.quick_tool = QuickTool::Hand;
            }
            Some(false) => {
                if let Some(previous) = self.finger_hand.take()
                    && self.quick_tool == QuickTool::Hand
                {
                    self.quick_tool = previous;
                }
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pencil_keeps_its_kind_through_a_resting_palm() {
        assert_eq!(after_down(Kind::Mouse, "pen"), Kind::Pen);
        assert_eq!(after_down(Kind::Mouse, "touch"), Kind::Finger);
        assert_eq!(after_down(Kind::Pen, "touch"), Kind::Pen, "a palm while writing");
        assert_eq!(after_up(Kind::Pen, "touch"), Kind::Pen, "the palm lifts, the Pencil writes on");
        assert_eq!(after_up(Kind::Pen, "pen"), Kind::Mouse);
        assert_eq!(after_down(Kind::Finger, "pen"), Kind::Pen, "the Pencil takes over from a finger");
        assert_eq!(after_up(Kind::Finger, "touch"), Kind::Mouse);
        assert_eq!(after_down(Kind::Pen, "mouse"), Kind::Mouse);
        // Anything a browser might invent is a mouse.
        assert_eq!(after_down(Kind::Finger, ""), Kind::Mouse);
        let shared = PointerKind::default();
        shared.pointer_down("pen");
        assert_eq!(shared.clone().get(), Kind::Pen, "the listeners and the app share one cell");
    }

    #[test]
    fn a_finger_on_the_page_pans_until_it_lifts() {
        // A finger touches the page: take the Hand.
        assert_eq!(finger_hand_step(false, true, Kind::Finger, true, false), Some(true));
        // The Pencil, a mouse, a touch off the page, or the Hand already chosen: nothing.
        assert_eq!(finger_hand_step(false, true, Kind::Pen, true, false), None);
        assert_eq!(finger_hand_step(false, true, Kind::Mouse, true, false), None);
        assert_eq!(finger_hand_step(false, true, Kind::Finger, false, false), None);
        assert_eq!(finger_hand_step(false, true, Kind::Finger, true, true), None);
        // Held while touching; given back once every finger lifts.
        assert_eq!(finger_hand_step(true, true, Kind::Finger, true, true), None);
        assert_eq!(finger_hand_step(true, false, Kind::Mouse, false, true), Some(false));
    }

    #[test]
    fn a_finger_hand_is_given_back_to_the_chosen_tool() {
        let mut app = PdfCraftApp::new();
        app.quick_tool = QuickTool::Comment(crate::comments::CommentTool::Ink);
        app.pointer_kind.pointer_down("touch");
        app.finger_hand = Some(app.quick_tool);
        app.quick_tool = QuickTool::Hand;
        // A frame with no touches gives the tool back.
        let ctx = egui::Context::default();
        let page = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        ctx.run_ui(egui::RawInput::default(), |ui| app.touch_tools(ui.ctx(), page)).textures_delta.clear();
        assert_eq!(app.quick_tool, QuickTool::Comment(crate::comments::CommentTool::Ink));
        assert!(app.finger_hand.is_none());
    }
}
