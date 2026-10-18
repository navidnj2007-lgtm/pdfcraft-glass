//! Immersive layout (View ▸ Immersive layout), as iPad and iPhone PDF apps present a document:
//!
//! - a top bar: back to Home, the tools and the page thumbnails on the left; the workspaces as
//!   tabs in the middle (View, Annotate, Draw, Fill & Sign, Edit, Measure; a menu on a phone);
//!   find, the side panels and ⋯ (the main menu) on the right;
//! - under it, the chosen tab's tools in one strip, with the pen's colour and undo and redo; the
//!   strip scrolls sideways when it doesn't fit (a phone);
//! - the page filling the rest, with its page number in a small pill;
//! - a tool's colour, thickness and opacity in a popover by the strip, or a bottom sheet on a
//!   phone (tap the chosen tool again, or its colour);
//! - side panels as sheets over the page (`docking::show_compact`).

use egui::{Align2, Color32, Rect, Sense, Stroke, vec2};

use crate::comments::{self, CommentTool};
use crate::fill_sign::FillTool;
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, QuickTool, RightPanel, icons};

/// The immersive layout's bars are panels now: nothing floats over the top of the pages.
pub(crate) const TOP_RESERVE: f32 = 0.0;

/// Room the right edge keeps from a sheet floating beside it.
pub(crate) const SIDE_CLEARANCE: f32 = 12.0;

/// Below this width (points) the layout is a phone's: the tabs become a menu and the strip
/// scrolls.
pub(crate) const PHONE: f32 = 600.0;

/// Below this width the classic layout's tool palette goes to the bottom.
const NARROW: f32 = 900.0;

/// Heights of the top bar and the tool strip, and the side of their buttons.
const BAR: f32 = 50.0;
const STRIP: f32 = 46.0;
const BUTTON: f32 = 36.0;

/// Whether the immersive layout is showing: the preference is on and a document is open.
pub(crate) fn showing(app: &PdfCraftApp) -> bool {
    app.immersive && app.active.is_some()
}

/// Where the classic layout's tools palette goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolBar {
    /// Down the page's left edge (the classic layout).
    Left,
    /// Across the top of the window.
    Top,
    /// Across the bottom of the window (a phone).
    Bottom,
}

/// The area the document is shown in this frame (the immersive layout's sheets line up with it).
const AREA: &str = "pdfcraft-immersive-area";

/// Remember the area the document is shown in, before it is drawn.
pub(crate) fn set_area(ctx: &egui::Context, area: Rect) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(AREA), area));
}

/// The classic tools palette's place, and the point and corner it is pinned by (`area` is the
/// page area). The immersive layout shows its tools in the strip instead.
pub(crate) fn tool_bar_anchor(_ctx: &egui::Context, app: &PdfCraftApp, area: Rect) -> (ToolBar, egui::Pos2, Align2) {
    match placement(showing(app), area.width()) {
        ToolBar::Left => (ToolBar::Left, area.left_top() + vec2(14.0, 14.0), Align2::LEFT_TOP),
        ToolBar::Top => (ToolBar::Top, egui::pos2(area.center().x, area.top() + 12.0), Align2::CENTER_TOP),
        ToolBar::Bottom => (ToolBar::Bottom, egui::pos2(area.center().x, area.bottom() - 12.0), Align2::CENTER_BOTTOM),
    }
}

fn placement(immersive: bool, width: f32) -> ToolBar {
    match (immersive, width >= NARROW) {
        (false, _) => ToolBar::Left,
        (true, true) => ToolBar::Top,
        (true, false) => ToolBar::Bottom,
    }
}

