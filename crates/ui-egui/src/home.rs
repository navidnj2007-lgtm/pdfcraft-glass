//! Home tab: recommended tools, open card, pinned folders and recent files (local only, never
//! another app's list).

use egui::{Align2, CornerRadius, Rect, Sense, vec2};
use pdfcraft_engine::catalog;

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons, panels::human_size, widgets};

/// Corner radius of the cards.
const CARD_RADIUS: u8 = 16;

/// Below this width Home is laid out for a phone: narrower margins, two tool columns.
const PHONE: f32 = 600.0;

/// The widest Home's content grows; a wider window centres it.
const MAX_WIDTH: f32 = 1120.0;

/// A soft shadow under a card, rising on hover and settling when the card is pressed.
fn card_lift(ui: &egui::Ui, t: &Tokens, rect: Rect, press: theme::Press) {
    let lift = 0.35 + 0.65 * press.hover * (1.0 - press.down);
    let shadow = egui::Shadow { offset: [0, 2], blur: (6.0 + 10.0 * lift) as u8, spread: 0, color: t.page_shadow.gamma_multiply(0.45 + 0.4 * lift) };
    ui.painter().add(shadow.as_shape(rect, CornerRadius::same(CARD_RADIUS)));
}

/// A card's surface: white (or the dark card colour), washed while hovered or pressed.
fn card(ui: &egui::Ui, t: &Tokens, rect: Rect, resp: &egui::Response) -> (Rect, theme::Press) {
    let press = theme::Press::track(ui, resp);
    card_lift(ui, t, rect, press);
    ui.painter().rect_filled(rect, CornerRadius::same(CARD_RADIUS), t.card);
    let wash = press.fill(egui::Color32::TRANSPARENT, t.hover.gamma_multiply(0.7), t.pressed.gamma_multiply(0.9));
    if wash.a() > 0 {
        ui.painter().rect_filled(rect, CornerRadius::same(CARD_RADIUS), wash);
    }
    (rect.translate(press.offset()), press)
}

/// Home: a large title, the quick actions (open, create, combine), recent files and every tool,
/// laid out for the window's width from a phone's to a desktop's.
pub fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let full = ui.available_width();
    let phone = full < PHONE;
    let margin = if phone { 16.0 } else { 32.0 };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let width = (full - 2.0 * margin).clamp(200.0, MAX_WIDTH);
        let left = ((full - width) / 2.0).max(margin);
        ui.add_space(if phone { 14.0 } else { 26.0 });
        ui.horizontal(|ui| {
            ui.add_space(left);
            ui.vertical(|ui| {
                ui.set_width(width);
                header(app, ui, &t, phone);
                ui.add_space(if phone { 16.0 } else { 22.0 });
                quick_actions(app, ui, &t, phone);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.add_space(26.0);
                    crate::folders_ui::section(app, ui);
                }
                recent(app, ui, &t);
                ui.add_space(26.0);
                tools(app, ui, &t, phone);
                ui.add_space(28.0);
                widgets::section_title(ui, tl!("Privacy"));
                ui.label(
                    egui::RichText::new(tl!("PdfCraft works offline. No telemetry, no account, and no cloud processing unless you add a provider."))
                        .color(t.text_muted),
                );
                ui.add_space(24.0);
            });
        });
    });
}

/// The title, and (in the immersive layout, which has no menu bar on Home) find, preferences and
/// the main menu.
fn header(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, phone: bool) {
    ui.horizontal(|ui| {
        let title = if phone { "PdfCraft" } else { tl!("Welcome to PdfCraft") };
        ui.label(egui::RichText::new(title).font(theme::semibold(30.0)).color(t.text));
        if app.immersive {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let more = icons::button(ui, "ellipsis", 36.0, false, tl!("More"));
                egui::Popup::menu(&more).align(egui::RectAlign::BOTTOM_END).show(|ui| crate::chrome::menu_items(app, ui));
                if icons::button(ui, "settings", 36.0, false, tl!("Preferences")).clicked() {
                    app.execute("app.preferences");
                }
                if icons::button(ui, "search", 36.0, app.palette_open, tl!("Find tools and commands")).clicked() {
                    app.palette_open = true;
                }
            });
        }
    });
    ui.label(
        egui::RichText::new(tl!("An open-source PDF workbench — local, private, and scriptable.")).color(t.text_muted).font(theme::regular(14.0)),
    );
}

