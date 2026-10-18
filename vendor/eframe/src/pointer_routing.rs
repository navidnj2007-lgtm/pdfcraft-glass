//! PdfCraft patch: how the web backend routes a tablet's pointers to egui, where one hand holds a
//! stylus (the Apple Pencil) and the other rests on the screen.
//!
//! Upstream eframe drives the egui pointer from the first touch on the screen, whatever made it,
//! and from every `mousemove`. On an iPad that goes wrong in four ways:
//!
//! - the Pencil hovering over the screen (iPad Pro and Air with M2 and later) sends compatibility
//!   `mousemove` events, so egui saw a pointer moving while nothing touched the screen;
//! - iPadOS cancels the touches it decides are a palm (`touchcancel`, `pointercancel`), and
//!   upstream never released the egui pointer for a cancelled touch: the press stayed down, and
//!   the next hover moved it as a drag, so the Pencil "drew" without touching the screen;
//! - a palm resting before the Pencil came down was the first touch, so the pointer followed the
//!   palm and not the Pencil, and palm plus Pencil made a two-finger pinch;
//! - a second finger pressed (and its lift released) the egui pointer again.
//!
//! Here the Pencil is driven by its pointer events alone: its touches never reach egui, a touch
//! that comes down while it writes is a palm and never reaches egui either, and a palm already
//! down when it comes is cancelled. The hover position is kept apart ([`PEN_HOVER`]) so an app
//! can preview where the Pencil will draw. A cancelled press is released without a click
//! ([`Router`] callers run one frame without a pointer first).
//!
//! The state is plain Rust without web-sys, so it is tested natively (PdfCraft's
//! `apps/pdfcraft-web/tests/pointer_routing.rs`).

/// What made a pointer event (`PointerEvent.pointerType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerType {
    /// A mouse or trackpad; also anything a browser reports that isn't "pen" or "touch".
    #[default]
    Mouse,
    /// A finger (or a palm).
    Touch,
    /// A stylus: the Apple Pencil, a Surface or Wacom pen.
    Pen,
}

impl PointerType {
    /// From `PointerEvent.pointerType`.
    pub fn parse(pointer_type: &str) -> Self {
        match pointer_type {
            "pen" => Self::Pen,
            "touch" => Self::Touch,
            _ => Self::Mouse,
        }
    }
}

/// The egui memory key (`egui::Id::new(PEN_HOVER)`, a temporary `egui::Pos2`) holding where a
/// stylus hovers over the canvas without touching it, in points; absent when it doesn't.
pub const PEN_HOVER: &str = "eframe-pen-hover";

/// Pointers and touches being tracked are capped: a browser that never ends them can't grow the
/// lists without bound.
const MAX_TRACKED: usize = 32;

/// What to tell egui about a `pointerdown`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Down {
    /// As upstream: press the egui pointer here.
    Press,
    /// The stylus came down: cancel the egui touches in `cancel` (a palm resting before it), then
    /// move the egui pointer here and press.
    PenPress {
        /// `Touch.identifier`s egui should stop hearing about.
        cancel: Vec<i32>,
    },
    /// Nothing: a second finger, or a palm while the stylus writes.
    Ignore,
}

/// What to tell egui about a `pointermove`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// The stylus touching the screen moved: move the egui pointer.
    Pen,
    /// The stylus moved above the screen without touching it: only [`PEN_HOVER`] changes.
    Hover,
    /// The stylus is hovering although no `pointerup` said it lifted: release the egui pointer
    /// (as [`Up::PenRelease`]), then it hovers.
    PenLifted,
    /// Nothing (a finger's or a mouse's moves come through `touchmove` and `mousemove`).
    Ignore,
}

/// What to tell egui about a `pointerup` or `pointercancel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Up {
    /// As upstream: release the egui pointer here.
    Release,
    /// The stylus lifted: release the egui pointer and forget its position.
    PenRelease,
    /// The stylus was cancelled: release the egui pointer without a click.
    PenCancel,
    /// Nothing.
    Ignore,
}

/// The phase of a touch event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    /// `touchstart`.
    Start,
    /// `touchmove`.
    Move,
    /// `touchend`.
    End,
    /// `touchcancel`.
    Cancel,
}

/// Pointer and touch routing state for one canvas.
#[derive(Clone, Debug, Default)]
pub struct Router {
    /// The stylus touching the screen (its `pointerId`).
    pen: Option<i32>,
    /// Fingers down (`pointerId`s), palms included.
    fingers: Vec<i32>,
    /// Finger `pointerId`s that came down while the stylus wrote: palms.
    palm_pointers: Vec<i32>,
    /// `Touch.identifier`s egui never hears about: the stylus's own touches and palms.
    ignored: Vec<i32>,
    /// `Touch.identifier`s egui heard start and not yet end.
    forwarded: Vec<i32>,
    /// The type of the last pointer event, which comes before the compatibility mouse events a
    /// browser derives from it.
    last: PointerType,
}

fn insert(list: &mut Vec<i32>, id: i32) {
    if !list.contains(&id) {
        if list.len() >= MAX_TRACKED {
            list.remove(0);
        }
        list.push(id);
    }
}

fn remove(list: &mut Vec<i32>, id: i32) -> bool {
    let before = list.len();
    list.retain(|&x| x != id);
    list.len() != before
}

impl Router {
    /// Whether a stylus is touching the screen.
    pub fn pen_down(&self) -> bool {
        self.pen.is_some()
    }