/// The immersive layout's workspaces, as tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    View,
    #[default]
    Annotate,
    Draw,
    Fill,
    Edit,
    Measure,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::View, Tab::Annotate, Tab::Draw, Tab::Fill, Tab::Edit, Tab::Measure];

    pub fn label(self) -> &'static str {
        match self {
            Tab::View => tl!("View"),
            Tab::Annotate => tl!("Annotate"),
            Tab::Draw => tl!("Draw"),
            Tab::Fill => tl!("Fill & Sign"),
            Tab::Edit => tl!("Edit"),
            Tab::Measure => tl!("Measure"),
        }
    }

    /// The tools and actions in this tab's strip.
    fn items(self) -> Vec<Item> {
        let select = Item::Tool(QuickTool::Select, "", "mouse-pointer-2", "Select (V)");
        let comment = |t: CommentTool| Item::Tool(QuickTool::Comment(t), t.command(), t.icon(), t.label());
        let fill = |f: FillTool| Item::Tool(QuickTool::Fill(f), f.command(), f.icon(), f.label());
        let measure = |m: crate::measure_ui::Tool, command, label| Item::Tool(QuickTool::Measure(m), command, "ruler", label);
        match self {
            Tab::View => vec![
                select,
                Item::Tool(QuickTool::Hand, "", "hand", "Hand (H)"),
                Item::Action("zoom-out", "Zoom out", Action::ZoomOut),
                Item::Action("zoom-in", "Zoom in", Action::ZoomIn),
                Item::Action("arrow-left-right", "Fit to width", Action::FitWidth),
                Item::Action("rotate-cw", "Rotate view clockwise", Action::Rotate),
            ],
            Tab::Annotate => vec![
                select,
                comment(CommentTool::Highlight),
                comment(CommentTool::Underline),
                comment(CommentTool::StrikeOut),
                comment(CommentTool::TextBox),
                comment(CommentTool::Note),
                comment(CommentTool::Ink),
                comment(CommentTool::Eraser),
                Item::Shapes,
                Item::Action("stamp", "Add a stamp", Action::Command("comment.stamp")),
            ],
            Tab::Draw => vec![
                select,
                comment(CommentTool::Ink),
                comment(CommentTool::Eraser),
                comment(CommentTool::Line),
                comment(CommentTool::Arrow),
                comment(CommentTool::Rectangle),
                comment(CommentTool::Oval),
                comment(CommentTool::Polygon),
                comment(CommentTool::Cloud),
            ],
            Tab::Fill => vec![
                select,
                fill(FillTool::Text),
                fill(FillTool::Check),
                fill(FillTool::Cross),
                fill(FillTool::Dot),
                fill(FillTool::Line),
                fill(FillTool::Date),
                fill(FillTool::Signature),
                fill(FillTool::Initials),
            ],
            Tab::Edit => vec![
                select,
                Item::Tool(QuickTool::EditText, "edit.edit_text", "text-cursor-input", "Edit text & images"),
                Item::Tool(QuickTool::AddText, "edit.text", "type", "Add text"),
                Item::Action("image-plus", "Add image", Action::Command("edit.image")),
                Item::Tool(QuickTool::Link, "edit.link", "link", "Link"),
                Item::Tool(QuickTool::Crop, "page.crop", "crop", "Crop pages"),
                Item::Tool(QuickTool::Redact, "redact.mark", "square-dashed", "Redact"),
            ],
            Tab::Measure => vec![
                select,
                measure(crate::measure_ui::Tool::Distance, "measure.distance", "Distance"),
                measure(crate::measure_ui::Tool::Perimeter, "measure.perimeter", "Perimeter"),
                measure(crate::measure_ui::Tool::Area, "measure.area", "Area"),
            ],
        }
    }

    /// The tab whose strip holds `tool` (`None` for Select, which every tab has, and for tools
    /// no strip shows).
    pub fn of(tool: QuickTool) -> Option<Tab> {
        match tool {
            QuickTool::Select => None,
            QuickTool::Hand => Some(Tab::View),
            QuickTool::Comment(t) if DRAW_SHAPES.contains(&t) => Some(Tab::Draw),
            QuickTool::Comment(_) | QuickTool::Stamp(_) | QuickTool::CustomStamp(_) => Some(Tab::Annotate),
            QuickTool::Fill(_) => Some(Tab::Fill),
            QuickTool::EditText | QuickTool::AddText | QuickTool::Link | QuickTool::Crop | QuickTool::Redact => Some(Tab::Edit),
            QuickTool::Measure(_) => Some(Tab::Measure),
            _ => None,
        }
    }
}

/// The shapes, in the Annotate tab's Shapes menu and the Draw tab.
const DRAW_SHAPES: [CommentTool; 6] =
    [CommentTool::Line, CommentTool::Arrow, CommentTool::Rectangle, CommentTool::Oval, CommentTool::Polygon, CommentTool::Cloud];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    ZoomIn,
    ZoomOut,
    FitWidth,
    Rotate,
    Command(&'static str),
}

/// One button in the tool strip.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Item {
    /// A tool, chosen by its command (none: set directly), with its icon and label.
    Tool(QuickTool, &'static str, &'static str, &'static str),
    /// A one-off action, with its icon and label.
    Action(&'static str, &'static str, Action),
    /// The shapes, in one button with a menu.
    Shapes,
}

/// The tool strip's tab follows the tool: one chosen from the menus, the palette or a key shows
/// its own tab's strip.
pub(crate) fn follow_tool(app: &mut PdfCraftApp) {
    if let Some(tab) = Tab::of(app.quick_tool)
        && tab != app.doc_tab
        && !(tab == Tab::Draw && app.doc_tab == Tab::Annotate)
        && !(app.doc_tab == Tab::Draw && matches!(app.quick_tool, QuickTool::Comment(CommentTool::Ink | CommentTool::Eraser)))
    {
        app.doc_tab = tab;
    }
}

/// Choose `tab`: its strip shows, and a tool of another tab is put down.
pub(crate) fn choose_tab(app: &mut PdfCraftApp, tab: Tab) {
    app.doc_tab = tab;
    app.tool_sheet = false;
    let keeps = tab.items().iter().any(|i| match i {
        Item::Tool(t, ..) => *t == app.quick_tool,
        Item::Shapes => matches!(app.quick_tool, QuickTool::Comment(t) if DRAW_SHAPES.contains(&t)),
        Item::Action(..) => false,
    });
    if !keeps {
        if matches!(app.quick_tool, QuickTool::EditText | QuickTool::AddText) {
            app.put_down_edit_text();
        }
        app.quick_tool = QuickTool::Select;
    }
}