/// Open a file (the one call to action, in the accent colour), create a PDF, combine files.
fn quick_actions(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, phone: bool) {
    let width = ui.available_width();
    let gap = 12.0;
    // Three across, or on a phone Open across the top and the other two under it.
    let (open_w, other_w) = if phone { (width, (width - gap) / 2.0) } else { ((width - 2.0 * gap) / 3.0, (width - 2.0 * gap) / 3.0) };
    let height = if phone { 64.0 } else { 76.0 };
    let open_card = |app: &mut PdfCraftApp, ui: &mut egui::Ui| {
        let (rect, resp) = ui.allocate_exact_size(vec2(open_w, height), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!("Open file")));
        let press = theme::Press::track(ui, &resp);
        card_lift(ui, t, rect, press);
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(CARD_RADIUS),
            press.fill(t.accent, t.accent_text, t.accent.lerp_to_gamma(egui::Color32::BLACK, 0.22)),
        );
        let body = rect.translate(press.offset());
        icons::paint(ui, Rect::from_center_size(body.left_center() + vec2(32.0, 0.0), vec2(26.0, 26.0)), "folder-open", 24.0, egui::Color32::WHITE);
        ui.painter().text(body.left_center() + vec2(58.0, 0.0), Align2::LEFT_CENTER, tl!("Open file"), theme::semibold(16.0), egui::Color32::WHITE);
        if theme::hand(resp).clicked() {
            app.open_dialog();
        }
    };
    if phone {
        open_card(app, ui);
        ui.add_space(gap);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        if !phone {
            open_card(app, ui);
        }
        for (id, icon, hue) in [("create", "file-plus", [0xE0u8, 0x3E, 0x3E]), ("combine", "files", [0x2F, 0x6F, 0xE4])] {
            let label = catalog::group(id).map_or(id, |g| g.label);
            let (rect, resp) = ui.allocate_exact_size(vec2(other_w, height), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!(label)));
            let (body, _) = card(ui, t, rect, &resp);
            let tile = if phone { 30.0 } else { 36.0 };
            widgets::icon_tile(
                ui,
                Rect::from_center_size(body.left_center() + vec2(14.0 + tile / 2.0, 0.0), vec2(tile, tile)),
                icon,
                egui::Color32::from_rgb(hue[0], hue[1], hue[2]),
            );
            widgets::row_text(
                ui,
                body.left_center() + vec2(26.0 + tile, -8.0),
                tl!(label),
                theme::semibold(if phone { 14.0 } else { 15.0 }),
                t.text,
                body.width() - tile - 36.0,
            );
            if theme::hand(resp).clicked() {
                app.select_tool(id);
            }
        }
    });
}

