//! Window chrome: menu bar, toolbar, header card, status bar, banners,
//! filter panel and the scanning / empty / about views.

use super::{App, Cmd, Source, Tab, icons};
use crate::config::Settings;
use crate::i18n::tr;
use crate::format::{self, SizeUnit};
use crate::theme::{self, Palette};
use eframe::egui::{self, Align, Align2, Color32, CornerRadius, Layout, Margin, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};
use std::sync::atomic::Ordering;

// ------------------------------------------------------------------ widgets

pub fn primary_button(ui: &mut Ui, pal: &Palette, label: &str, icon: fn(&egui::Painter, Pos2, f32, Color32)) -> Response {
    let font = theme::semibold(13.0);
    let galley = ui.painter().layout_no_wrap(tr(label), font.clone(), Color32::WHITE);
    let size = Vec2::new(galley.size().x + 44.0, 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let base = pal.accent;
    let fill = if resp.is_pointer_button_down_on() {
        theme::lerp(base, Color32::BLACK, 0.15)
    } else if resp.hovered() {
        theme::lerp(base, Color32::WHITE, 0.10)
    } else {
        base
    };
    let p = ui.painter();
    p.rect_filled(rect.translate(Vec2::new(0.0, 1.0)), CornerRadius::same(9), Color32::from_black_alpha(30));
    p.rect_filled(rect, CornerRadius::same(9), fill);
    icon(p, Pos2::new(rect.left() + 18.0, rect.center().y), 16.0, Color32::WHITE);
    p.galley(Pos2::new(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn ghost_button(ui: &mut Ui, pal: &Palette, label: &str) -> Response {
    let font = theme::medium(12.5);
    let galley = ui.painter().layout_no_wrap(tr(label), font, pal.text);
    let size = Vec2::new(galley.size().x + 24.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.hovered() { pal.hover } else { pal.raised };
    ui.painter().rect(rect, CornerRadius::same(8), fill, Stroke::new(1.0, theme::lerp(pal.raised, pal.border, 0.7)), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, pal.text);
    resp
}

pub fn icon_button(ui: &mut Ui, pal: &Palette, active: bool, tip: &str, draw: impl FnOnce(&egui::Painter, Pos2, Color32)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(32.0, 32.0), Sense::click());
    if active {
        ui.painter().rect_filled(rect, CornerRadius::same(8), theme::with_alpha(pal.accent, 0.16));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(8), pal.hover);
    }
    draw(ui.painter(), rect.center(), if active { pal.accent } else { pal.text2 });
    resp.on_hover_text(tip)
}

pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, pal: &Palette, value: &mut T, items: &[(T, &str)]) -> bool {
    let font = theme::medium(12.5);
    let labels: Vec<String> = items.iter().map(|(_, l)| tr(l)).collect();
    let widths: Vec<f32> = labels.iter().map(|l| ui.painter().layout_no_wrap(l.clone(), font.clone(), pal.text).size().x + 28.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 30.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(9), theme::lerp(pal.chrome, pal.text3, 0.12));
    let mut x = rect.left() + 2.0;
    let mut changed = false;
    for (i, (v, label)) in items.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(x, rect.top() + 2.0), Vec2::new(widths[i], rect.height() - 4.0));
        let resp = ui.interact(r, ui.scope_id().with(("seg", i, *label)), Sense::click());
        let sel = *value == *v;
        if sel {
            p.rect_filled(r.translate(Vec2::new(0.0, 0.5)), CornerRadius::same(7), Color32::from_black_alpha(if pal.dark { 50 } else { 18 }));
            p.rect_filled(r, CornerRadius::same(7), if pal.dark { pal.raised } else { pal.card });
        } else if resp.hovered() {
            p.rect_filled(r, CornerRadius::same(7), theme::with_alpha(pal.text3, 0.12));
        }
        p.text(r.center(), Align2::CENTER_CENTER, &labels[i], font.clone(), if sel { pal.text } else { pal.text2 });
        if resp.clicked() && !sel {
            *value = *v;
            changed = true;
        }
        x += widths[i];
    }
    changed
}

pub fn card(ui: &mut Ui, pal: &Palette, add: impl FnOnce(&mut Ui)) -> Response {
    egui::Frame::new()
        .fill(pal.card)
        .stroke(Stroke::new(1.0, pal.separator))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(0))
        .show(ui, add)
        .response
}

fn shortcut_label(s: &str) -> String {
    if cfg!(target_os = "macos") { s.replace("Ctrl+", "⌘").replace("Alt+", "⌥").replace("Shift+", "⇧") } else { s.to_string() }
}

fn item(ui: &mut Ui, label: &str, sc: &str) -> bool {
    let b = egui::Button::new(tr(label)).shortcut_text(shortcut_label(sc));
    let r = ui.add(b);
    if r.clicked() {
        ui.close();
        true
    } else {
        false
    }
}

fn check(ui: &mut Ui, label: &str, sc: &str, on: bool) -> bool {
    let b = egui::Button::new(tr(label)).shortcut_text(shortcut_label(sc)).selected(on);
    let r = ui.add(b);
    if r.clicked() {
        ui.close();
        true
    } else {
        false
    }
}

// ------------------------------------------------------------------ menu bar