/// The top bar and the tool strip, as panels above the document.
pub(crate) fn bars(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    follow_tool(app);
    let phone = ui.available_width() < PHONE;
    let frame = egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(8, 0));
    egui::Panel::top("immersive-top-bar").exact_size(BAR).resizable(false).show_separator_line(false).frame(frame).show(ui, |ui| {
        top_bar(app, ui, &t, phone);
    });
    egui::Panel::top("immersive-tool-strip").exact_size(STRIP).resizable(false).show_separator_line(false).frame(frame).show(ui, |ui| {
        let r = ui.max_rect();
        ui.painter().hline(r.x_range().expand(8.0), r.bottom(), Stroke::new(1.0, t.divider));
        tool_strip(app, ui, &t, phone);
    });
    tool_sheet(app, ui.ctx(), &t, phone);
}

fn top_bar(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, phone: bool) {
    let rect = ui.max_rect();
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if icons::button(ui, "chevron-left", BUTTON, false, tl!("Home")).clicked() {
            app.active = None;
            app.tool_sheet = false;
        }
        let tools_open = app.compact_sheet == Some(crate::docking::Panel::Tools);
        if icons::button(ui, "layout-grid", BUTTON, tools_open, tl!("All tools")).clicked() {
            if tools_open {
                app.compact_sheet = None;
            } else {
                app.reveal_tools();
            }
        }
        if !phone {
            panel_button(app, ui, RightPanel::Pages, "files", "Page thumbnails");
        }
    });
    // The tabs, centred in the bar; a menu of them on a phone.
    let center = rect.center();
    let tabs_rect = if phone {
        Rect::from_center_size(center, vec2(150.0, 34.0))
    } else {
        let widths: Vec<f32> = Tab::ALL.iter().map(|tab| crate::widgets::text_width(ui, tab.label(), &theme::medium(13.5)) + 26.0).collect();
        let total: f32 = widths.iter().sum::<f32>() + 8.0;
        Rect::from_center_size(center, vec2(total, 34.0))
    };
    let side = 3.0 * BUTTON + 24.0;
    let fits = !phone && tabs_rect.left() > rect.left() + side && tabs_rect.right() < rect.right() - side - BUTTON;
    if fits {
        segmented_tabs(app, ui, t, tabs_rect);
    } else {
        tab_menu(app, ui, t, Rect::from_center_size(center, vec2(150.0, 34.0)));
    }
    let right = Rect::from_min_max(egui::pos2(rect.right() - 4.0 * (BUTTON + 2.0), rect.top()), rect.right_bottom());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)));
    let ui = &mut child;
    ui.spacing_mut().item_spacing.x = 2.0;
    let more = icons::button(ui, "ellipsis", BUTTON, false, tl!("More"));
    egui::Popup::menu(&more).align(egui::RectAlign::BOTTOM_END).show(|ui| {
        ui.set_min_width(240.0);
        if ui.button(tl!("Find tools and commands")).clicked() {
            app.palette_open = true;
            ui.close();
        }
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
    if !phone {
        panel_button(app, ui, RightPanel::Bookmarks, "bookmark", "Bookmarks");
        panel_button(app, ui, RightPanel::Comments, "message-square-text", "Comments");
    }
    let tip = crate::commands::command_tip(ui.ctx(), tl!("Find text"), "edit.find");
    if icons::button(ui, "search", BUTTON, false, &tip).clicked() {
        app.execute("edit.find");
    }
}

/// A side panel's button: opens it as a sheet over the page, or closes it.
fn panel_button(app: &mut PdfCraftApp, ui: &mut egui::Ui, panel: RightPanel, icon: &str, tip: &'static str) {
    let open = app.right == Some(panel) && app.compact_sheet == Some(crate::docking::Panel::Inspector);
    if icons::button(ui, icon, BUTTON, open, tl!(tip)).clicked() {
        if open {
            app.compact_sheet = None;
        } else {
            app.choose_right_panel(Some(panel));
        }
    }
}

/// The tabs as one segmented control, as iPadOS draws them.
fn segmented_tabs(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, rect: Rect) {
    ui.painter().rect_filled(rect, 10.0, t.control);
    let mut x = rect.left() + 4.0;
    for tab in Tab::ALL {
        let w = crate::widgets::text_width(ui, tab.label(), &theme::medium(13.5)) + 26.0;
        let r = Rect::from_min_size(egui::pos2(x, rect.top() + 3.0), vec2(w, rect.height() - 6.0));
        x += w;
        let resp = ui.interact(r, ui.id().with(("immersive-tab", tab.label())), Sense::click());
        let on = app.doc_tab == tab;
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, tab.label()));
        let press = theme::Press::track(ui, &resp);
        if on {
            ui.painter().add(egui::Shadow { offset: [0, 1], blur: 4, spread: 0, color: Color32::from_black_alpha(28) }.as_shape(r, 8));
            ui.painter().rect_filled(r, 8.0, t.card);
        } else {
            press.wash(ui, r, 8, false);
        }
        let font = if on { theme::semibold(13.5) } else { theme::medium(13.5) };
        ui.painter().text(r.center() + press.offset(), Align2::CENTER_CENTER, tab.label(), font, if on { t.text } else { t.text_muted });
        if theme::hand(resp).clicked() && !on {
            choose_tab(app, tab);
        }
    }
}

