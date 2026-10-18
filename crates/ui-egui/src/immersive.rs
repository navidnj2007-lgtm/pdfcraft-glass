//! Immersive layout (View ▸ Immersive layout), as iPad and iPhone apps present a document: the
//! page fills the window and the controls float over it on glass capsules instead of sitting
//! in bars and docked panels around it.
//!
//! - top left: back to Home, the tools (a floating sheet), and the document with its switcher;
//! - top centre (bottom centre on a phone): the tools palette (`canvas::quick_bar`);
//! - top right: find, save, print and ⋯ (the main menu);
//! - right edge: the side panels, each opening as a floating sheet (`docking::show_compact`);
//! - bottom left: undo and redo; bottom right: page and zoom.

use egui::{Align2, Rect, vec2};

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, RightPanel, icons};

/// Height kept clear at the top of the document area for the top capsules: notices and the
/// first page start below them.
pub(crate) const TOP_RESERVE: f32 = 2.0 * EDGE + BUTTON + 2.0 * PAD as f32 + 4.0;

/// Room the right-edge capsule takes from a sheet floating beside it.
pub(crate) const SIDE_CLEARANCE: f32 = 60.0;

/// Below this width (points) the layout is a phone's: the tools go to the bottom and the corner
/// capsules that would collide with them are left out (their commands stay in ⋯).
const NARROW: f32 = 900.0;

/// Side of a capsule's buttons, its padding and its distance from the window's edges.
const BUTTON: f32 = 36.0;
const PAD: i8 = 5;
const EDGE: f32 = 12.0;

/// Whether the immersive layout is showing: the preference is on and a document is open.
pub(crate) fn showing(app: &PdfCraftApp) -> bool {
    app.immersive && app.active.is_some()
}

/// Where the tools palette goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolBar {
    /// Down the page's left edge (the classic layout).
    Left,
    /// Across the top of the window (immersive, tablet and desktop).
    Top,
    /// Across the bottom of the window (immersive, phone).
    Bottom,
}

/// Where [`overlay`]'s capsules are laid out this frame (the whole document area, before notices
/// take their share), for the tools palette to line up with them.
const AREA: &str = "pdfcraft-immersive-area";

/// Remember the area the floating controls sit in, before the document is drawn.
pub(crate) fn set_area(ctx: &egui::Context, area: Rect) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(AREA), area));
}

/// The tools palette's place, and the point and corner it is pinned by: `area` is the page area
/// the classic layout pins it to; the immersive layout uses the window-wide area of
/// [`set_area`], so the palette lines up with the other capsules.
pub(crate) fn tool_bar_anchor(ctx: &egui::Context, app: &PdfCraftApp, area: Rect) -> (ToolBar, egui::Pos2, Align2) {
    let whole = if showing(app) { ctx.data(|d| d.get_temp::<Rect>(egui::Id::new(AREA))).unwrap_or(area) } else { area };
    match placement(showing(app), whole.width()) {
        ToolBar::Left => (ToolBar::Left, area.left_top() + vec2(14.0, 14.0), Align2::LEFT_TOP),
        ToolBar::Top => (ToolBar::Top, egui::pos2(whole.center().x, whole.top() + EDGE), Align2::CENTER_TOP),
        ToolBar::Bottom => (ToolBar::Bottom, egui::pos2(whole.center().x, whole.bottom() - EDGE), Align2::CENTER_BOTTOM),
    }
}

fn placement(immersive: bool, width: f32) -> ToolBar {
    match (immersive, width >= NARROW) {
        (false, _) => ToolBar::Left,
        (true, true) => ToolBar::Top,
        (true, false) => ToolBar::Bottom,
    }
}

/// One floating glass capsule of buttons at `pos` (its `pivot` corner).
fn capsule<R>(
    ctx: &egui::Context,
    t: &Tokens,
    id: &str,
    pos: egui::Pos2,
    pivot: Align2,
    across: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let radius = ((BUTTON + 2.0 * f32::from(PAD)) / 2.0).round().clamp(0.0, 255.0) as u8;
    egui::Area::new(egui::Id::new(("immersive", id)))
        .order(egui::Order::Middle)
        .pivot(pivot)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            theme::glass_frame(t, radius, 1.2)
                .inner_margin(egui::Margin::same(PAD))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                    let layout = if across { egui::Layout::left_to_right(egui::Align::Center) } else { egui::Layout::top_down(egui::Align::Center) };
                    ui.with_layout(layout, add).inner
                })
                .inner
        })
        .inner
        .into()
}