pub fn menu_bar(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let ctx = ui.ctx().clone();
    egui::Panel::top("menubar")
        .frame(egui::Frame::new().fill(pal.chrome).inner_margin(Margin::symmetric(8, 2)))
        .show(ui, |ui| {
            let mut cmd: Option<Cmd> = None;
            egui::MenuBar::new().ui(ui, |ui| {
                ui.spacing_mut().button_padding = egui::vec2(8.0, 3.0);
                ui.menu_button(tr("File"), |ui| {
                    if item(ui, "Scan", "F5") { cmd = Some(Cmd::Scan) }
                    if item(ui, "Select Folder…", "F3") { cmd = Some(Cmd::SelectFolder) }
                    if item(ui, "Select Multiple…", "Alt+F3") { cmd = Some(Cmd::SelectMultiple) }
                    if item(ui, "Select CSV File…", "Shift+F3") { cmd = Some(Cmd::SelectCsv) }
                    if item(ui, "New Window", "Ctrl+N") { cmd = Some(Cmd::NewInstance) }
                    ui.separator();
                    if item(ui, "Export to CSV File…", "Ctrl+Alt+E") { cmd = Some(Cmd::ExportCsv) }
                    if item(ui, "Export File Types to CSV File…", "Ctrl+Alt+F") { cmd = Some(Cmd::ExportFileTypes) }
                    if item(ui, "Save Treemap Image as PNG…", "Ctrl+S") { cmd = Some(Cmd::SaveTreemapImage) }
                    if item(ui, "Copy Visible Tree Structure", "") { cmd = Some(Cmd::CopyVisibleTree) }
                    ui.separator();
                    if item(ui, "Restart as Administrator…", "") { cmd = Some(Cmd::RestartAdmin) }
                    if item(ui, "Reset Configuration to Default Values…", "") { cmd = Some(Cmd::ResetConfig) }
                    ui.separator();
                    if item(ui, "Exit", "Alt+F4") { cmd = Some(Cmd::Exit) }
                });
                ui.menu_button(tr("Edit"), |ui| {
                    if item(ui, "Open", "Enter") { cmd = Some(Cmd::Open) }
                    if item(ui, reveal_label(), "Ctrl+E") { cmd = Some(Cmd::Reveal) }
                    if item(ui, terminal_label(), "Ctrl+P") { cmd = Some(Cmd::Terminal) }
                    ui.separator();
                    if item(ui, "Copy", "Ctrl+C") { cmd = Some(Cmd::Copy) }
                    if item(ui, "Cut", "Ctrl+X") { cmd = Some(Cmd::Cut) }
                    if item(ui, "Copy Path", "Ctrl+Alt+C") { cmd = Some(Cmd::CopyPath) }
                    if item(ui, "Copy File and Size Info", "Ctrl+Shift+C") { cmd = Some(Cmd::CopyFileInfo) }
                    ui.separator();
                    if item(ui, "Rename", "F2") { cmd = Some(Cmd::Rename) }
                    if item(ui, trash_label(), "Del") { cmd = Some(Cmd::DeleteToTrash) }
                    if item(ui, "Delete Permanently", "Shift+Del") { cmd = Some(Cmd::DeletePermanently) }
                    ui.separator();
                    if item(ui, "Properties", "Alt+Enter") { cmd = Some(Cmd::Properties) }
                    if item(ui, "Find Files…", "Ctrl+F") { cmd = Some(Cmd::FocusSearch) }
                });
                ui.menu_button(tr("View"), |ui| {
                    if check(ui, "Show Header", "Ctrl+H", app.s.show_header) { cmd = Some(Cmd::ToggleHeader) }
                    if check(ui, "Show Treemap", "F9", app.s.show_treemap) { cmd = Some(Cmd::ToggleTreemap) }
                    if check(ui, "Show File Types", "F8", app.s.show_file_types) { cmd = Some(Cmd::ToggleFileTypes) }
                    if check(ui, "Filter Scan Results", "Ctrl+Shift+F", app.s.show_filter_panel) { cmd = Some(Cmd::ToggleFilterPanel) }
                    ui.separator();
                    if item(ui, "Reset Tree Expansion", "F4") { cmd = Some(Cmd::ResetTreeExpansion) }
                    if item(ui, "Expand All", "Shift+F4") { cmd = Some(Cmd::ExpandAll) }
                    if item(ui, "Select Parent", "F6") { cmd = Some(Cmd::SelectParent) }
                    if item(ui, "Reselect Child", "F7") { cmd = Some(Cmd::ReselectChild) }
                    ui.separator();
                    ui.label(RichText::new("Size Units").color(pal.text3).small());
                    for (u, l) in [
                        (SizeUnit::Dynamic, "Dynamic"),
                        (SizeUnit::Bytes, "Bytes"),
                        (SizeUnit::KB, "KB"),
                        (SizeUnit::MB, "MB"),
                        (SizeUnit::GB, "GB"),
                        (SizeUnit::TB, "TB"),
                    ] {
                        if ui.add(egui::Button::new(l).selected(app.s.size_unit == u)).clicked() {
                            app.s.size_unit = u;
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.add(egui::Button::new("Binary units (1024)").selected(app.s.binary_units)).clicked() {
                        app.s.binary_units = true;
                        ui.close();
                    }
                    if ui.add(egui::Button::new("Decimal units (1000)").selected(!app.s.binary_units)).clicked() {
                        app.s.binary_units = false;
                        ui.close();
                    }
                });
                ui.menu_button(tr("Treemap"), |ui| {
                    if item(ui, "Zoom to Selected Folder", "F10") { cmd = Some(Cmd::ZoomTreemap) }
                    if item(ui, "Unzoom", "F11") { cmd = Some(Cmd::UnzoomTreemap) }
                    ui.separator();
                    if check(ui, "Show Allocated Size", "", app.s.treemap_allocated) { cmd = Some(Cmd::ToggleTreemapAllocated) }
                    if check(ui, "Show Free Space", "Ctrl+Shift+S", app.s.treemap_free_space) { cmd = Some(Cmd::ToggleTreemapFree) }
                    if check(ui, "Show Folder and File Names", "Ctrl+Shift+N", app.s.treemap_names) { cmd = Some(Cmd::ToggleTreemapNames) }
                    ui.separator();
                    ui.menu_button(tr("Color Scheme"), |ui| {
                        for sch in theme::SCHEMES {
                            if ui.add(egui::Button::new(sch.name).selected(app.s.treemap_scheme == sch.name)).clicked() {
                                app.s.treemap_scheme = sch.name.to_string();
                                app.tm.dirty = true;
                                ui.close();
                            }
                        }
                    });
                    if item(ui, "Colors…", "") { cmd = Some(Cmd::Appearance) }
                });
                ui.menu_button(tr("Options"), |ui| {
                    if check(ui, "Auto Scan After Making Selection", "", app.s.auto_scan) {
                        app.s.auto_scan = !app.s.auto_scan;
                    }
                    if check(ui, "Monitor File System for Changes", "", app.s.monitor_changes) {
                        app.s.monitor_changes = !app.s.monitor_changes;
                        if !app.s.monitor_changes {
                            app.monitor = None;
                        }
                    }
                    if check(ui, "Stay on One File System", "", app.s.one_filesystem) {
                        app.s.one_filesystem = !app.s.one_filesystem;
                    }
                    ui.separator();
                    if item(ui, "Appearance…", "") { cmd = Some(Cmd::Appearance) }
                    if item(ui, "Tweaks…", "") { cmd = Some(Cmd::Tweaks) }
                    if item(ui, "Language…", "") { cmd = Some(Cmd::Language) }
                });
                ui.menu_button(tr("Help"), |ui| {
                    if item(ui, "About & Search Syntax", "") { cmd = Some(Cmd::About) }
                });
            });
            if let Some(c) = cmd {
                app.run(c, &ctx);
            }
        });
}

pub fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else if cfg!(windows) {
        "Explore Folder"
    } else {
        "Show in File Manager"
    }
}
pub fn terminal_label() -> &'static str {
    if cfg!(windows) { "Command Prompt Here" } else { "Open Terminal Here" }
}
pub fn trash_label() -> &'static str {
    if cfg!(windows) { "Delete (to Recycle Bin)" } else { "Move to Trash" }
}