/// The tabs as a menu under the current tab's name (a phone, or a narrow window).
fn tab_menu(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, rect: Rect) {
    let resp = ui.interact(rect, ui.id().with("immersive-tab-menu"), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, app.doc_tab.label()));
    let press = theme::Press::track(ui, &resp);
    ui.painter().rect_filled(rect, 10.0, press.fill(t.control, t.hover, t.pressed));
    let body = rect.translate(press.offset());
    let label = app.doc_tab.label();
    let font = theme::semibold(14.5);
    let w = crate::widgets::text_width(ui, label, &font);
    let start = body.center().x - (w + 18.0) / 2.0;
    ui.painter().text(egui::pos2(start, body.center().y), Align2::LEFT_CENTER, label, font, t.text);
    icons::paint(ui, Rect::from_center_size(egui::pos2(start + w + 12.0, body.center().y), vec2(14.0, 14.0)), "chevron-down", 13.0, t.text_muted);
    egui::Popup::menu(&theme::hand(resp)).align(egui::RectAlign::BOTTOM).show(|ui| {
        ui.set_min_width(200.0);
        for tab in Tab::ALL {
            if ui.add(egui::Button::selectable(app.doc_tab == tab, tab.label())).clicked() {
                choose_tab(app, tab);
                ui.close();
            }
        }
    });
}

/// The strip's buttons are 38 points apart.
const ITEM: f32 = 38.0;

fn tool_strip(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, phone: bool) {
    let rect = ui.max_rect();
    let items = app.doc_tab.items();
    let extras = if matches!(app.quick_tool, QuickTool::Comment(c) if c != CommentTool::Eraser) { 1.0 } else { 0.0 };
    let tools_width = (items.len() as f32 + extras) * ITEM;
    let history = 2.0 * ITEM + 10.0;
    let title_room = if phone { 0.0 } else { (rect.width() / 2.0 - tools_width / 2.0 - 16.0).clamp(0.0, 260.0) };
    // The document's name on the left, when there's room beside the centred tools.
    if title_room > 90.0
        && let Some((_, id)) = app.active_ids()
    {
        let name = app.session.get(id).map(|d| d.name.clone()).unwrap_or_default();
        let r = Rect::from_min_size(rect.left_top(), vec2(title_room, rect.height()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center)));
        document_switcher(app, &mut child, t, &name, title_room - 50.0);
    }
    let fits = tools_width + 2.0 * history <= rect.width();
    if fits {
        let tools = Rect::from_center_size(rect.center(), vec2(tools_width, BUTTON));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(tools).layout(egui::Layout::left_to_right(egui::Align::Center)));
        strip_items(app, &mut child, t, &items);
        let r = Rect::from_min_max(egui::pos2(rect.right() - history, rect.top()), rect.right_bottom());
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::right_to_left(egui::Align::Center)));
        undo_redo(app, &mut child, true);
    } else {
        // A phone: everything in one row that scrolls sideways.
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.set_min_height(rect.height());
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        strip_items(app, ui, t, &items);
                        let (sep, _) = ui.allocate_exact_size(vec2(9.0, BUTTON), Sense::hover());
                        ui.painter().vline(sep.center().x, sep.y_range().shrink(8.0), Stroke::new(1.0, t.divider));
                        undo_redo(app, ui, false);
                    });
                });
        });
    }
}

fn undo_redo(app: &mut PdfCraftApp, ui: &mut egui::Ui, reversed: bool) {
    ui.spacing_mut().item_spacing.x = 2.0;
    let undo = |app: &mut PdfCraftApp, ui: &mut egui::Ui| {
        let tip = crate::commands::command_tip(ui.ctx(), tl!("Undo ({key})"), "edit.undo");
        if icons::button(ui, "undo-2", BUTTON, false, &tip).clicked() {
            app.execute("edit.undo");
        }
    };
    let redo = |app: &mut PdfCraftApp, ui: &mut egui::Ui| {
        let tip = crate::commands::command_tip(ui.ctx(), tl!("Redo ({key})"), "edit.redo");
        if icons::button(ui, "redo-2", BUTTON, false, &tip).clicked() {
            app.execute("edit.redo");
        }
    };
    // Right to left in the wide strip, so Redo is drawn first.
    if reversed {
        redo(app, ui);
        undo(app, ui);
    } else {
        undo(app, ui);
        redo(app, ui);
    }
}