/// Draw the floating controls over the document area `area`.
pub(crate) fn overlay(app: &mut PdfCraftApp, ctx: &egui::Context, area: Rect) {
    let Some((index, id)) = app.active_ids() else { return };
    let t = Tokens::get(ctx);
    let narrow = area.width() < NARROW;
    let name = app.session.get(id).map(|d| d.name.clone()).unwrap_or_default();
    let pages = app.session.get(id).map_or(0, |d| d.info.pages.len());

    // Top left: Home, the tools, the document.
    capsule(ctx, &t, "top-left", area.left_top() + vec2(EDGE, EDGE), Align2::LEFT_TOP, true, |ui| {
        if icons::button(ui, "chevron-left", BUTTON, false, tl!("Home")).clicked() {
            app.active = None;
        }
        let tools_open = app.compact_sheet == Some(crate::docking::Panel::Tools);
        if icons::button(ui, "panel-left", BUTTON, tools_open, tl!("All tools")).clicked() {
            if tools_open {
                app.compact_sheet = None;
            } else {
                app.reveal_tools();
            }
        }
        // Short enough to stay clear of the tools centred beside it.
        let room = if narrow {
            90.0
        } else if area.width() < 1200.0 {
            130.0
        } else {
            220.0
        };
        document_switcher(app, ui, &t, &name, room);
    });

    // Top right: find, save, print, and everything else.
    capsule(ctx, &t, "top-right", area.right_top() + vec2(-EDGE, EDGE), Align2::RIGHT_TOP, true, |ui| {
        if icons::button(ui, "search", BUTTON, app.palette_open, tl!("Find tools and commands")).clicked() {
            app.palette_open = true;
        }
        if !narrow {
            let tip = crate::commands::command_tip(ui.ctx(), tl!("Save ({key})"), "file.save");
            if icons::button(ui, "save", BUTTON, false, &tip).clicked() {
                app.run_command("file.save");
            }
            let tip = crate::commands::command_tip(ui.ctx(), tl!("Print ({key})"), "print.dialog");
            if icons::button(ui, "printer", BUTTON, false, &tip).clicked() {
                app.run_command("print.dialog");
            }
        }
        let more = icons::button(ui, "ellipsis", BUTTON, false, tl!("More"));
        egui::Popup::menu(&more).align(egui::RectAlign::BOTTOM_END).show(|ui| {
            ui.menu_button(tl!("Workspace"), |ui| {
                for (mode, label) in crate::chrome::MODES {
                    if ui.add(egui::Button::selectable(app.mode == mode, tl!(label))).clicked() {
                        app.switch_mode(mode, true);
                        ui.close();
                    }
                }
            });
            ui.separator();
            crate::chrome::menu_items(app, ui);
        });
    });

    // Right edge: the side panels.
    let mid = egui::pos2(area.right() - EDGE, area.center().y);
    capsule(ctx, &t, "panels", mid, Align2::RIGHT_CENTER, false, |ui| {
        for (panel, icon, tip) in [
            (RightPanel::Comments, "message-square-text", "Comments"),
            (RightPanel::Bookmarks, "bookmark", "Bookmarks"),
            (RightPanel::Pages, "files", "Page thumbnails"),
            (RightPanel::Search, "search", "Search"),
            (RightPanel::Attachments, "paperclip", "Attachments"),
            (RightPanel::Signatures, "signature", "Signatures"),
        ] {
            let open = app.right == Some(panel) && app.compact_sheet == Some(crate::docking::Panel::Inspector);
            if icons::button(ui, icon, BUTTON, open, tl!(tip)).clicked() {
                if open {
                    app.compact_sheet = None;
                } else {
                    app.choose_right_panel(Some(panel));
                }
            }
        }
    });

    if narrow {
        return;
    }
    // Bottom left: undo and redo.
    capsule(ctx, &t, "undo", area.left_bottom() + vec2(EDGE, -EDGE), Align2::LEFT_BOTTOM, true, |ui| {
        let tip = crate::commands::command_tip(ui.ctx(), tl!("Undo ({key})"), "edit.undo");
        if icons::button(ui, "undo-2", BUTTON, false, &tip).clicked() {
            app.execute("edit.undo");
        }
        let tip = crate::commands::command_tip(ui.ctx(), tl!("Redo ({key})"), "edit.redo");
        if icons::button(ui, "redo-2", BUTTON, false, &tip).clicked() {
            app.execute("edit.redo");
        }
    });
    // Bottom right: page and zoom.
    capsule(ctx, &t, "page-zoom", area.right_bottom() + vec2(-EDGE, -EDGE), Align2::RIGHT_BOTTOM, true, |ui| {
        let Some(view) = app.views.get_mut(index) else { return };
        let current = view.current;
        if icons::button(ui, "chevron-left", BUTTON, false, tl!("Previous page")).clicked() && current > 0 {
            view.go_to_page(current - 1);
        }
        ui.label(egui::RichText::new(format!("{} / {}", (current + 1).min(pages.max(1)), pages)).font(theme::medium(13.0)).color(t.text));
        if icons::button(ui, "chevron-right", BUTTON, false, tl!("Next page")).clicked() && current + 1 < pages {
            view.go_to_page(current + 1);
        }
        let (bar, _) = ui.allocate_exact_size(vec2(9.0, BUTTON), egui::Sense::hover());
        ui.painter().vline(bar.center().x, bar.y_range().shrink(9.0), egui::Stroke::new(1.0, t.divider));
        if icons::button(ui, "zoom-out", BUTTON, false, tl!("Zoom out")).clicked() {
            view.zoom_step(false);
        }
        let zoom = if view.zoom.is_finite() { (view.zoom * 100.0).round().clamp(0.0, 100_000.0) as u32 } else { 100 };
        ui.label(egui::RichText::new(format!("{zoom}%")).font(theme::medium(13.0)).color(t.text_muted));
        if icons::button(ui, "zoom-in", BUTTON, false, tl!("Zoom in")).clicked() {
            view.zoom_step(true);
        }
    });
}

