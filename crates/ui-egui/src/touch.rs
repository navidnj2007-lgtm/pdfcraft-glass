//! Fingers and Apple Pencil on a tablet, as iPad apps treat them. The Pencil always uses the
//! chosen tool and never scrolls. A finger depends on the tool ([`finger_role`]):
//!
//! - with Select, the Hand and the tools placed with a tap (notes, text boxes, stamps, Fill &
//!   Sign), a tap reaches the tool and a drag pans the pages;
//! - with the tools that mark the page (pens, shapes, highlighters, the eraser), a finger pans
//!   while **Draw with Apple Pencil only** is on, and draws like the Pencil while it is off;
//! - with the other drag tools (crop, redact, link areas…) one finger uses the tool.
//!
//! Wherever a finger uses the tool, a second finger turns the gesture into panning and pinch
//! zooming instead. Draw with Apple Pencil only is Automatic by default ([`PencilOnly`]): off
//! until a Pencil touches the screen (an iPhone, an iPad without one), then on, as in Apple Notes.
//!
//! The browser tells a Pencil from a finger (`PointerEvent.pointerType` is "pen" or "touch"),
//! but egui sees both as touches. The web app reports each pointer's kind through
//! [`PointerKind`]; [`PdfCraftApp::touch_tools`] turns a finger on the page into a temporary
//! Hand, as holding Space does. The web backend (the vendored eframe's `pointer_routing`) keeps a
//! palm resting while the Pencil writes away from egui altogether.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

use crate::comments::CommentTool;
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

/// View ▸ Draw with Apple Pencil Only (and Preferences): whether a finger may mark the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PencilOnly {
    /// Off until an Apple Pencil (any stylus) touches the screen, then on.
    #[default]
    Automatic,
    On,
    Off,
}

impl PencilOnly {
    pub const ALL: [Self; 3] = [Self::Automatic, Self::On, Self::Off];

    /// The choice's name in Preferences and the View menu, as the answer to "Draw with Apple
    /// Pencil only?" (not On and Off, which Animation effects beside it already uses).
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => tl!("Auto"),
            Self::On => tl!("Yes"),
            Self::Off => tl!("No"),
        }
    }

    /// The setting's value names (`set_option pencil_only …`, the settings file).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "automatic" | "auto" => Some(Self::Automatic),
            "on" | "true" => Some(Self::On),
            "off" | "false" => Some(Self::Off),
            _ => None,
        }
    }

    /// Whether only the Pencil marks the page now; `pencil_seen` is whether one has been used.
    pub fn active(self, pencil_seen: bool) -> bool {
        match self {
            Self::Automatic => pencil_seen,
            Self::On => true,
            Self::Off => false,
        }
    }
}

/// What a finger on the page does with the chosen tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FingerRole {
    /// Pans at once, and a tap does nothing: the tool marks the page, which only the Pencil does.
    Pan,
    /// A tap reaches the tool; a drag pans.
    TapOrPan,
    /// One finger uses the tool like the Pencil; a second finger pans and zooms instead.
    Tool,
}

/// What a finger does with `tool`; `pencil_only` is whether only the Pencil marks the page.
pub(crate) fn finger_role(tool: QuickTool, pencil_only: bool) -> FingerRole {
    let marks = if pencil_only { FingerRole::Pan } else { FingerRole::Tool };
    match tool {
        QuickTool::Comment(CommentTool::Note | CommentTool::TextBox | CommentTool::Callout | CommentTool::Attach) => FingerRole::TapOrPan,
        QuickTool::Comment(_) => marks,
        QuickTool::Select
        | QuickTool::Hand
        | QuickTool::Fill(_)
        | QuickTool::Stamp(_)
        | QuickTool::CustomStamp(_)
        | QuickTool::AddText
        | QuickTool::EditText
        | QuickTool::Measure(_) => FingerRole::TapOrPan,
        QuickTool::Crop
        | QuickTool::EnhanceCamera
        | QuickTool::Field(_)
        | QuickTool::Redact
        | QuickTool::Link
        | QuickTool::SignArea { .. }
        | QuickTool::MarqueeZoom
        | QuickTool::Snapshot
        | QuickTool::ColumnSelect => FingerRole::Tool,
    }
}