fn strip_items(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, items: &[Item]) {
    ui.spacing_mut().item_spacing.x = 2.0;
    for item in items {
        match *item {
            Item::Tool(tool, command, icon, label) => {
                let on = app.quick_tool == tool;
                let resp = icons::button(ui, icon, BUTTON, on, tl!(label));
                let has_sheet = matches!(tool, QuickTool::Comment(c) if c != CommentTool::Eraser);
                if resp.clicked() || resp.long_touched() {
                    if on && has_sheet {
                        app.tool_sheet = !app.tool_sheet;
                    } else if !on {
                        choose_tool(app, tool, command);
                    }
                }
                if on
                    && let QuickTool::Comment(c) = tool
                    && c != CommentTool::Eraser
                {
                    // The pen's colour, beside it, as note-taking apps show it.
                    colour_dot(app, ui, t, c);
                }
            }
            Item::Action(icon, label, action) => {
                if icons::button(ui, icon, BUTTON, false, tl!(label)).clicked() {
                    run(app, action);
                }
            }
            Item::Shapes => {
                let current = match app.quick_tool {
                    QuickTool::Comment(c) if DRAW_SHAPES.contains(&c) => Some(c),
                    _ => None,
                };
                let shown = current
                    .unwrap_or(app.comment_prefs.group_tool.get(2).copied().filter(|c| DRAW_SHAPES.contains(c)).unwrap_or(CommentTool::Rectangle));
                let resp = icons::button(ui, shown.icon(), BUTTON, current.is_some(), tl!(shown.label()));
                let r = resp.rect;
                let tri = [r.right_bottom() + vec2(-5.0, -5.0), r.right_bottom() + vec2(-10.0, -5.0), r.right_bottom() + vec2(-5.0, -10.0)];
                ui.painter().add(egui::Shape::convex_polygon(tri.to_vec(), if current.is_some() { t.accent } else { t.text_faint }, Stroke::NONE));
                let open = (resp.clicked() && current.is_some()) || resp.long_touched() || resp.secondary_clicked();
                if resp.clicked() && current.is_none() {
                    choose_tool(app, QuickTool::Comment(shown), shown.command());
                }
                egui::Popup::menu(&resp)
                    .open_memory(open.then_some(egui::SetOpenCommand::Toggle))
                    .align(egui::RectAlign::BOTTOM_START)
                    .gap(6.0)
                    .show(|ui| {
                        ui.set_min_width(190.0);
                        for shape in DRAW_SHAPES {
                            let row = ui.horizontal(|ui| {
                                icons::paint(ui, Rect::from_min_size(ui.cursor().min + vec2(0.0, 2.0), vec2(16.0, 16.0)), shape.icon(), 16.0, t.text);
                                ui.add_space(24.0);
                                ui.add(egui::Button::selectable(current == Some(shape), tl!(shape.label())))
                            });
                            if row.inner.clicked() {
                                choose_tool(app, QuickTool::Comment(shape), shape.command());
                                ui.close();
                            }
                        }
                        if let Some(c) = current {
                            ui.separator();
                            if ui.button(tl!("Colour")).clicked() {
                                app.tool_sheet = true;
                                let _ = c;
                                ui.close();
                            }
                        }
                    });
                if current.is_some()
                    && let Some(c) = current
                {
                    colour_dot(app, ui, t, c);
                }
            }
        }
    }
}

/// The chosen pen's colour as a dot; tapping it opens the tool's settings.
fn colour_dot(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, tool: CommentTool) {
    let style = app.comment_prefs.style(tool);
    let (r, resp) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!("Colour")));
    let press = theme::Press::track(ui, &resp);
    press.wash(ui, r, 8, app.tool_sheet);
    let c = r.center() + press.offset();
    ui.painter().circle_filled(c, 10.0, comments::color32(style.color));
    ui.painter().circle_stroke(c, 10.0, Stroke::new(1.0, Color32::from_black_alpha(45)));
    ui.painter().circle_stroke(c, 12.5, Stroke::new(1.5, if app.tool_sheet { t.accent } else { Color32::TRANSPARENT }));
    if theme::hand(resp).on_hover_text(tl!("Colour")).clicked() {
        app.tool_sheet = !app.tool_sheet;
    }
}

/// Choose `tool` through its command (which says what to do and opens what it needs), keeping
/// the Tools sheet closed: the strip is where the tools are.
fn choose_tool(app: &mut PdfCraftApp, tool: QuickTool, command: &str) {
    app.tool_sheet = false;
    if matches!(app.quick_tool, QuickTool::EditText | QuickTool::AddText) && !matches!(tool, QuickTool::EditText | QuickTool::AddText) {
        app.put_down_edit_text();
    }
    if command.is_empty() || !app.execute(command) {
        app.quick_tool = tool;
    }
    app.pending_panel_reveals.retain(|p| *p != crate::docking::Panel::Tools);
}