/// The document's name with a chevron; its menu switches to another open document or opens one.
fn document_switcher(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, name: &str, max_width: f32) {
    let font = theme::semibold(13.5);
    let shown = crate::widgets::fit_line(name, max_width, |s| crate::widgets::text_width(ui, s, &font));
    let w = crate::widgets::text_width(ui, &shown, &font);
    let (rect, resp) = ui.allocate_exact_size(vec2(w + 44.0, BUTTON), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, name));
    let press = theme::Press::track(ui, &resp);
    press.wash(ui, rect, (BUTTON / 2.0) as u8, false);
    let body = rect.translate(press.offset());
    ui.painter().text(body.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, shown, font, t.text);
    icons::paint(ui, Rect::from_center_size(body.right_center() - vec2(16.0, 0.0), vec2(14.0, 14.0)), "chevron-down", 13.0, t.text_muted);
    let resp = theme::hand(resp).on_hover_text(name);
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(220.0);
        let names: Vec<(usize, String)> =
            app.views.iter().enumerate().filter_map(|(i, v)| app.session.get(v.id).map(|d| (i, d.name.clone()))).collect();
        for (i, n) in names {
            if ui.add(egui::Button::selectable(app.active == Some(i), n)).clicked() {
                app.active = Some(i);
                ui.close();
            }
        }
        ui.separator();
        if ui.button(tl!("Open…")).clicked() {
            ui.close();
            app.open_dialog();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tools_move_to_the_top_or_bottom_only_when_immersive() {
        assert_eq!(placement(false, 1400.0), ToolBar::Left);
        assert_eq!(placement(false, 390.0), ToolBar::Left);
        assert_eq!(placement(true, 1400.0), ToolBar::Top);
        assert_eq!(placement(true, NARROW), ToolBar::Top);
        assert_eq!(placement(true, 390.0), ToolBar::Bottom);
        // A nonsense width is a phone's, never a crash.
        assert_eq!(placement(true, f32::NAN), ToolBar::Bottom);
    }

    #[test]
    fn immersive_needs_the_preference_and_a_document() {
        let mut app = PdfCraftApp::new();
        assert!(!showing(&app));
        app.immersive = true;
        assert!(!showing(&app), "Home keeps the classic layout");
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";
        app.open_bytes("one.pdf", None, pdf.to_vec()).expect("a minimal PDF opens");
        assert!(showing(&app));
        app.immersive = false;
        assert!(!showing(&app));
    }
}