// ------------------------------------------------------------------ toolbar

pub fn toolbar(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let ctx = ui.ctx().clone();
    egui::Panel::top("toolbar")
        .frame(egui::Frame::new().fill(pal.chrome).inner_margin(Margin { left: 14, right: 14, top: 8, bottom: 10 }))
        .show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().line_segment(
                [Pos2::new(r.left() - 14.0, r.bottom() + 10.0), Pos2::new(r.right() + 14.0, r.bottom() + 10.0)],
                Stroke::new(1.0, pal.separator),
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                source_picker(app, ui);
                if app.scan.is_some() {
                    if primary_button(ui, &pal, "Cancel", icons::stop).clicked() {
                        app.run(Cmd::Cancel, &ctx);
                    }
                } else if primary_button(ui, &pal, "Scan", icons::play).on_hover_text("Scan (F5)").clicked() {
                    app.run(Cmd::Scan, &ctx);
                }
                ui.add_space(8.0);
                let mut tab = app.tab;
                if segmented(ui, &pal, &mut tab, &[(Tab::Tree, "Tree View"), (Tab::Files, "File View"), (Tab::About, "About")]) {
                    app.tab = tab;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if icon_button(ui, &pal, false, "Appearance", |p, c, col| icons::gear(p, c, 18.0, col)).clicked() {
                        app.run(Cmd::Appearance, &ctx);
                    }
                    if icon_button(ui, &pal, app.s.show_filter_panel, "Filter Scan Results (Ctrl+Shift+F)", |p, c, col| icons::filter(p, c, 18.0, col)).clicked() {
                        app.run(Cmd::ToggleFilterPanel, &ctx);
                    }
                    ui.add_space(6.0);
                    search_field(app, ui);
                });
            });
        });
}

fn search_field(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let w = (ui.available_width() - 8.0).clamp(160.0, 340.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 32.0), Sense::hover());
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(9), if pal.dark { pal.raised } else { pal.card }, Stroke::new(1.0, theme::lerp(pal.raised, pal.border, 0.8)), StrokeKind::Inside);
    icons::search(p, Pos2::new(rect.left() + 16.0, rect.center().y), 16.0, pal.text3);
    let te_rect = Rect::from_min_max(Pos2::new(rect.left() + 30.0, rect.top() + 3.0), Pos2::new(rect.right() - 8.0, rect.bottom() - 3.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(te_rect).layout(Layout::left_to_right(Align::Center)));
    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
        let mut job = highlight_job(text.as_str(), &app_pal(ui), ui);
        job.wrap.max_width = wrap;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let te = egui::TextEdit::singleline(&mut app.fv.query)
        .hint_text("Search files  (*.mp4, >1gb, !temp …)")
        .frame(egui::Frame::NONE)
        .font(theme::regular(13.0))
        .desired_width(te_rect.width())
        .layouter(&mut layouter);
    let resp = child.add(te);
    if app.focus_search {
        resp.request_focus();
        app.focus_search = false;
    }
    if resp.changed() {
        app.fv.dirty = true;
        app.fv.last_edit = std::time::Instant::now();
        if app.tab != Tab::Files && !app.fv.query.is_empty() {
            app.tab = Tab::Files;
        }
    }
    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let q = app.fv.query.clone();
        Settings::push_history(&mut app.s.search_history, &q);
    }
}

fn app_pal(ui: &Ui) -> (Color32, Color32) {
    (ui.visuals().text_color(), ui.visuals().hyperlink_color)
}