fn run(app: &mut PdfCraftApp, action: Action) {
    let Some(index) = app.active else { return };
    match action {
        // An action may need its panel (the stamp palette is in the Tools sheet).
        Action::Command(c) => {
            app.execute(c);
        }
        _ => {
            let Some(view) = app.views.get_mut(index) else { return };
            match action {
                Action::ZoomIn => view.zoom_step(true),
                Action::ZoomOut => view.zoom_step(false),
                Action::FitWidth => view.fit = crate::canvas::Fit::Width,
                Action::Rotate => view.rotate_view(true),
                Action::Command(_) => {}
            }
        }
    }
}

/// The chosen tool's settings: colour, thickness and opacity (and fill for shapes), in a popover
/// under the strip's right end, or a bottom sheet on a phone.
fn tool_sheet(app: &mut PdfCraftApp, ctx: &egui::Context, t: &Tokens, phone: bool) {
    let QuickTool::Comment(tool) = app.quick_tool else {
        app.tool_sheet = false;
        return;
    };
    if !app.tool_sheet || tool == CommentTool::Eraser {
        app.tool_sheet = false;
        return;
    }
    let screen = ctx.content_rect();
    let area = egui::Area::new(egui::Id::new("immersive-tool-sheet")).order(egui::Order::Foreground);
    let (area, width, frame) = if phone {
        let frame = egui::Frame::NONE
            .fill(t.card)
            .corner_radius(egui::CornerRadius { nw: 16, ne: 16, sw: 0, se: 0 })
            .inner_margin(egui::Margin { left: 20, right: 20, top: 12, bottom: 28 })
            .shadow(egui::Shadow { offset: [0, -2], blur: 18, spread: 0, color: Color32::from_black_alpha(40) });
        (area.pivot(Align2::CENTER_BOTTOM).fixed_pos(screen.center_bottom()), screen.width() - 40.0, frame)
    } else {
        let frame = egui::Frame::NONE
            .fill(t.card)
            .corner_radius(14)
            .stroke(Stroke::new(1.0, t.divider))
            .inner_margin(egui::Margin::same(16))
            .shadow(egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(36) });
        (area.pivot(Align2::RIGHT_TOP).fixed_pos(egui::pos2(screen.right() - 12.0, screen.top() + BAR + STRIP + 6.0)), 260.0, frame)
    };
    let modal = egui::Modal::new(egui::Id::new("immersive-tool-sheet-modal"))
        .area(area)
        .frame(frame)
        .backdrop_color(if phone { Color32::from_black_alpha(60) } else { Color32::TRANSPARENT })
        .show(ctx, |ui| {
            ui.set_width(width.max(200.0));
            sheet_body(app, ui, t, tool);
        });
    if modal.should_close() {
        app.tool_sheet = false;
    }
}