    /// Whether egui should hear about the touch `identifier` in `touchmove` and `touchend` (and
    /// whether it may be the primary touch that drives the egui pointer).
    pub fn forwards(&self, identifier: i32) -> bool {
        self.forwarded.contains(&identifier)
    }

    /// A `pointerdown` of `kind` with `pointerId` `id`.
    pub fn pointer_down(&mut self, kind: PointerType, id: i32) -> Down {
        self.last = kind;
        match kind {
            PointerType::Mouse => Down::Press,
            // A second `pointerdown` without a `pointerup` between them presses again: a lost
            // `pointerup` must not leave the stylus down for good.
            PointerType::Pen => {
                self.pen = Some(id);
                // Whatever touches are down now are the writing hand's: egui stops hearing them.
                let cancel = std::mem::take(&mut self.forwarded);
                for &touch in &cancel {
                    insert(&mut self.ignored, touch);
                }
                for &finger in &self.fingers {
                    insert(&mut self.palm_pointers, finger);
                }
                Down::PenPress { cancel }
            }
            PointerType::Touch => {
                let first = !self.any_finger();
                insert(&mut self.fingers, id);
                if self.pen.is_some() {
                    insert(&mut self.palm_pointers, id);
                    Down::Ignore
                } else if first {
                    Down::Press
                } else {
                    Down::Ignore
                }
            }
        }
    }

    /// A `pointermove` of `kind` with `pointerId` `id`; `buttons` is `PointerEvent.buttons`.
    pub fn pointer_move(&mut self, kind: PointerType, id: i32, buttons: u16) -> Move {
        self.last = kind;
        match kind {
            PointerType::Pen if self.pen == Some(id) && buttons == 0 => {
                self.pen = None;
                Move::PenLifted
            }
            PointerType::Pen if self.pen == Some(id) => Move::Pen,
            PointerType::Pen if self.pen.is_none() && buttons == 0 => Move::Hover,
            _ => Move::Ignore,
        }
    }

    /// Whether a `mousemove` should move the egui pointer: not when it is the browser's
    /// compatibility event for a stylus or finger (the Pencil hovering sends them), nor while the
    /// stylus writes.
    pub fn mouse_move(&self) -> bool {
        self.last == PointerType::Mouse && self.pen.is_none()
    }

    /// A `pointerup` of `kind` with `pointerId` `id`.
    pub fn pointer_up(&mut self, kind: PointerType, id: i32) -> Up {
        self.last = kind;
        self.lift(kind, id, false)
    }

    /// A `pointercancel` of `kind` with `pointerId` `id`.
    pub fn pointer_cancel(&mut self, kind: PointerType, id: i32) -> Up {
        self.last = kind;
        self.lift(kind, id, true)
    }

    fn lift(&mut self, kind: PointerType, id: i32, cancelled: bool) -> Up {
        match kind {
            PointerType::Mouse if cancelled => Up::Ignore,
            PointerType::Mouse => Up::Release,
            PointerType::Pen if self.pen == Some(id) => {
                self.pen = None;
                if cancelled { Up::PenCancel } else { Up::PenRelease }
            }
            PointerType::Pen => Up::Ignore,
            PointerType::Touch => {
                let known = remove(&mut self.fingers, id);
                let palm = remove(&mut self.palm_pointers, id);
                // The last finger lifting releases the pointer (and runs the app while the
                // browser allows downloads and the share sheet). A cancelled finger is released
                // by `touchcancel`, without a click.
                if known && !palm && !cancelled && !self.any_finger() && self.pen.is_none() { Up::Release } else { Up::Ignore }
            }
        }
    }

    /// Whether a finger that isn't a palm is down.
    fn any_finger(&self) -> bool {
        self.fingers.iter().any(|f| !self.palm_pointers.contains(f))
    }

    /// A touch event left no touch on the screen (`TouchEvent.touches` is empty): forget every
    /// finger and touch, in case the browser never ended one.
    pub fn no_touches(&mut self) {
        self.fingers.clear();
        self.palm_pointers.clear();
        self.ignored.clear();
        self.forwarded.clear();
    }

    /// The changed touches of a touch event, as `(identifier, is_stylus)`; returns the ones egui
    /// should hear about. A stylus's touches and any touch starting while the stylus writes are
    /// left out until they end.
    pub fn touches(&mut self, phase: TouchPhase, changed: &[(i32, bool)]) -> Vec<i32> {
        let mut out = Vec::new();
        for &(id, stylus) in changed {
            match phase {
                TouchPhase::Start => {
                    if stylus || self.pen.is_some() || self.ignored.contains(&id) {
                        insert(&mut self.ignored, id);
                    } else {
                        insert(&mut self.forwarded, id);
                        out.push(id);
                    }
                }
                TouchPhase::Move => {
                    if self.forwarded.contains(&id) {
                        out.push(id);
                    }
                }
                TouchPhase::End | TouchPhase::Cancel => {
                    remove(&mut self.ignored, id);
                    if remove(&mut self.forwarded, id) {
                        out.push(id);
                    }
                }
            }
        }
        out
    }

    /// Whether a Safari `gesturestart`/`gesturechange` (a two-touch pinch) should zoom: not when
    /// one of its touches is the stylus or a palm, which egui doesn't hear about.
    pub fn gesture(&self) -> bool {
        self.pen.is_none() && self.ignored.is_empty()
    }
}