/// Syntax-highlight special operators (WizTree's FilterSyntaxHighlighter).
pub fn highlight_job(text: &str, colors: &(Color32, Color32), _ui: &Ui) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let font = theme::regular(13.0);
    let ranges = crate::filter::highlight_ranges(text);
    let mut pos = 0;
    let fmt = |c: Color32, f: egui::FontId| egui::TextFormat { font_id: f, color: c, ..Default::default() };
    for r in ranges {
        if r.start > pos {
            job.append(&text[pos..r.start], 0.0, fmt(colors.0, font.clone()));
        }
        job.append(&text[r.clone()], 0.0, fmt(colors.1, theme::medium(13.0)));
        pos = r.end;
    }
    if pos < text.len() {
        job.append(&text[pos..], 0.0, fmt(colors.0, font));
    }
    job
}

fn source_picker(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let label = app.source.as_ref().map(|s| s.label()).unwrap_or_else(|| "Choose a location".into());
    let sub = match &app.source {
        Some(Source::Volume(p)) => app
            .volumes
            .iter()
            .find(|v| &v.path == p)
            .map(|v| format!("{} · {} free of {}", v.fs_type, app.fmt().size(v.free), app.fmt().size(v.total)))
            .unwrap_or_default(),
        Some(Source::Folder(_)) => "Folder".into(),
        Some(Source::Multi(v)) => v.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "),
        Some(Source::Csv(_)) => "CSV file".into(),
        None => String::new(),
    };
    let w = 300.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 36.0), Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() { pal.hover } else if pal.dark { pal.raised } else { pal.card };
    p.rect(rect, CornerRadius::same(10), fill, Stroke::new(1.0, theme::lerp(pal.raised, pal.border, 0.8)), StrokeKind::Inside);
    match &app.source {
        Some(Source::Folder(_)) | Some(Source::Multi(_)) => icons::folder(p, Pos2::new(rect.left() + 20.0, rect.center().y), 18.0, pal.accent),
        Some(Source::Csv(_)) => icons::file(p, Pos2::new(rect.left() + 20.0, rect.center().y), 18.0, pal.text3, pal.success),
        _ => icons::drive(p, Pos2::new(rect.left() + 20.0, rect.center().y), 18.0, pal.text2),
    }
    let tx = rect.left() + 38.0;
    let clip = p.with_clip_rect(Rect::from_min_max(Pos2::new(tx, rect.top()), Pos2::new(rect.right() - 26.0, rect.bottom())));
    clip.text(Pos2::new(tx, rect.top() + 11.0), Align2::LEFT_CENTER, &label, theme::semibold(12.5), pal.text);
    clip.text(Pos2::new(tx, rect.top() + 25.0), Align2::LEFT_CENTER, &sub, theme::regular(11.0), pal.text2);
    icons::chevron(p, Pos2::new(rect.right() - 14.0, rect.center().y), 10.0, true, pal.text2);
    let popup_id = ui.scope_id().with("source_popup");
    if resp.clicked() {
        egui::Popup::toggle_id(ui.ctx(), popup_id);
        app.volumes = crate::volumes::list_volumes();
    }
    let ctx = ui.ctx().clone();
    egui::Popup::new(popup_id, ui.ctx().clone(), egui::PopupAnchor::from(rect), ui.layer_id())
        .open_memory(None)
        .width(380.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(380.0);
            ui.label(RichText::new("VOLUMES").font(theme::semibold(10.5)).color(pal.text3));
            let vols = app.volumes.clone();
            let f = app.fmt();
            for v in vols {
                let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::click());
                let p = ui.painter();
                if resp.hovered() {
                    p.rect_filled(r, CornerRadius::same(8), pal.hover);
                }
                icons::drive(p, Pos2::new(r.left() + 20.0, r.center().y), 20.0, if v.is_boot { pal.accent } else { pal.text2 });
                let name = if v.label.is_empty() || cfg!(windows) { v.display.clone() } else { format!("{}  ({})", v.display, v.label) };
                let free_r = p.text(
                    Pos2::new(r.right() - 10.0, r.top() + 13.0),
                    Align2::RIGHT_CENTER,
                    format!("{} free of {}", f.size(v.free), f.size(v.total)),
                    theme::regular(11.0),
                    pal.text2,
                );
                let name_clip = Rect::from_min_max(Pos2::new(r.left() + 40.0, r.top()), Pos2::new(free_r.left() - 10.0, r.bottom()));
                p.with_clip_rect(name_clip).text(Pos2::new(r.left() + 40.0, r.top() + 13.0), Align2::LEFT_CENTER, name, theme::medium(12.5), pal.text);
                let bar = Rect::from_min_size(Pos2::new(r.left() + 40.0, r.top() + 27.0), Vec2::new(r.width() - 50.0, 5.0));
                p.rect_filled(bar, CornerRadius::same(3), theme::with_alpha(pal.text3, 0.25));
                let used = if v.total > 0 { 1.0 - v.free as f32 / v.total as f32 } else { 0.0 };
                let col = if used > 0.9 { pal.danger } else if used > 0.75 { pal.warning } else { pal.accent };
                p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * used, 5.0)), CornerRadius::same(3), col);
                p.text(Pos2::new(r.left() + 40.0, r.top() + 40.0), Align2::LEFT_CENTER, &v.fs_type, theme::regular(10.5), pal.text3);
                if resp.clicked() {
                    app.set_source(Source::Volume(v.path.clone()));
                    egui::Popup::close_id(&ctx, popup_id);
                }
            }
            ui.separator();
            let mut cmd = None;
            for (label, c) in [("Select Folder…", Cmd::SelectFolder), ("Select Multiple…", Cmd::SelectMultiple), ("Open CSV File…", Cmd::SelectCsv)] {
                if ui.add(egui::Button::new(label).frame(false).min_size(Vec2::new(ui.available_width(), 28.0))).clicked() {
                    cmd = Some(c);
                    egui::Popup::close_id(&ctx, popup_id);
                }
            }
            if let Some(c) = cmd {
                app.run(c, &ctx);
            }
        });
}