/// An iOS-style slider across the sheet: a track filled with the accent up to a round knob, and
/// the value after it. Returns whether the value changed.
fn slider(ui: &mut egui::Ui, t: &Tokens, label: &str, value: &mut f64, range: (f64, f64), step: f64, show: impl Fn(f64) -> String) -> bool {
    let (lo, hi) = range;
    let before = *value;
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click_and_drag());
    let track = Rect::from_min_max(egui::pos2(rect.left() + 10.0, rect.center().y - 2.0), egui::pos2(rect.right() - 70.0, rect.center().y + 2.0));
    if let Some(p) = resp.interact_pointer_pos()
        && track.width() > 0.0
    {
        let f = f64::from(((p.x - track.left()) / track.width()).clamp(0.0, 1.0));
        let v = lo + f * (hi - lo);
        *value = if step > 0.0 { ((v - lo) / step).round() * step + lo } else { v }.clamp(lo, hi);
    }
    if !value.is_finite() {
        *value = lo;
    }
    let f = if hi > lo { ((*value - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let knob = egui::pos2(track.left() + f * track.width(), track.center().y);
    ui.painter().rect_filled(track, 2.0, t.control.lerp_to_gamma(t.border, 0.5));
    ui.painter().rect_filled(Rect::from_min_max(track.min, egui::pos2(knob.x, track.max.y)), 2.0, t.accent);
    ui.painter().circle_filled(knob + vec2(0.0, 1.0), 11.0, Color32::from_black_alpha(30));
    ui.painter().circle_filled(knob, 10.5, Color32::WHITE);
    ui.painter().circle_stroke(knob, 10.5, Stroke::new(0.5, Color32::from_black_alpha(40)));
    ui.painter().text(egui::pos2(rect.right(), rect.center().y), Align2::RIGHT_CENTER, show(*value), theme::medium(13.0), t.text);
    resp.widget_info(|| egui::WidgetInfo::slider(true, *value, label));
    (*value - before).abs() > f64::EPSILON
}

fn caption(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(text.to_uppercase()).font(theme::medium(11.0)).color(t.text_faint));
    ui.add_space(2.0);
}

fn sheet_body(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, tool: CommentTool) {
    let style = app.comment_prefs.style(tool);
    ui.horizontal(|ui| {
        let w = ui.available_width();
        let (r, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
        ui.painter().text(r.center(), Align2::CENTER_CENTER, tl!(tool.label()), theme::semibold(15.0), t.text);
        let close = Rect::from_center_size(r.right_center() - vec2(12.0, 0.0), vec2(26.0, 26.0));
        let resp = ui.interact(close, ui.id().with("tool-sheet-close"), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!("Close")));
        if resp.hovered() {
            ui.painter().circle_filled(close.center(), 13.0, t.hover);
        }
        icons::paint(ui, close.shrink(6.0), "x", 14.0, t.text_muted);
        if theme::hand(resp).clicked() {
            app.tool_sheet = false;
        }
    });
    let mut picked: Option<pdfcraft_engine::Rgb> = None;
    caption(ui, t, tl!("Colour"));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (name, c) in comments::SWATCHES {
            let (r, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!(name)));
            let on = comments::same_colour(&style.color, &c);
            ui.painter().circle_filled(r.center(), 12.0, comments::color32(c));
            ui.painter().circle_stroke(r.center(), 12.0, Stroke::new(1.0, Color32::from_black_alpha(40)));
            if on {
                ui.painter().circle_stroke(r.center(), 15.0, Stroke::new(2.0, t.accent));
            }
            if theme::hand(resp).on_hover_text(tl!(name)).clicked() {
                picked = Some(c);
            }
        }
    });
    if let Some(c) = picked {
        app.comment_prefs.set_color(tool, c);
        restyle_selected(app, tool, Some(c));
    }
    if tool.has_width() {
        caption(ui, t, tl!("Line thickness"));
        let mut w = style.width;
        if slider(ui, t, tl!("Line thickness"), &mut w, (0.5, 12.0), 0.5, |v| format!("{v:.1} pt")) {
            app.comment_prefs.set_width(tool, w);
        }
    }
    caption(ui, t, tl!("Opacity"));
    let mut percent = (style.opacity * 100.0).round();
    if slider(ui, t, tl!("Opacity"), &mut percent, (10.0, 100.0), 5.0, |v| format!("{v:.0} %")) {
        app.comment_prefs.set_opacity(tool, percent / 100.0);
    }
    if tool.has_fill() {
        caption(ui, t, tl!("Fill colour"));
        let mut fill: Option<Option<pdfcraft_engine::Rgb>> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let (r, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!("No fill")));
            ui.painter().circle_stroke(r.center(), 12.0, Stroke::new(1.0, Color32::from_black_alpha(80)));
            ui.painter()
                .line_segment([r.center() + vec2(-8.0, 8.0), r.center() + vec2(8.0, -8.0)], Stroke::new(1.5, Color32::from_rgb(0xD3, 0x2F, 0x2F)));
            if style.fill.is_none() {
                ui.painter().circle_stroke(r.center(), 15.0, Stroke::new(2.0, t.accent));
            }
            if theme::hand(resp).on_hover_text(tl!("No fill")).clicked() {
                fill = Some(None);
            }
            for (name, c) in comments::SWATCHES {
                let (r, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!(name)));
                ui.painter().rect_filled(Rect::from_center_size(r.center(), vec2(22.0, 22.0)), 5.0, comments::color32(c));
                if style.fill.is_some_and(|f| comments::same_colour(&f, &c)) {
                    ui.painter().rect_stroke(
                        Rect::from_center_size(r.center(), vec2(28.0, 28.0)),
                        7.0,
                        Stroke::new(2.0, t.accent),
                        egui::StrokeKind::Inside,
                    );
                }
                if theme::hand(resp).on_hover_text(tl!(name)).clicked() {
                    fill = Some(Some(c));
                }
            }
        });
        if let Some(f) = fill {
            app.comment_prefs.set_fill(tool, f);
        }
    }
    ui.add_space(4.0);
}

/// A colour picked for the tool also recolours the comment of that tool that is selected.
fn restyle_selected(app: &mut PdfCraftApp, tool: CommentTool, color: Option<pdfcraft_engine::Rgb>) {
    let Some(color) = color else { return };
    let Some((index, id)) = app.active_ids() else { return };
    let Some((page, annotation)) = app.views.get(index).and_then(|view| view.comments.selected) else { return };
    let matches = app.session.get(id).is_some_and(|doc| {
        doc.allows_annotation()
            && doc.info.annotations.iter().any(|a| a.page == page && a.index == annotation && a.in_reply_to.is_none() && comments::made_by(a, tool))
    });
    if matches {
        app.apply_edit(pdfcraft_engine::Edit::StyleAnnotation {
            page,
            index: annotation,
            color: Some(color),
            opacity: None,
            width: None,
            endings: None,
        });
    }
}