/// Files opened before, newest first (none in a browser, which has no paths to reopen).
fn recent(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    if app.recent.is_empty() && cfg!(target_arch = "wasm32") {
        return;
    }
    ui.add_space(26.0);
    let mut clear = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("Recent")).font(theme::semibold(20.0)));
        if !app.recent.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let resp = widgets::ghost_button(ui, "trash-2", tl!("Clear"));
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!("Clear Recent Files")));
                clear = resp.on_hover_text(tl!("Clear Recent Files")).clicked();
            });
        }
    });
    ui.add_space(8.0);
    if app.recent.is_empty() {
        ui.label(egui::RichText::new(tl!("Files you open in PdfCraft appear here. Drop a PDF anywhere to open it.")).color(t.text_muted));
        return;
    }
    let mut open = None;
    let mut remove = None;
    egui::Frame::NONE.fill(t.card).corner_radius(CORNER).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let count = app.recent.len();
        for (k, r) in app.recent.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &r.name));
            let press = theme::Press::track(ui, &resp);
            // The row, not its response: the pointer stays "in" the row over its remove button.
            let hovered = ui.rect_contains_pointer(rect);
            let corners = CornerRadius {
                nw: if k == 0 { 14 } else { 0 },
                ne: if k == 0 { 14 } else { 0 },
                sw: if k + 1 == count { 14 } else { 0 },
                se: if k + 1 == count { 14 } else { 0 },
            };
            if hovered && press.hover < 0.04 && press.down < 0.04 {
                ui.painter().rect_filled(rect, corners, t.hover);
            } else {
                let wash = press.wash_color(t, false);
                if wash.a() > 0 {
                    ui.painter().rect_filled(rect, corners, wash);
                }
            }
            if k + 1 < count {
                ui.painter().hline(rect.left() + 58.0..=rect.right(), rect.bottom(), egui::Stroke::new(1.0, t.divider));
            }
            if hovered {
                // Remove just this file from the list (#430); the file itself is untouched.
                let x_rect = Rect::from_center_size(rect.right_center() - vec2(26.0, 0.0), vec2(28.0, 28.0));
                let x = ui.interact(x_rect, ui.id().with(("remove-recent", &r.path)), Sense::click());
                let label = crate::i18n::fmt(tl!("Remove {name} from Recent"), &[("name", &r.name)]);
                x.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
                if x.hovered() {
                    ui.painter().rect_filled(x_rect, CornerRadius::same(6), t.pressed);
                }
                icons::paint(ui, x_rect.shrink(6.0), "x", 16.0, t.icon);
                if x.on_hover_text(tl!("Remove from Recent")).clicked() {
                    remove = Some(r.path.clone());
                }
            }
            let body = rect.translate(press.offset());
            widgets::icon_tile(
                ui,
                Rect::from_center_size(body.left_center() + vec2(30.0, 0.0), vec2(32.0, 32.0)),
                "file-text",
                egui::Color32::from_rgb(0xE0, 0x3E, 0x3E),
            );
            // Room on the right for the remove button.
            let detail = ui.painter().text(
                body.right_center() - vec2(48.0, 0.0),
                Align2::RIGHT_CENTER,
                format!("{}  ·  {}", crate::i18n::trn(crate::i18n::current(), r.pages as u64, "{n} page", "{n} pages"), human_size(r.size)),
                theme::regular(12.0),
                t.text_muted,
            );
            // The name and path stop short of the page count; hovering shows the whole path.
            let width = detail.left() - 16.0 - (body.left() + 58.0);
            widgets::row_text(ui, body.min + vec2(58.0, 19.0), crate::bidi::visual(&r.name), theme::medium(14.0), t.text, width);
            widgets::row_text(ui, body.min + vec2(58.0, 37.0), crate::bidi::visual(&r.path), theme::regular(11.5), t.text_faint, width);
            if theme::hand(resp).on_hover_text(&r.path).clicked() {
                open = Some(r.path.clone());
            }
        }
    });
    if let Some(p) = open {
        app.open_recent(&p);
    }
    if let Some(p) = remove {
        app.recent.retain(|r| r.path != p);
    }
    if clear {
        app.execute("file.clear_recent");
    }
}

/// The corner radius of Home's grouped lists.
const CORNER: u8 = 14;