// ------------------------------------------------------------------ header

pub fn header(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let f = app.fmt();
    egui::Panel::top("header")
        .frame(egui::Frame::new().fill(pal.bg).inner_margin(Margin { left: 14, right: 14, top: 12, bottom: 2 }))
        .show(ui, |ui| {
            card(ui, &pal, |ui| {
                ui.set_min_height(74.0);
                let r = ui.max_rect();
                let (rect, _) = ui.allocate_exact_size(Vec2::new(r.width(), 74.0), Sense::hover());
                let p = ui.painter();
                let (vol, title, sub) = match &app.tree {
                    Some(t) => {
                        let title = if t.roots.len() == 1 { t.name(t.roots[0]).to_string() } else { format!("{} locations", t.roots.len()) };
                        let mut sub = t.volume.fs_type.clone();
                        if !t.volume.label.is_empty() {
                            if !sub.is_empty() {
                                sub.push_str(" · ");
                            }
                            sub.push_str(&t.volume.label);
                        }
                        if app.base_tree.is_some() {
                            sub.push_str(" · filtered");
                        }
                        (t.volume.clone(), title, sub)
                    }
                    None => (Default::default(), app.source.as_ref().map(|s| s.label()).unwrap_or_default(), "Not scanned yet".into()),
                };
                // Title block
                let x0 = rect.left() + 18.0;
                p.text(Pos2::new(x0, rect.top() + 24.0), Align2::LEFT_CENTER, &title, theme::semibold(17.0), pal.text);
                p.text(Pos2::new(x0, rect.top() + 47.0), Align2::LEFT_CENTER, &sub, theme::regular(11.5), pal.text2);
                // Capacity bar
                let bar_l = rect.left() + (rect.width() * 0.26).max(220.0);
                let stats_w = 430.0f32.min(rect.width() * 0.42);
                let bar_r = rect.right() - stats_w - 24.0;
                if bar_r > bar_l + 60.0 {
                    let bar = Rect::from_min_max(Pos2::new(bar_l, rect.center().y - 5.0), Pos2::new(bar_r, rect.center().y + 5.0));
                    p.rect_filled(bar, CornerRadius::same(5), theme::with_alpha(pal.text3, 0.22));
                    if vol.total > 0 {
                        let scanned = app.tree.as_ref().map(|t| t.total_alloc().max(t.total_size())).unwrap_or(0).min(vol.used());
                        let used = vol.used() as f32 / vol.total as f32;
                        let res = vol.reserved as f32 / vol.total as f32;
                        let sc = scanned as f32 / vol.total as f32;
                        let w = bar.width();
                        p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(w * used, 10.0)), CornerRadius::same(5), theme::with_alpha(pal.accent, 0.45));
                        p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(w * sc, 10.0)), CornerRadius::same(5), pal.accent);
                        if res > 0.0 {
                            p.rect_filled(
                                Rect::from_min_size(Pos2::new(bar.left() + w * used, bar.top()), Vec2::new((w * res).max(2.0), 10.0)),
                                CornerRadius::same(2),
                                pal.warning,
                            );
                        }
                        let pct = format!("{:.0}% used", used * 100.0);
                        p.text(Pos2::new(bar.left(), bar.top() - 11.0), Align2::LEFT_CENTER, pct, theme::medium(11.0), pal.text2);
                        p.text(
                            Pos2::new(bar.right(), bar.top() - 11.0),
                            Align2::RIGHT_CENTER,
                            format!("{} of {}", f.size(vol.used()), f.size(vol.total)),
                            theme::regular(11.0),
                            pal.text2,
                        );
                        if let Some(j) = &app.scan {
                            let exp = j.progress.expected.load(Ordering::Relaxed).max(1);
                            let done = j.progress.bytes.load(Ordering::Relaxed) as f32 / exp as f32;
                            p.text(Pos2::new(bar.left(), bar.bottom() + 12.0), Align2::LEFT_CENTER, format!("Rescanning… {:.0}%", (done * 100.0).min(99.0)), theme::regular(11.0), pal.accent);
                        } else if scanned > 0 && vol.used() > scanned + vol.total / 200 {
                            p.text(
                                Pos2::new(bar.left(), bar.bottom() + 12.0),
                                Align2::LEFT_CENTER,
                                format!("{} not in scan (other users, system, metadata)", f.size(vol.used() - scanned)),
                                theme::regular(10.5),
                                pal.text3,
                            );
                        }
                    }
                }
                // Stats
                let mut sx = rect.right() - stats_w;
                let stats: Vec<(&str, String, String)> = vec![
                    ("TOTAL", if vol.total > 0 { f.size(vol.total) } else { "n/a".into() }, String::new()),
                    (
                        "USED",
                        if vol.total > 0 { f.size(vol.used()) } else { "n/a".into() },
                        if vol.total > 0 { format!("{:.1}%", vol.used() as f64 / vol.total as f64 * 100.0) } else { String::new() },
                    ),
                    (
                        "FREE",
                        if vol.total > 0 { f.size(vol.free) } else { "n/a".into() },
                        if vol.total > 0 { format!("{:.1}%", vol.free as f64 / vol.total as f64 * 100.0) } else { String::new() },
                    ),
                    ("RESERVED", if vol.total > 0 { f.size(vol.reserved) } else { "n/a".into() }, String::new()),
                ];
                let cw = stats_w / stats.len() as f32;
                for (k, v, sub) in stats {
                    p.line_segment([Pos2::new(sx, rect.top() + 18.0), Pos2::new(sx, rect.bottom() - 18.0)], Stroke::new(1.0, pal.separator));
                    p.text(Pos2::new(sx + 14.0, rect.top() + 22.0), Align2::LEFT_CENTER, k, theme::semibold(10.0), pal.text3);
                    p.text(Pos2::new(sx + 14.0, rect.top() + 42.0), Align2::LEFT_CENTER, v, theme::semibold(14.0), pal.text);
                    if !sub.is_empty() {
                        p.text(Pos2::new(sx + 14.0, rect.top() + 59.0), Align2::LEFT_CENTER, sub, theme::regular(10.5), pal.text2);
                    }
                    sx += cw;
                }
            });
        });
}