/// One frame's touch input, for [`finger_hand_step`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fingers {
    /// A temporary Hand is held for a finger.
    pub holding: bool,
    /// Anything touches the screen (as egui hears it).
    pub touching: bool,
    pub kind: Kind,
    /// The press began on the pages.
    pub on_page: bool,
    pub tool_is_hand: bool,
    pub role: FingerRole,
    /// The press has become a drag (moved, or held long enough).
    pub dragging: bool,
    /// How many touches egui hears about.
    pub count: usize,
}

/// What the temporary Hand should do this frame: `Some(true)` take it, `Some(false)` give the
/// tool back, `None` nothing.
pub(crate) fn finger_hand_step(f: Fingers) -> Option<bool> {
    match (f.holding, f.touching) {
        (true, false) => Some(false),
        (false, true) if f.kind == Kind::Finger && f.on_page && !f.tool_is_hand => {
            let take = match f.role {
                FingerRole::Pan => true,
                FingerRole::TapOrPan => f.dragging || f.count >= 2,
                FingerRole::Tool => f.count >= 2,
            };
            take.then_some(true)
        }
        _ => None,
    }
}

/// A tap with two fingers undoes and with three redoes, as in iPad note-taking apps: the fingers
/// come down and lift within [`TAP_TIME`] seconds without moving or pinching.
pub(crate) const TAP_TIME: f64 = 0.35;

/// How far (points, and zoom × 100) the fingers may move for a tap.
const TAP_SLOP: f32 = 12.0;

/// A multi-finger tap in progress.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct MultiTap {
    start: f64,
    fingers: usize,
    moved: f32,
}

/// What a frame's touches mean for a multi-finger tap: the tap state after it, and the command
/// a finished tap runs (`edit.undo` for two fingers, `edit.redo` for three).
pub(crate) fn multi_tap_step(state: Option<MultiTap>, now: f64, count: usize, motion: f32) -> (Option<MultiTap>, Option<&'static str>) {
    match (state, count) {
        (None, 0 | 1) => (None, None),
        (None, n) => (Some(MultiTap { start: now, fingers: n, moved: 0.0 }), None),
        (Some(tap), 0) => {
            let command = (now - tap.start <= TAP_TIME && tap.moved <= TAP_SLOP)
                .then_some(match tap.fingers {
                    2 => Some("edit.undo"),
                    3 => Some("edit.redo"),
                    _ => None,
                })
                .flatten();
            (None, command)
        }
        (Some(tap), n) => {
            let moved = tap.moved + if motion.is_finite() { motion } else { TAP_SLOP + 1.0 };
            (Some(MultiTap { fingers: tap.fingers.max(n), moved, ..tap }), None)
        }
    }
}

impl PdfCraftApp {
    /// Whether only the Pencil marks the page now (Draw with Apple Pencil only).
    pub fn pencil_only_active(&self) -> bool {
        self.pencil_only.active(self.pencil_seen)
    }