/// Every tool group as a card: its icon, its name and what it holds. Two columns on a phone, as
/// many as fit elsewhere.
fn tools(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, phone: bool) {
    ui.label(egui::RichText::new(tl!("All tools")).font(theme::semibold(20.0)));
    ui.add_space(10.0);
    let width = ui.available_width();
    let gap = if phone { 10.0 } else { 14.0 };
    let columns = if phone { 2 } else { ((width + gap) / (220.0 + gap)).floor().clamp(2.0, 6.0) as usize };
    let card_w = ((width - gap * (columns as f32 - 1.0)) / columns as f32).max(60.0);
    let card_h = if phone { 112.0 } else { 118.0 };
    let groups: Vec<&catalog::ToolGroup> = catalog::TOOL_GROUPS.iter().collect();
    for row in groups.chunks(columns) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for g in row {
                let (rect, resp) = ui.allocate_exact_size(vec2(card_w, card_h), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!(g.label)));
                let (body, _) = card(ui, t, rect, &resp);
                let color = egui::Color32::from_rgb(g.hue[0], g.hue[1], g.hue[2]);
                widgets::icon_tile(ui, Rect::from_min_size(body.min + vec2(14.0, 14.0), vec2(34.0, 34.0)), g.icon, color);
                let mut title = egui::text::LayoutJob::simple_singleline(tl!(g.label).to_owned(), theme::semibold(14.0), t.text);
                title.wrap = egui::text::TextWrapping { max_width: body.width() - 28.0, max_rows: 1, break_anywhere: true, ..Default::default() };
                let title = ui.fonts_mut(|f| f.layout_job(title));
                ui.painter().galley(body.min + vec2(14.0, 58.0), title, t.text);
                let blurb =
                    g.sections.first().map(|s| s.items.iter().take(2).map(|i| tl!(i.label)).collect::<Vec<_>>().join(" · ")).unwrap_or_default();
                let mut job = egui::text::LayoutJob::simple(blurb.clone(), theme::regular(11.5), t.text_muted, body.width() - 28.0);
                job.wrap.max_rows = 2;
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                ui.painter().galley(body.min + vec2(14.0, 80.0), galley, t.text_muted);
                if theme::hand(resp).on_hover_text(format!("{}\n{blurb}", tl!(g.label))).clicked() {
                    app.select_tool(g.id);
                }
            }
        });
        ui.add_space(gap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Home's text and cards within the screen at `width` × 900, in German (long labels).
    fn shapes_at(width: f32) -> (Vec<Rect>, Rect) {
        let de = crate::i18n::Lang::from_code("de").unwrap();
        crate::i18n::set_current(de);
        let ctx = egui::Context::default();
        ctx.set_fonts(theme::font_definitions());
        let mut app = PdfCraftApp::new();
        app.immersive = true;
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 900.0));
        let mut rects = Vec::new();
        for _ in 0..2 {
            let output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| show(&mut app, ui));
            rects = output
                .shapes
                .iter()
                .filter(|s| s.clip_rect.intersects(screen))
                .map(|s| s.shape.visual_bounding_rect())
                .filter(|r| r.is_finite())
                .collect();
            output.drop_without_applying_deltas();
        }
        crate::i18n::set_current(crate::i18n::Lang::EN);
        (rects, screen)
    }

    /// A tool card's description stays inside its card in a language with long labels.
    #[test]
    fn translated_card_description_leaves_room_for_the_action() {
        let de = crate::i18n::Lang::from_code("de").unwrap();
        crate::i18n::set_current(de);
        let ctx = egui::Context::default();
        ctx.set_fonts(theme::font_definitions());
        let mut app = PdfCraftApp::new();
        let group = catalog::group("form").unwrap();
        let blurb = group.sections.first().unwrap().items.iter().take(2).map(|item| crate::i18n::tr(de, item.label)).collect::<Vec<_>>().join(" · ");
        let title = crate::i18n::tr(de, group.label);
        let output = ctx
            .run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 900.0))), ..Default::default() }, |ui| {
                show(&mut app, ui)
            });
        crate::i18n::set_current(crate::i18n::Lang::EN);
        let texts: Vec<_> =
            output.shapes.iter().filter_map(|shape| if let egui::Shape::Text(text) = &shape.shape { Some(text) } else { None }).collect();
        let description = texts.iter().find(|text| text.galley.job.text == blurb).unwrap();
        let heading = texts.iter().find(|text| text.galley.job.text == title && (text.pos.x - description.pos.x).abs() < 0.1).unwrap();
        // The title sits 58 points into its card, which is 118 tall.
        let card_bottom = heading.pos.y - 58.0 + 118.0;
        assert!(description.galley.rows.len() <= 2);
        assert!(description.pos.y + description.galley.size().y + 4.0 <= card_bottom, "the description must stay inside its card");
        output.drop_without_applying_deltas();
    }

    /// Home reflows from a phone to a desktop: nothing runs off the screen.
    #[test]
    fn home_fits_a_phone_and_a_tablet() {
        for width in [375.0, 390.0, 768.0, 1180.0] {
            let (rects, screen) = shapes_at(width);
            assert!(rects.len() > 30, "Home drew its cards at {width}");
            for r in &rects {
                assert!(r.right() <= screen.right() + 1.0 && r.left() >= -1.0, "{r:?} leaves the screen at width {width}");
            }
        }
    }
}