// ------------------------------------------------------------------ status bar

pub fn status_bar(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let f = app.fmt();
    egui::Panel::bottom("status")
        .frame(egui::Frame::new().fill(pal.chrome).inner_margin(Margin::symmetric(14, 5)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                let dot = if app.scan.is_some() { pal.warning } else if app.tree.is_some() { pal.success } else { pal.text3 };
                let (r, _) = ui.allocate_exact_size(Vec2::new(8.0, 14.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 3.5, dot);
                let status = if let Some(j) = &app.scan {
                    format!(
                        "Scanning… Folders: {}  Files: {}",
                        format::thousands(j.progress.folders.load(Ordering::Relaxed)),
                        format::thousands(j.progress.files.load(Ordering::Relaxed))
                    )
                } else {
                    app.status.clone()
                };
                ui.label(RichText::new(status).font(theme::regular(11.5)).color(pal.text2));
                if let Some(t) = &app.tree {
                    let ids = app.targets();
                    if !ids.is_empty() {
                        let (mut files, mut size, mut alloc) = (0u64, 0u64, 0u64);
                        for &id in &ids {
                            let n = &t.nodes[id as usize];
                            files += if n.is_dir() { n.files as u64 } else { 1 };
                            size += n.size;
                            alloc += n.alloc;
                        }
                        ui.label(
                            RichText::new(format!(
                                "Selected Files: {}   Total Size: {}   Allocated: {}",
                                format::thousands(files),
                                f.size(size),
                                f.size(alloc)
                            ))
                            .font(theme::regular(11.5))
                            .color(pal.text2),
                        );
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !app.hover_path.is_empty() {
                        ui.add(egui::Label::new(RichText::new(&app.hover_path).font(theme::regular(11.5)).color(pal.text3)).truncate());
                    }
                });
            });
        });
}

// ------------------------------------------------------------------ banners

pub fn banners(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let ctx = ui.ctx().clone();
    let denied = app.tree.as_ref().map(|t| t.denied.len()).unwrap_or(0);
    let show_admin = cfg!(windows) && !crate::actions::is_elevated() && !app.s.dont_show_admin_warning && !app.admin_dismissed;
    let show_denied = denied > 0 && !app.denied_dismissed && app.scan.is_none();
    if !show_admin && !show_denied {
        return;
    }
    egui::Panel::top("banners")
        .frame(egui::Frame::new().fill(pal.bg).inner_margin(Margin { left: 14, right: 14, top: 10, bottom: 0 }))
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(theme::with_alpha(pal.warning, if pal.dark { 0.14 } else { 0.12 }))
                .stroke(Stroke::new(1.0, theme::with_alpha(pal.warning, 0.35)))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                        icons::warning(ui.painter(), r.center(), 18.0, pal.warning);
                        let text = if show_admin {
                            "Administrator rights enable high-speed NTFS scanning (reading the MFT directly) and access to all system files and folders.".to_string()
                        } else {
                            format!(
                                "{} folder{} couldn't be read during the scan (permission denied). Their contents aren't included.",
                                format::thousands(denied as u64),
                                if denied == 1 { "" } else { "s" }
                            )
                        };
                        ui.label(RichText::new(text).color(pal.text).font(theme::regular(12.5)));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(egui::Button::new("Dismiss").frame(false)).clicked() {
                                if show_admin {
                                    app.admin_dismissed = true;
                                } else {
                                    app.denied_dismissed = true;
                                }
                            }
                            if show_admin {
                                if ui.add(egui::Button::new("Don't show again").frame(false)).clicked() {
                                    app.s.dont_show_admin_warning = true;
                                }
                            } else if ui.add(egui::Button::new("Show folders").frame(false)).clicked() {
                                app.dlg.denied = true;
                            }
                            if ghost_button(ui, &pal, "Restart as Administrator").clicked() {
                                app.run(Cmd::RestartAdmin, &ctx);
                            }
                        });
                    });
                });
        });
}

// ------------------------------------------------------------------ filter panel

pub fn filter_panel(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let ctx = ui.ctx().clone();
    egui::Panel::top("filter_panel")
        .frame(egui::Frame::new().fill(pal.bg).inner_margin(Margin { left: 14, right: 14, top: 12, bottom: 0 }))
        .show(ui, |ui| {
            card(ui, &pal, |ui| {
                egui::Frame::new().inner_margin(Margin::symmetric(16, 12)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Filter Scan Results").font(theme::semibold(13.0)));
                        ui.add_space(12.0);
                        ui.radio_value(&mut app.s.filter_match_path, false, "Match file name only");
                        ui.radio_value(&mut app.s.filter_match_path, true, "Match entire path");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if primary_button(ui, &pal, "Apply Filter", icons::filter).on_hover_text("Shift+F5").clicked() {
                                app.run(Cmd::ApplyFilter, &ctx);
                            }
                            if ghost_button(ui, &pal, "Clear").clicked() {
                                app.run(Cmd::ClearFilter, &ctx);
                            }
                        });
                    });
                    ui.add_space(6.0);
                    let hist_inc = app.s.include_history.clone();
                    let hist_exc = app.s.exclude_history.clone();
                    filter_row(ui, &pal, "Include", &mut app.filter_include, &hist_inc, "e.g.  *.mp4|*.mkv   >100mb   \\Videos\\");
                    filter_row(ui, &pal, "Exclude", &mut app.filter_exclude, &hist_exc, "e.g.  *.tmp   node_modules");
                });
            });
        });
}