/// The page number in a small pill at the bottom of the page area, with the previous and next
/// page beside it (and the zoom, on a tablet).
pub(crate) fn page_pill(app: &mut PdfCraftApp, ctx: &egui::Context, area: Rect) {
    let Some((index, id)) = app.active_ids() else { return };
    let t = Tokens::get(ctx);
    let pages = app.session.get(id).map_or(0, |d| d.info.pages.len());
    let phone = area.width() < PHONE;
    egui::Area::new(egui::Id::new("immersive-page-pill"))
        .order(egui::Order::Middle)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(egui::pos2(area.center().x, area.bottom() - 14.0))
        .show(ctx, |ui| {
            theme::glass_frame(&t, 19, 1.0).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                ui.horizontal(|ui| {
                    let Some(view) = app.views.get_mut(index) else { return };
                    let current = view.current;
                    if icons::button(ui, "chevron-left", 30.0, false, tl!("Previous page")).clicked() && current > 0 {
                        view.go_to_page(current - 1);
                    }
                    ui.label(egui::RichText::new(format!("{} / {}", (current + 1).min(pages.max(1)), pages)).font(theme::medium(13.0)).color(t.text));
                    if icons::button(ui, "chevron-right", 30.0, false, tl!("Next page")).clicked() && current + 1 < pages {
                        view.go_to_page(current + 1);
                    }
                    if !phone {
                        let (bar, _) = ui.allocate_exact_size(vec2(9.0, 30.0), Sense::hover());
                        ui.painter().vline(bar.center().x, bar.y_range().shrink(8.0), Stroke::new(1.0, t.divider));
                        let zoom = if view.zoom.is_finite() { (view.zoom * 100.0).round().clamp(0.0, 100_000.0) as u32 } else { 100 };
                        ui.label(egui::RichText::new(format!("{zoom}%")).font(theme::medium(12.5)).color(t.text_muted));
                        ui.add_space(6.0);
                    }
                });
            });
        });
}

/// The document's name with a chevron; its menu switches to another open document or opens one.
fn document_switcher(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, name: &str, max_width: f32) {
    let font = theme::medium(13.0);
    let shown = crate::widgets::fit_line(name, max_width.max(40.0), |s| crate::widgets::text_width(ui, s, &font));
    let w = crate::widgets::text_width(ui, &shown, &font);
    let (rect, resp) = ui.allocate_exact_size(vec2(w + 40.0, 32.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, name));
    let press = theme::Press::track(ui, &resp);
    press.wash(ui, rect, 8, false);
    let body = rect.translate(press.offset());
    icons::paint(ui, Rect::from_min_size(body.left_center() - vec2(-6.0, 8.0), vec2(16.0, 16.0)), "file-text", 15.0, t.text_muted);
    ui.painter().text(body.left_center() + vec2(26.0, 0.0), Align2::LEFT_CENTER, shown, font, t.text_muted);
    icons::paint(ui, Rect::from_center_size(body.right_center() - vec2(8.0, 0.0), vec2(12.0, 12.0)), "chevron-down", 11.0, t.text_faint);
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
    fn the_classic_tools_move_to_the_top_or_bottom_only_when_immersive() {
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
        assert!(!showing(&app), "Home has its own layout");
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";
        app.open_bytes("one.pdf", None, pdf.to_vec()).expect("a minimal PDF opens");
        assert!(showing(&app));
        app.immersive = false;
        assert!(!showing(&app));
    }

    #[test]
    fn every_tool_in_a_strip_belongs_to_its_tab() {
        for tab in Tab::ALL {
            for item in tab.items() {
                if let Item::Tool(tool, ..) = item
                    && tool != QuickTool::Select
                {
                    let home = Tab::of(tool);
                    assert!(
                        home == Some(tab) || (tab == Tab::Draw && matches!(tool, QuickTool::Comment(CommentTool::Ink | CommentTool::Eraser))),
                        "{tool:?} in {tab:?} belongs to {home:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn choosing_a_tab_puts_down_another_tab_s_tool() {
        let mut app = PdfCraftApp::new();
        app.quick_tool = QuickTool::Comment(CommentTool::Ink);
        choose_tab(&mut app, Tab::Draw);
        assert_eq!(app.quick_tool, QuickTool::Comment(CommentTool::Ink), "the pen is in Draw too");
        choose_tab(&mut app, Tab::Fill);
        assert_eq!(app.quick_tool, QuickTool::Select);
        // A tool chosen elsewhere (a key, the palette) brings its tab.
        app.quick_tool = QuickTool::Measure(crate::measure_ui::Tool::Area);
        follow_tool(&mut app);
        assert_eq!(app.doc_tab, Tab::Measure);
        app.quick_tool = QuickTool::Comment(CommentTool::Rectangle);
        follow_tool(&mut app);
        assert_eq!(app.doc_tab, Tab::Draw);
        // Shapes are in Annotate's menu too: Annotate stays.
        app.doc_tab = Tab::Annotate;
        follow_tool(&mut app);
        assert_eq!(app.doc_tab, Tab::Annotate);
    }
}