    /// A finger on the page pans it (a temporary Hand, given back when every finger lifts), as
    /// [`finger_role`] says for the tool; the Pencil keeps the chosen tool. `page` is where the
    /// pages are shown on screen, `index` the view showing them.
    pub(crate) fn touch_tools(&mut self, ctx: &egui::Context, page: egui::Rect, index: usize) {
        let kind = self.pointer_kind.get();
        if kind == Kind::Pen && !self.pencil_seen {
            // Automatic: from the first Pencil stroke on, a finger no longer draws.
            self.pencil_seen = true;
        }
        let drag_started = ctx.drag_started_id().is_some();
        let (touching, at, dragging, count) = ctx.input(|i| {
            (
                i.any_touches(),
                i.pointer.press_origin().or(i.pointer.latest_pos()),
                i.pointer.is_decidedly_dragging() || drag_started,
                i.multi_touch().map_or(usize::from(i.any_touches()), |m| m.num_touches),
            )
        });
        // Two fingers tapped undo, three redo (only fingers on the page; the Pencil never taps).
        let (now, motion) =
            ctx.input(|i| (i.time, i.multi_touch().map_or(0.0, |m| m.translation_delta.length() + (m.zoom_delta - 1.0).abs() * 100.0)));
        let on_page = at.is_some_and(|p| page.contains(p));
        let (tap, command) = multi_tap_step(self.multi_tap, now, if on_page && kind == Kind::Finger { count } else { 0 }, motion);
        self.multi_tap = tap;
        if let Some(command) = command {
            self.execute(command);
        }
        let role = finger_role(self.quick_tool, self.pencil_only_active());
        let step = finger_hand_step(Fingers {
            holding: self.finger_hand.is_some(),
            touching,
            kind,
            on_page,
            tool_is_hand: self.quick_tool == QuickTool::Hand,
            role,
            dragging,
            count,
        });
        match step {
            Some(true) => {
                if role == FingerRole::Tool
                    && let Some(view) = self.views.get_mut(index)
                {
                    // The first finger may have begun a stroke: a pinch is no stroke.
                    view.comments.cancel_gesture();
                }
                self.finger_hand = Some(self.quick_tool);
                self.quick_tool = QuickTool::Hand;
            }
            // egui still counts touches in the frame they lift, so their release is never a
            // click with the tool given back (a dot of ink, a note).
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

    fn finger(role: FingerRole) -> Fingers {
        Fingers { holding: false, touching: true, kind: Kind::Finger, on_page: true, tool_is_hand: false, role, dragging: false, count: 1 }
    }

    #[test]
    fn a_finger_on_the_page_pans_until_it_lifts() {
        // A finger touches the page with a pen while only the Pencil draws: take the Hand at once.
        assert_eq!(finger_hand_step(finger(FingerRole::Pan)), Some(true));
        // The Pencil, a mouse, a touch off the page, or the Hand already chosen: nothing.
        assert_eq!(finger_hand_step(Fingers { kind: Kind::Pen, ..finger(FingerRole::Pan) }), None);
        assert_eq!(finger_hand_step(Fingers { kind: Kind::Mouse, ..finger(FingerRole::Pan) }), None);
        assert_eq!(finger_hand_step(Fingers { on_page: false, ..finger(FingerRole::Pan) }), None);
        assert_eq!(finger_hand_step(Fingers { tool_is_hand: true, ..finger(FingerRole::Pan) }), None);
        // Held while touching; given back once every finger lifts.
        assert_eq!(finger_hand_step(Fingers { holding: true, tool_is_hand: true, ..finger(FingerRole::Pan) }), None);
        let lifted = Fingers { holding: true, touching: false, kind: Kind::Mouse, on_page: false, tool_is_hand: true, ..finger(FingerRole::Pan) };
        assert_eq!(finger_hand_step(lifted), Some(false));
    }

    #[test]
    fn a_tap_reaches_the_tool_and_a_drag_pans() {
        // A finger down on the page with Select or a note: the tool keeps it while it may be a tap.
        assert_eq!(finger_hand_step(finger(FingerRole::TapOrPan)), None);
        // Once it moves (or a second finger comes), it pans.
        assert_eq!(finger_hand_step(Fingers { dragging: true, ..finger(FingerRole::TapOrPan) }), Some(true));
        assert_eq!(finger_hand_step(Fingers { count: 2, ..finger(FingerRole::TapOrPan) }), Some(true));
    }

    #[test]
    fn a_drawing_finger_pans_with_a_second_finger() {
        // Draw with Apple Pencil only is off: one finger draws, even as it moves.
        assert_eq!(finger_hand_step(Fingers { dragging: true, ..finger(FingerRole::Tool) }), None);
        // A second finger pans and zooms instead.
        assert_eq!(finger_hand_step(Fingers { dragging: true, count: 2, ..finger(FingerRole::Tool) }), Some(true));
    }

    #[test]
    fn which_tools_a_finger_uses() {
        let ink = QuickTool::Comment(CommentTool::Ink);
        assert_eq!(finger_role(ink, true), FingerRole::Pan, "only the Pencil draws");
        assert_eq!(finger_role(ink, false), FingerRole::Tool, "a finger draws");
        assert_eq!(finger_role(QuickTool::Comment(CommentTool::Highlight), true), FingerRole::Pan);
        assert_eq!(finger_role(QuickTool::Comment(CommentTool::Eraser), false), FingerRole::Tool);
        // Taps place notes and select; Pencil only or not, a drag pans.
        for tool in [QuickTool::Select, QuickTool::Comment(CommentTool::Note), QuickTool::Fill(crate::fill_sign::FillTool::Text)] {
            assert_eq!(finger_role(tool, true), FingerRole::TapOrPan);
            assert_eq!(finger_role(tool, false), FingerRole::TapOrPan);
        }
        // Tools that work by dragging a box work with a finger.
        assert_eq!(finger_role(QuickTool::Crop, true), FingerRole::Tool);
    }

    #[test]
    fn two_fingers_tapped_undo_and_three_redo() {
        // Two fingers down, still, and up within the time: undo.
        let (s, c) = multi_tap_step(None, 1.0, 2, 0.0);
        assert!(s.is_some() && c.is_none());
        let (s, c) = multi_tap_step(s, 1.1, 2, 1.0);
        assert!(c.is_none());
        assert_eq!(multi_tap_step(s, 1.2, 0, 0.0), (None, Some("edit.undo")));
        // A third finger joining makes it redo.
        let (s, _) = multi_tap_step(None, 1.0, 2, 0.0);
        let (s, _) = multi_tap_step(s, 1.05, 3, 0.0);
        assert_eq!(multi_tap_step(s, 1.2, 0, 0.0).1, Some("edit.redo"));
        // A pinch or a pan moves too far; a slow press is held too long; one finger never taps.
        let (s, _) = multi_tap_step(None, 1.0, 2, 0.0);
        let (s, _) = multi_tap_step(s, 1.1, 2, 30.0);
        assert_eq!(multi_tap_step(s, 1.2, 0, 0.0).1, None);
        let (s, _) = multi_tap_step(None, 1.0, 2, 0.0);
        assert_eq!(multi_tap_step(s, 1.0 + TAP_TIME + 0.1, 0, 0.0).1, None);
        assert_eq!(multi_tap_step(None, 1.0, 1, 0.0), (None, None));
        // Nonsense motion is a move, never a tap.
        let (s, _) = multi_tap_step(None, 1.0, 2, 0.0);
        let (s, _) = multi_tap_step(s, 1.1, 2, f32::NAN);
        assert_eq!(multi_tap_step(s, 1.2, 0, 0.0).1, None);
    }

    #[test]
    fn pencil_only_is_automatic_until_chosen() {
        assert_eq!(PencilOnly::default(), PencilOnly::Automatic);
        assert!(!PencilOnly::Automatic.active(false), "no Pencil yet (an iPhone): a finger draws");
        assert!(PencilOnly::Automatic.active(true), "once a Pencil has drawn, only it draws");
        assert!(PencilOnly::On.active(false));
        assert!(!PencilOnly::Off.active(true));
        assert_eq!(PencilOnly::parse("on"), Some(PencilOnly::On));
        assert_eq!(PencilOnly::parse("auto"), Some(PencilOnly::Automatic));
        assert_eq!(PencilOnly::parse("sometimes"), None);
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
        ctx.run_ui(egui::RawInput::default(), |ui| app.touch_tools(ui.ctx(), page, 0)).textures_delta.clear();
        assert_eq!(app.quick_tool, QuickTool::Comment(crate::comments::CommentTool::Ink));
        assert!(app.finger_hand.is_none());
    }

    #[test]
    fn the_pencil_turns_automatic_pencil_only_on() {
        let mut app = PdfCraftApp::new();
        assert!(!app.pencil_only_active(), "no Pencil seen yet");
        app.pointer_kind.pointer_down("pen");
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |ui| app.touch_tools(ui.ctx(), egui::Rect::NOTHING, 0)).textures_delta.clear();
        assert!(app.pencil_seen && app.pencil_only_active());
        app.pencil_only = PencilOnly::Off;
        assert!(!app.pencil_only_active(), "the user's choice wins");
    }
}