fn filter_row(ui: &mut Ui, pal: &Palette, label: &str, text: &mut String, history: &[String], hint: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(Vec2::new(64.0, 26.0), egui::Label::new(RichText::new(label).color(pal.text2)));
        let w = ui.available_width() - 40.0;
        let colors = (pal.text, pal.accent);
        let mut layouter = |ui: &Ui, t: &dyn egui::TextBuffer, wrap: f32| {
            let mut job = highlight_job(t.as_str(), &colors, ui);
            job.wrap.max_width = wrap;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        let te = egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(w)
            .margin(Margin::symmetric(8, 5))
            .layouter(&mut layouter);
        ui.add(te);
        let resp = icon_button(ui, pal, false, "History", |p, c, col| icons::chevron(p, c, 14.0, true, col));
        let popup_id = ui.scope_id().with(("filter_hist", label));
        if resp.clicked() {
            egui::Popup::toggle_id(ui.ctx(), popup_id);
        }
        let ctx = ui.ctx().clone();
        egui::Popup::new(popup_id, ctx.clone(), egui::PopupAnchor::from(resp.rect), ui.layer_id())
            .open_memory(None)
            .align(egui::RectAlign::BOTTOM_END)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(260.0);
                if history.is_empty() {
                    ui.label(RichText::new("No history yet").color(pal.text3));
                }
                for h in history {
                    if ui.add(egui::Button::new(h.as_str()).frame(false).min_size(Vec2::new(ui.available_width(), 24.0))).clicked() {
                        *text = h.clone();
                        egui::Popup::close_id(&ctx, popup_id);
                    }
                }
            });
    });
}

// ------------------------------------------------------------------ views

pub fn scanning_view(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let Some(j) = &app.scan else { return };
    let t = ui.input(|i| i.time);
    let r = ui.max_rect();
    let card_r = Rect::from_center_size(r.center(), Vec2::new(520.0f32.min(r.width() - 20.0), 250.0));
    let p = ui.painter();
    p.rect_filled(card_r.translate(Vec2::new(0.0, 6.0)), CornerRadius::same(18), Color32::from_black_alpha(if pal.dark { 60 } else { 16 }));
    p.rect(card_r, CornerRadius::same(18), pal.card, Stroke::new(1.0, pal.separator), StrokeKind::Inside);
    icons::spinner(p, Pos2::new(card_r.center().x, card_r.top() + 46.0), 34.0, pal.accent, t);
    p.text(Pos2::new(card_r.center().x, card_r.top() + 88.0), Align2::CENTER_CENTER, format!("Scanning {}", j.source.label()), theme::semibold(17.0), pal.text);
    let files = j.progress.files.load(Ordering::Relaxed);
    let folders = j.progress.folders.load(Ordering::Relaxed);
    let bytes = j.progress.bytes.load(Ordering::Relaxed);
    let exp = j.progress.expected.load(Ordering::Relaxed);
    let f = app.fmt();
    p.text(
        Pos2::new(card_r.center().x, card_r.top() + 116.0),
        Align2::CENTER_CENTER,
        format!("{} folders  ·  {} files  ·  {}", format::thousands(folders), format::thousands(files), f.size(bytes)),
        theme::regular(12.5),
        pal.text2,
    );
    let bar = Rect::from_center_size(Pos2::new(card_r.center().x, card_r.top() + 148.0), Vec2::new(card_r.width() - 80.0, 6.0));
    p.rect_filled(bar, CornerRadius::same(3), theme::with_alpha(pal.text3, 0.22));
    let frac = if exp > 0 { (bytes as f32 / exp as f32).min(0.99) } else { ((t * 0.7).sin() as f32 * 0.5 + 0.5) * 0.3 };
    p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * frac, 6.0)), CornerRadius::same(3), pal.accent);
    let cur = j.progress.current.lock().clone();
    let clip = p.with_clip_rect(card_r.shrink(20.0));
    clip.text(Pos2::new(card_r.center().x, card_r.top() + 172.0), Align2::CENTER_CENTER, cur, theme::regular(11.0), pal.text3);
    p.text(
        Pos2::new(card_r.center().x, card_r.top() + 196.0),
        Align2::CENTER_CENTER,
        format!("{:.1}s", j.started.elapsed().as_secs_f32()),
        theme::regular(11.0),
        pal.text3,
    );
    let br = Rect::from_center_size(Pos2::new(card_r.center().x, card_r.bottom() - 26.0), Vec2::new(90.0, 28.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(br));
    if ghost_button(&mut child, &pal, "Cancel").clicked() {
        app.cancel_scan();
    }
    ui.ctx().request_repaint();
}

pub fn empty_state(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let ctx = ui.ctx().clone();
    let r = ui.max_rect();
    let c = r.center();
    let p = ui.painter();
    // App mark: a little treemap.
    let mark = Rect::from_center_size(Pos2::new(c.x, c.y - 90.0), Vec2::splat(72.0));
    p.rect_filled(mark, CornerRadius::same(18), pal.accent);
    let inner = mark.shrink(14.0);
    let cols = [Color32::WHITE, Color32::from_white_alpha(200), Color32::from_white_alpha(150), Color32::from_white_alpha(110)];
    let a = Rect::from_min_max(inner.min, Pos2::new(inner.center().x + 2.0, inner.max.y));
    let b = Rect::from_min_max(Pos2::new(a.max.x + 3.0, inner.min.y), Pos2::new(inner.max.x, inner.center().y));
    let cc = Rect::from_min_max(Pos2::new(a.max.x + 3.0, b.max.y + 3.0), Pos2::new(inner.center().x + 13.0, inner.max.y));
    let d = Rect::from_min_max(Pos2::new(cc.max.x + 3.0, b.max.y + 3.0), inner.max);
    for (rr, col) in [a, b, cc, d].iter().zip(cols) {
        p.rect_filled(*rr, CornerRadius::same(3), col);
    }
    p.text(Pos2::new(c.x, c.y - 20.0), Align2::CENTER_CENTER, "Ready to Scan", theme::semibold(22.0), pal.text);
    p.text(Pos2::new(c.x, c.y + 10.0), Align2::CENTER_CENTER, "Choose a drive or folder above, then click Scan.", theme::regular(13.5), pal.text2);
    p.text(
        Pos2::new(c.x, c.y + 32.0),
        Align2::CENTER_CENTER,
        "Tree View shows a tree and treemap; File View is a searchable list of every file.",
        theme::regular(12.0),
        pal.text3,
    );
    let br = Rect::from_center_size(Pos2::new(c.x, c.y + 76.0), Vec2::new(110.0, 32.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(br).layout(Layout::left_to_right(Align::Center)));
    if app.source.is_some() && primary_button(&mut child, &pal, "Scan", icons::play).clicked() {
        app.run(Cmd::Scan, &ctx);
    }
}

pub fn about(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.set_max_width(780.0);
            ui.add_space(20.0);
            card(ui, &pal, |ui| {
                egui::Frame::new().inner_margin(Margin::same(28)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("WizTree").font(theme::semibold(28.0)));
                    ui.label(RichText::new(format!("Cross-platform disk space analyzer · version {}", env!("CARGO_PKG_VERSION"))).color(pal.text2));
                    ui.add_space(14.0);
                    ui.label(
                        "Scans a whole drive once, in parallel, into a compact in-memory index. Every view — the tree, \
                         the treemap, the file list, search and duplicates — then works instantly from that index, \
                         without touching the disk again.",
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Take care not to delete files required by your operating system or applications.").color(pal.warning),
                    );
                });
            });
            ui.add_space(14.0);
            card(ui, &pal, |ui| {
                egui::Frame::new().inner_margin(Margin::same(24)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Search & Filter Syntax").font(theme::semibold(16.0)));
                    ui.add_space(8.0);
                    let rows = [
                        ("word", "Names containing “word” (case-insensitive)"),
                        ("one two", "Both terms must match (AND)"),
                        ("*.mp4|*.mkv", "Either alternative (OR)"),
                        ("\"two words\"", "Quotes keep spaces in a term"),
                        ("!*.tmp", "Exclude matches"),
                        ("*.iso   report_??.pdf", "Wildcards * and ?"),
                        ("/Videos/  or  \\Videos\\", "A path separator matches against the full path"),
                        (">1gb   <=10k   s>500m", "Size comparisons (B, K, M, G, T)"),
                        ("a>1g", "Allocated size"),
                        (">2024/01/31   <2020/01/01-12:00", "Modified date comparisons"),
                        ("=folder   =file", "Only folders / only files"),
                        ("namelen>100   pathlen>250", "Name / path length"),
                        ("dupcount>2   dupsize>1g", "Duplicate count / size (File View with duplicates on)"),
                        ("\\^IMG_\\d+\\.jpg$\\", "Regular expression between backslashes"),
                    ];
                    egui::Grid::new("syntax").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
                        for (a, b) in rows {
                            ui.label(RichText::new(a).monospace().color(pal.accent));
                            ui.label(RichText::new(b).color(pal.text2));
                            ui.end_row();
                        }
                    });
                });
            });
            ui.add_space(14.0);
            card(ui, &pal, |ui| {
                egui::Frame::new().inner_margin(Margin::same(24)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Keyboard Shortcuts").font(theme::semibold(16.0)));
                    ui.add_space(8.0);
                    let rows = [
                        ("F5", "Scan"),
                        ("F3 / Shift+F3 / Alt+F3", "Select folder / CSV file / multiple"),
                        ("F4 / Shift+F4", "Reset tree expansion / Expand all"),
                        ("F6 / F7", "Select parent / Reselect child"),
                        ("F8 / F9", "Show file types / Show treemap"),
                        ("F10 / F11", "Zoom / Unzoom treemap"),
                        ("Ctrl+F", "Search files"),
                        ("Ctrl+E / Ctrl+P", "Show in file manager / Terminal here"),
                        ("Ctrl+Alt+C", "Copy path"),
                        ("Del / Shift+Del", "Move to trash / Delete permanently"),
                        ("F2", "Rename"),
                        ("Ctrl+Alt+E / Ctrl+Alt+F", "Export CSV / Export file types"),
                        ("Ctrl+S", "Save treemap image"),
                        ("Ctrl+Shift+F", "Filter scan results"),
                        ("Ctrl+H", "Show header"),
                    ];
                    egui::Grid::new("keys").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
                        for (a, b) in rows {
                            ui.label(RichText::new(shortcut_label(a)).font(theme::medium(12.5)));
                            ui.label(RichText::new(b).color(pal.text2));
                            ui.end_row();
                        }
                    });
                });
            });
            ui.add_space(20.0);
        });
    });
}
