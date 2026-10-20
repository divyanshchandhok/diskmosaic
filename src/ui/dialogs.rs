//! Modal dialogs.

use super::{App, Confirm, Source, Tab, chrome};
use crate::config::{ColorBy, Settings, COLOR_BLIND_PALETTE, DEFAULT_PALETTE, default_ext_lists};
use crate::csvio::ExportOptions;
use crate::format;
use crate::model::NodeId;
use crate::theme::{self, Palette};
use crate::volumes::VolumeInfo;
use eframe::egui::{self, Align, Color32, CornerRadius, Id, Layout, Margin, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};
use std::path::PathBuf;

#[derive(Default)]
pub struct Dialogs {
    pub multi: Option<Vec<(PathBuf, String, bool, bool)>>, // (path, label, checked, custom)
    pub export: Option<ExportState>,
    pub treemap_export: bool,
    pub appearance: bool,
    pub tweaks: Option<Settings>,
    pub language: bool,
    pub denied: bool,
    pub ntfs: Option<NtfsAccess>,
}

/// Prompt for raw read access to an NTFS device (Linux fast scan).
pub struct NtfsAccess {
    pub dev: PathBuf,
    pub password: String,
    pub remember: bool,
    pub error: Option<String>,
}

pub struct ExportState {
    pub from_files: bool,
    pub selected_only: bool,
    pub path: String,
}

impl Dialogs {
    pub fn none_open(&self) -> bool {
        self.multi.is_none() && self.export.is_none() && !self.treemap_export && !self.appearance && self.tweaks.is_none() && !self.language && !self.denied && self.ntfs.is_none()
    }

    pub fn open_multi(&mut self, s: &Settings, vols: &[VolumeInfo]) {
        let mut items: Vec<(PathBuf, String, bool, bool)> = vols
            .iter()
            .map(|v| (v.path.clone(), v.display.clone(), s.multi_checked.contains(&v.path.display().to_string()), false))
            .collect();
        for f in &s.custom_folders {
            items.push((PathBuf::from(f), f.clone(), s.multi_checked.contains(f), true));
        }
        self.multi = Some(items);
    }

    pub fn open_export(&mut self, s: &Settings, from_files: bool) {
        let dir = if s.export_dir.is_empty() {
            directories::UserDirs::new().and_then(|u| u.document_dir().map(|d| d.to_path_buf())).unwrap_or_else(std::env::temp_dir)
        } else {
            PathBuf::from(&s.export_dir)
        };
        let name = format!("WizTree_{}.csv", chrono::Local::now().format("%Y%m%d%H%M%S"));
        self.export = Some(ExportState { from_files, selected_only: false, path: dir.join(name).display().to_string() });
    }

    pub fn open_tweaks(&mut self, s: &Settings) {
        self.tweaks = Some(s.clone());
    }
}

fn modal_frame(pal: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(pal.card)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(22))
        .shadow(egui::Shadow { offset: [0, 14], blur: 40, spread: 0, color: Color32::from_black_alpha(if pal.dark { 140 } else { 50 }) })
}

fn modal<R>(ctx: &egui::Context, pal: &Palette, id: &str, width: f32, title: &str, body: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let resp = egui::Modal::new(Id::unique(id))
        .frame(modal_frame(pal))
        .backdrop_color(Color32::from_black_alpha(if pal.dark { 120 } else { 70 }))
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new(title).font(theme::semibold(17.0)).color(pal.text));
            ui.add_space(10.0);
            body(ui)
        });
    let close = resp.should_close();
    (resp.inner, close)
}

fn buttons(ui: &mut Ui, pal: &Palette, ok: &str, cancel: Option<&str>) -> (bool, bool) {
    let mut r = (false, false);
    ui.add_space(14.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if chrome::primary_button(ui, pal, ok, |_, _, _, _| {}).clicked() {
            r.0 = true;
        }
        if let Some(c) = cancel {
            if chrome::ghost_button(ui, pal, c).clicked() {
                r.1 = true;
            }
        }
    });
    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !ui.ctx().egui_wants_keyboard_input() {
        r.0 = true;
    }
    r
}

fn section(ui: &mut Ui, pal: &Palette, title: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(title.to_uppercase()).font(theme::semibold(10.5)).color(pal.text3));
    ui.add_space(2.0);
}

pub fn show_all(app: &mut App, ctx: &egui::Context) {
    let pal = app.pal;
    if app.dlg.appearance {
        appearance(app, ctx);
    }
    if app.dlg.tweaks.is_some() {
        tweaks(app, ctx);
    }
    if app.dlg.export.is_some() {
        export(app, ctx);
    }
    if app.dlg.treemap_export {
        treemap_export(app, ctx);
    }
    if app.dlg.multi.is_some() {
        multi(app, ctx);
    }
    if app.dlg.language {
        language(app, ctx);
    }
    if app.dlg.ntfs.is_some() {
        ntfs_access(app, ctx);
    }
    if app.dlg.denied {
        let list: Vec<String> = app.tree.as_ref().map(|t| t.denied.iter().map(|&d| t.path(d)).collect()).unwrap_or_default();
        let ((), close) = modal(ctx, &pal, "denied", 560.0, "Folders Access Was Denied", |ui| {
            ui.label(RichText::new("These folders couldn't be read during the scan. Restarting with administrator rights usually fixes this.").color(pal.text2));
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for p in &list {
                    ui.add(egui::Label::new(RichText::new(p).monospace()).selectable(true));
                }
            });
            if buttons(ui, &pal, "Done", None).0 {
                app.dlg.denied = false;
            }
        });
        if close {
            app.dlg.denied = false;
        }
    }
    if let Some((id, mut name)) = app.rename.take() {
        let mut keep = true;
        let mut commit = false;
        let ((), close) = modal(ctx, &pal, "rename", 420.0, "Rename", |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY).margin(Margin::symmetric(8, 6)));
            if !r.has_focus() && !ui.memory(|m| m.has_focus(r.id)) && ui.input(|i| i.time) < 1e9 {
                r.request_focus();
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                commit = true;
            }
            let (ok, cancel) = buttons(ui, &pal, "Rename", Some("Cancel"));
            commit |= ok;
            if cancel {
                keep = false;
            }
        });
        if close {
            keep = false;
        }
        if commit {
            if !name.trim().is_empty() {
                app.do_rename(id, name.trim());
            }
        } else if keep {
            app.rename = Some((id, name));
        }
    }
    if let Some(id) = app.properties {
        properties(app, ctx, id);
    }
    if let Some(c) = app.confirm.take() {
        confirm(app, ctx, c);
    }
    if app.msg.is_some() {
        let (t, b, err) = {
            let m = app.msg.as_ref().unwrap();
            (m.title.clone(), m.body.clone(), m.error)
        };
        let ((), close) = modal(ctx, &pal, "msg", 440.0, &t, |ui| {
            ui.label(RichText::new(&b).color(if err { pal.text } else { pal.text2 }));
            if buttons(ui, &pal, "OK", None).0 {
                app.msg = None;
            }
        });
        if close {
            app.msg = None;
        }
    }
}

fn confirm(app: &mut App, ctx: &egui::Context, c: Confirm) {
    let pal = app.pal;
    let (title, body, ok) = match &c {
        Confirm::Delete { ids, permanent } => {
            let t = app.tree.as_ref().unwrap();
            let what = if ids.len() == 1 { format!("\"{}\"", t.name(ids[0])) } else { format!("{} items", ids.len()) };
            if *permanent {
                (format!("Permanently erase {what}?"), "You can't undo this action.".to_string(), "Delete")
            } else {
                (
                    format!("{} {what}?", if cfg!(windows) { "Move to the Recycle Bin:" } else { "Move to the Trash:" }),
                    "You can recover this until the trash is emptied.".to_string(),
                    "Move to Trash",
                )
            }
        }
        Confirm::OpenMany { ids, .. } => (format!("You are about to open {} windows — are you sure?", ids.len()), String::new(), "Open"),
        Confirm::ResetConfig => (
            "Reset All Settings?".into(),
            "This clears every saved setting — window layout, column widths, filters, colors and view options — and restarts the app.".into(),
            "Reset & Restart",
        ),
    };
    let mut keep = true;
    let mut go = false;
    let ((), close) = modal(ctx, &pal, "confirm", 440.0, &title, |ui| {
        if !body.is_empty() {
            ui.label(RichText::new(&body).color(pal.text2));
        }
        let (ok_c, cancel) = buttons(ui, &pal, ok, Some("Cancel"));
        go = ok_c;
        if cancel {
            keep = false;
        }
    });
    if close {
        keep = false;
    }
    if go {
        match c {
            Confirm::Delete { ids, permanent } => app.delete_now(ids, permanent),
            Confirm::OpenMany { ids, what } => match what {
                super::Cmd::Terminal => {
                    for id in ids {
                        let _ = crate::actions::terminal_here(&app.path_of(id));
                    }
                }
                super::Cmd::Reveal => app.open_ids(&ids, true),
                _ => app.open_ids(&ids, false),
            },
            Confirm::ResetConfig => {
                Settings::reset();
                crate::actions::restart_self();
            }
        }
    } else if keep {
        app.confirm = Some(c);
    }
}

fn properties(app: &mut App, ctx: &egui::Context, id: NodeId) {
    let pal = app.pal;
    let t = app.tree.as_ref().unwrap();
    let n = t.nodes[id as usize];
    let f = app.fmt();
    let rows: Vec<(&str, String)> = vec![
        ("Name", t.name(id).to_string()),
        ("Location", if t.is_root(id) { String::new() } else { t.path(t.parent(id)) }),
        ("Type", if n.is_dir() { "Folder".into() } else { crate::filetypes::describe(&t.exts.names[n.ext as usize]) }),
        ("Size", format!("{} ({} bytes)", f.size(n.size), format::thousands(n.size))),
        ("Allocated", format!("{} ({} bytes)", f.size(n.alloc), format::thousands(n.alloc))),
        ("Contains", if n.is_dir() { format!("{} files, {} folders", format::thousands(n.files as u64), format::thousands(n.folders as u64)) } else { String::new() }),
        ("Modified", format::date(n.mtime)),
        ("Created", format::date(n.ctime)),
        ("Accessed", format::date(n.atime)),
        ("Attributes", format::attrs(n.flags)),
        ("File ID", n.file_id.to_string()),
    ];
    let path = PathBuf::from(t.path(id));
    let mut open = true;
    let ((), close) = modal(ctx, &pal, "props", 520.0, "Properties", |ui| {
        egui::Grid::new("props_grid").num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
            for (k, v) in &rows {
                if v.is_empty() {
                    continue;
                }
                ui.label(RichText::new(*k).color(pal.text2));
                ui.add(egui::Label::new(v).selectable(true).wrap());
                ui.end_row();
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if chrome::ghost_button(ui, &pal, chrome::reveal_label()).clicked() {
                let _ = crate::actions::reveal(&path);
            }
            if chrome::ghost_button(ui, &pal, "Open").clicked() {
                let _ = crate::actions::open_path(&path);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if chrome::primary_button(ui, &pal, "Done", |_, _, _, _| {}).clicked() {
                    open = false;
                }
            });
        });
    });
    if close || !open {
        app.properties = None;
    }
}

// ------------------------------------------------------------------ appearance

fn theme_card(ui: &mut Ui, sel_pal: &Palette, p: &Palette, label: &str, selected: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(118.0, 94.0), Sense::click());
    let painter = ui.painter();
    let prev = Rect::from_min_size(r.min, Vec2::new(118.0, 70.0));
    painter.rect(prev, CornerRadius::same(10), p.bg, Stroke::new(if selected { 2.5 } else { 1.0 }, if selected { sel_pal.accent } else { sel_pal.border }), StrokeKind::Inside);
    let inner = prev.shrink(8.0);
    painter.rect_filled(Rect::from_min_size(inner.min, Vec2::new(inner.width(), 9.0)), CornerRadius::same(3), p.chrome);
    let card = Rect::from_min_max(Pos2::new(inner.left(), inner.top() + 13.0), inner.max);
    painter.rect_filled(card, CornerRadius::same(4), p.card);
    for i in 0..3 {
        let y = card.top() + 6.0 + i as f32 * 9.0;
        painter.rect_filled(Rect::from_min_size(Pos2::new(card.left() + 6.0, y), Vec2::new(card.width() * (0.7 - i as f32 * 0.15), 4.0)), CornerRadius::same(2), p.text3);
    }
    painter.rect_filled(Rect::from_min_size(Pos2::new(card.right() - 22.0, card.top() + 5.0), Vec2::new(16.0, 6.0)), CornerRadius::same(3), p.accent);
    painter.text(Pos2::new(r.center().x, r.bottom() - 10.0), egui::Align2::CENTER_CENTER, label, theme::medium(12.0), if selected { sel_pal.text } else { sel_pal.text2 });
    resp.clicked()
}

fn swatch_strip(ui: &mut Ui, pal: &Palette, colors: &[[u8; 3]], gain: f32, label: &str, selected: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(560.0), 34.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(r, CornerRadius::same(8), theme::with_alpha(pal.accent, 0.14));
    } else if resp.hovered() {
        p.rect_filled(r, CornerRadius::same(8), pal.hover);
    }
    p.text(Pos2::new(r.left() + 12.0, r.center().y), egui::Align2::LEFT_CENTER, label, theme::medium(12.5), pal.text);
    let x0 = r.left() + 130.0;
    let sw = ((r.width() - 140.0) / 13.0).min(28.0);
    for (i, c) in colors.iter().enumerate() {
        let g = (gain * 1.3).min(1.0);
        let col = Color32::from_rgb((c[0] as f32 * g) as u8, (c[1] as f32 * g) as u8, (c[2] as f32 * g) as u8);
        let rr = Rect::from_min_size(Pos2::new(x0 + i as f32 * sw, r.top() + 8.0), Vec2::new(sw - 3.0, 18.0));
        p.rect_filled(rr, CornerRadius::same(4), col);
    }
    resp.clicked()
}

fn appearance(app: &mut App, ctx: &egui::Context) {
    let pal = app.pal;
    let mut done = false;
    let system_dark = ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(true);
    let ((), close) = modal(ctx, &pal, "appearance", 640.0, "Appearance", |ui| {
        egui::ScrollArea::vertical().max_height(ctx.content_rect().height() - 200.0).show(ui, |ui| {
            section(ui, &pal, "Theme");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);
                let auto = theme::resolve("Auto", "", system_dark);
                if theme_card(ui, &pal, &auto, "Auto", app.s.theme == "Auto") {
                    app.s.theme = "Auto".into();
                }
                for t in theme::THEMES {
                    if theme_card(ui, &pal, t, t.name, app.s.theme == t.name) {
                        app.s.theme = t.name.to_string();
                    }
                }
            });
            section(ui, &pal, "Accent");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                let base = theme::resolve(&app.s.theme, "", system_dark).accent;
                let mut opts: Vec<(&str, Color32)> = vec![("", base)];
                opts.extend(theme::ACCENTS.iter().copied());
                for (name, c) in opts {
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                    let sel = app.s.accent == name;
                    ui.painter().circle_filled(r.center(), 11.0, c);
                    if sel {
                        ui.painter().circle_stroke(r.center(), 13.5, Stroke::new(2.0, pal.text));
                    }
                    if name.is_empty() {
                        ui.painter().circle_stroke(r.center(), 6.0, Stroke::new(1.5, Color32::WHITE));
                    }
                    if resp.on_hover_text(if name.is_empty() { "Theme default" } else { name }).clicked() {
                        app.s.accent = name.to_string();
                    }
                }
            });
            section(ui, &pal, "Treemap Colors");
            for sch in theme::SCHEMES {
                if swatch_strip(ui, &pal, &sch.colors, sch.gain, sch.name, app.s.treemap_scheme == sch.name) {
                    app.s.treemap_scheme = sch.name.to_string();
                    app.tm.dirty = true;
                }
            }
            if swatch_strip(ui, &pal, &app.s.palette.clone(), 0.72, "Custom", app.s.treemap_scheme == "Custom") {
                app.s.treemap_scheme = "Custom".into();
                app.tm.dirty = true;
            }
            if app.s.treemap_scheme == "Custom" {
                ui.horizontal_wrapped(|ui| {
                    for i in 0..13 {
                        let mut c = Color32::from_rgb(app.s.palette[i][0], app.s.palette[i][1], app.s.palette[i][2]);
                        if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                            app.s.palette[i] = [c.r(), c.g(), c.b()];
                            app.tm.dirty = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if chrome::ghost_button(ui, &pal, "Default").clicked() {
                        app.s.palette = DEFAULT_PALETTE.to_vec();
                        app.tm.dirty = true;
                    }
                    if chrome::ghost_button(ui, &pal, "Color Blind").clicked() {
                        app.s.palette = COLOR_BLIND_PALETTE.to_vec();
                        app.tm.dirty = true;
                    }
                });
            }
            ui.add_space(6.0);
            if ui.checkbox(&mut app.s.gradient, "Cushion shading (gradient colors in treemap)").changed() {
                app.tm.dirty = true;
            }
            section(ui, &pal, "Color By");
            let mut cb = app.s.color_by;
            if chrome::segmented(ui, &pal, &mut cb, &[(ColorBy::Size, "Size (12 largest file types)"), (ColorBy::Extension, "Extension lists")]) {
                app.s.color_by = cb;
                app.tm.dirty = true;
            }
            if app.s.color_by == ColorBy::Extension {
                ui.add_space(6.0);
                for i in 0..13 {
                    ui.horizontal(|ui| {
                        let mut c = Color32::from_rgb(app.s.ext_colors[i][0], app.s.ext_colors[i][1], app.s.ext_colors[i][2]);
                        if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                            app.s.ext_colors[i] = [c.r(), c.g(), c.b()];
                            app.tm.dirty = true;
                        }
                        if ui
                            .add(egui::TextEdit::singleline(&mut app.s.ext_lists[i]).hint_text("ext1,ext2,…").desired_width(f32::INFINITY))
                            .changed()
                        {
                            app.tm.dirty = true;
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if chrome::ghost_button(ui, &pal, "Default Extensions").clicked() {
                        app.s.ext_lists = default_ext_lists();
                        app.tm.dirty = true;
                    }
                    if chrome::ghost_button(ui, &pal, "Default Colors").clicked() {
                        app.s.ext_colors = DEFAULT_PALETTE.to_vec();
                        app.tm.dirty = true;
                    }
                    if chrome::ghost_button(ui, &pal, "Color Blind").clicked() {
                        app.s.ext_colors = COLOR_BLIND_PALETTE.to_vec();
                        app.tm.dirty = true;
                    }
                });
            }
        });
        done = buttons(ui, &pal, "Done", None).0;
    });
    if close || done {
        app.dlg.appearance = false;
        app.s.save();
    }
}

// ------------------------------------------------------------------ tweaks

fn tweaks(app: &mut App, ctx: &egui::Context) {
    let pal = app.pal;
    let mut t = app.dlg.tweaks.take().unwrap();
    let mut keep = true;
    let mut apply = false;
    let ((), close) = modal(ctx, &pal, "tweaks", 520.0, "Tweaks", |ui| {
        section(ui, &pal, "Behavior");
        ui.checkbox(&mut t.drag_drop, "Enable drag/drop");
        ui.checkbox(&mut t.double_click_open, "Open file on double click");
        ui.checkbox(&mut t.enable_rename, "Enable rename");
        ui.checkbox(&mut t.monitor_changes, "Monitor file system for changes");
        ui.checkbox(&mut t.max_modified_date, "Show max modified date of all sub folders/files as folder's last modified date");
        ui.checkbox(&mut t.one_filesystem, "Stay on one file system (don't descend into other volumes)");
        #[cfg(windows)]
        {
            ui.checkbox(&mut t.mft_for_folders, "Scan MFT (if possible) when a folder is selected for analysis");
            ui.label(
                RichText::new("Normally the MFT is only scanned when an entire drive is selected. This can speed up folders with many files, but can be slower for small folders.")
                    .small()
                    .color(pal.text3),
            );
        }
        section(ui, &pal, "Display");
        ui.checkbox(&mut t.horizontal_lines, "Show horizontal lines in grids");
        ui.checkbox(&mut t.alt_row_colors, "Alternate row colors");
        ui.checkbox(&mut t.gradient, "Enable gradient colors in treemap");
        ui.horizontal(|ui| {
            ui.label("Number of decimal places to display");
            ui.add(egui::Slider::new(&mut t.decimals, 0..=4));
        });
        ui.horizontal(|ui| {
            ui.label("File group minimum size");
            ui.add(egui::DragValue::new(&mut t.file_group_min).range(0..=100_000));
        });
        ui.label(
            RichText::new("Loose files in a folder are grouped into one \"[N Files in …]\" entry once there are at least this many. Set to 0 to disable file grouping.")
                .small()
                .color(pal.text3),
        );
        let (ok, cancel) = buttons(ui, &pal, "OK", Some("Cancel"));
        apply = ok;
        if cancel {
            keep = false;
        }
    });
    if close {
        keep = false;
    }
    if apply {
        let rescan_needed = t.max_modified_date != app.s.max_modified_date;
        if t.file_group_min != app.s.file_group_min {
            app.rows_dirty = true;
        }
        if t.gradient != app.s.gradient {
            app.tm.dirty = true;
        }
        if !t.monitor_changes {
            app.monitor = None;
        }
        app.s = t;
        app.s.save();
        crate::model::MAX_MTIME.store(app.s.max_modified_date, std::sync::atomic::Ordering::Relaxed);
        if rescan_needed && app.tree.is_some() {
            if let Some(tr) = &mut app.tree {
                tr.finalize();
            }
            app.tree_changed();
        }
    } else if keep {
        app.dlg.tweaks = Some(t);
    }
}

// ------------------------------------------------------------------ export

fn export(app: &mut App, ctx: &egui::Context) {
    let pal = app.pal;
    let mut st = app.dlg.export.take().unwrap();
    let mut keep = true;
    let mut go = false;
    let sel_label = {
        let ids = if st.from_files { app.fv.sel.iter().copied().collect::<Vec<_>>() } else { app.sel.clone() };
        match (ids.first(), &app.tree) {
            (Some(&i), Some(t)) if ids.len() == 1 => t.path(i),
            (Some(_), _) => format!("{} items", ids.len()),
            _ => "nothing selected".into(),
        }
    };
    let ((), close) = modal(ctx, &pal, "export", 560.0, "Export to CSV", |ui| {
        section(ui, &pal, "Source");
        ui.horizontal(|ui| {
            ui.radio_value(&mut st.from_files, false, "Tree View");
            ui.radio_value(&mut st.from_files, true, "File View");
            ui.add_space(12.0);
            ui.checkbox(&mut st.selected_only, "Selected only");
        });
        ui.label(RichText::new(format!("({sel_label})")).small().color(pal.text3));
        section(ui, &pal, "Options");
        let s = &mut app.s;
        ui.checkbox(&mut s.export_files, "Export Files");
        ui.checkbox(&mut s.export_folders, "Export Folders");
        ui.checkbox(&mut s.export_dates, "Export \"Last Accessed\" and \"Created\" dates");
        ui.checkbox(&mut s.export_folder_sizes, "Export total size and allocated size of folders (excluding sub folders)");
        ui.checkbox(&mut s.export_split_names, "Split file names into root, folder, filename and extension");
        ui.checkbox(&mut s.export_drive_size, "Export drive size information");
        ui.checkbox(&mut s.export_percent, "Export percentage of parent value");
        ui.checkbox(&mut s.export_file_ids, if cfg!(windows) { "Export MFT record number" } else { "Export file ID (inode)" });
        ui.horizontal(|ui| {
            ui.label("Max folder depth (0 = no limit)");
            ui.add(egui::DragValue::new(&mut s.export_max_depth).range(0..=1000));
        });
        section(ui, &pal, "File");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut st.path).desired_width(ui.available_width() - 90.0));
            if chrome::ghost_button(ui, &pal, "Browse…").clicked() {
                let p = PathBuf::from(&st.path);
                let mut d = rfd::FileDialog::new()
                    .add_filter("Comma Separated Values (.csv)", &["csv"])
                    .add_filter("Tab Separated Values (.tsv)", &["tsv"]);
                if let Some(dir) = p.parent() {
                    d = d.set_directory(dir);
                }
                if let Some(name) = p.file_name() {
                    d = d.set_file_name(name.to_string_lossy());
                }
                if let Some(np) = d.save_file() {
                    st.path = np.display().to_string();
                }
            }
        });
        if !app.s.export_files && !app.s.export_folders {
            ui.label(RichText::new("Either one or both of \"Export Files\" and \"Export Folders\" must be selected for the export to be valid.").color(pal.danger).small());
        }
        let (ok, cancel) = buttons(ui, &pal, "Export", Some("Cancel"));
        go = ok && (app.s.export_files || app.s.export_folders);
        if cancel {
            keep = false;
        }
    });
    if close {
        keep = false;
    }
    if go {
        let Some(t) = &app.tree else { return };
        let path = PathBuf::from(&st.path);
        if let Some(d) = path.parent() {
            app.s.export_dir = d.display().to_string();
        }
        let o = ExportOptions {
            dates: app.s.export_dates,
            folder_sizes: app.s.export_folder_sizes,
            split_names: app.s.export_split_names,
            drive_size: app.s.export_drive_size,
            percent_of_parent: app.s.export_percent,
            max_depth: app.s.export_max_depth,
            file_ids: app.s.export_file_ids,
            files: app.s.export_files,
            folders: app.s.export_folders,
            tsv: path.extension().map(|e| e.eq_ignore_ascii_case("tsv")).unwrap_or(false),
            ..Default::default()
        };
        let (ids, recursive): (Vec<NodeId>, bool) = if st.from_files {
            if st.selected_only {
                let mut v: Vec<NodeId> = app.fv.sel.iter().copied().collect();
                v.sort_by_key(|&i| std::cmp::Reverse(t.nodes[i as usize].size));
                (v, false)
            } else {
                (app.fv.results.clone(), false)
            }
        } else if st.selected_only {
            (app.sel.clone(), true)
        } else {
            (t.roots.clone(), true)
        };
        match crate::csvio::export(t, &ids, recursive, &o, &path, None) {
            Ok(n) => app.status = format!("Export complete. {} rows written to {}", format::thousands(n as u64), path.display()),
            Err(e) => app.error("Couldn't Export", &format!("Couldn't write the CSV file: {e}")),
        }
    } else if keep {
        app.dlg.export = Some(st);
    }
}

fn treemap_export(app: &mut App, ctx: &egui::Context) {
    let pal = app.pal;
    let mut keep = true;
    let mut go = false;
    let ((), close) = modal(ctx, &pal, "tmexport", 440.0, "Save Treemap Image", |ui| {
        ui.horizontal(|ui| {
            ui.label("Width");
            ui.add(egui::DragValue::new(&mut app.s.treemap_export_w).range(16..=16384));
            ui.label("Height");
            ui.add(egui::DragValue::new(&mut app.s.treemap_export_h).range(16..=16384));
            ui.menu_button("Presets", |ui| {
                for (w, h) in [(1024, 768), (1280, 720), (1680, 1050), (1920, 1080), (2560, 1440), (3840, 2160), (7680, 4320)] {
                    if ui.button(format!("{w}×{h}")).clicked() {
                        app.s.treemap_export_w = w;
                        app.s.treemap_export_h = h;
                        ui.close();
                    }
                }
            });
        });
        ui.checkbox(&mut app.s.treemap_export_gray, "Shades of Gray (single color)");
        ui.checkbox(&mut app.s.treemap_export_free, "Show Free Space");
        let (ok, cancel) = buttons(ui, &pal, "Save…", Some("Cancel"));
        go = ok;
        if cancel {
            keep = false;
        }
    });
    if close {
        keep = false;
    }
    if go {
        keep = false;
        let name = format!("WizTree_{}.png", chrono::Local::now().format("%Y%m%d%H%M%S"));
        if let Some(p) = rfd::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name(name).save_file() {
            let (w, h) = (app.s.treemap_export_w, app.s.treemap_export_h);
            match super::treemap_view::render_image(app, w, h, app.s.treemap_export_gray, app.s.treemap_export_free) {
                Some(rgba) => {
                    if let Err(e) = write_png(&p, w, h, &rgba) {
                        app.error("Couldn't Export", &format!("Couldn't write the PNG file: {e}"));
                    } else {
                        app.status = format!("Saved {}", p.display());
                    }
                }
                None => app.error("Couldn't Export", "Couldn't render the treemap image."),
            }
        }
    }
    app.dlg.treemap_export = keep;
}

pub fn write_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    let f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(f, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(std::io::Error::other)?;
    wr.write_image_data(rgba).map_err(std::io::Error::other)?;
    Ok(())
}

// ------------------------------------------------------------------ select multiple

fn multi(app: &mut App, ctx: &egui::Context) {
    let pal = app.pal;
    let mut items = app.dlg.multi.take().unwrap();
    let mut keep = true;
    let mut go = false;
    let ((), close) = modal(ctx, &pal, "multi", 520.0, "Select Multiple", |ui| {
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            let mut remove = None;
            for (i, it) in items.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut it.2, "");
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                    if it.3 {
                        super::icons::folder(ui.painter(), r.center(), 16.0, pal.accent);
                    } else {
                        super::icons::drive(ui.painter(), r.center(), 16.0, pal.text2);
                    }
                    ui.label(&it.1);
                    if it.3 {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Remove").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                });
            }
            if let Some(i) = remove {
                items.remove(i);
            }
        });
        ui.horizontal(|ui| {
            if chrome::ghost_button(ui, &pal, "Add Folder…").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    let l = p.display().to_string();
                    items.push((p, l, true, true));
                }
            }
            if chrome::ghost_button(ui, &pal, "Select All").clicked() {
                items.iter_mut().for_each(|i| i.2 = true);
            }
            if chrome::ghost_button(ui, &pal, "Select None").clicked() {
                items.iter_mut().for_each(|i| i.2 = false);
            }
        });
        let (ok, cancel) = buttons(ui, &pal, "OK", Some("Cancel"));
        go = ok;
        if cancel {
            keep = false;
        }
    });
    if close {
        keep = false;
    }
    if go {
        let chosen: Vec<PathBuf> = items.iter().filter(|i| i.2).map(|i| i.0.clone()).collect();
        // Nested selections are not allowed.
        for a in &chosen {
            for b in &chosen {
                if a != b && b.starts_with(a) {
                    app.msg = Some(super::Msg {
                        title: "Cannot select these items".into(),
                        body: format!("\"{}\"\nis included within\n\"{}\"\n\nThey cannot both be selected.", b.display(), a.display()),
                        error: true,
                    });
                    app.dlg.multi = Some(items);
                    return;
                }
            }
        }
        app.s.custom_folders = items.iter().filter(|i| i.3).map(|i| i.0.display().to_string()).collect();
        app.s.multi_checked = chosen.iter().map(|p| p.display().to_string()).collect();
        if !chosen.is_empty() {
            app.tab = Tab::Tree;
            app.set_source(if chosen.len() == 1 { Source::Folder(chosen[0].clone()) } else { Source::Multi(chosen) });
        }
    } else if keep {
        app.dlg.multi = Some(items);
    }
}

fn language(app: &mut App, ctx: &egui::Context) {
    let pal = app.pal;
    let langs = crate::i18n::available();
    let mut done = false;
    let ((), close) = modal(ctx, &pal, "language", 380.0, "Language", |ui| {
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            if ui.selectable_label(app.s.language.is_empty(), "English (built-in)").clicked() {
                app.s.language.clear();
                crate::i18n::load("");
            }
            for (code, name) in &langs {
                if ui.selectable_label(&app.s.language == code, format!("{name}  ({code})")).clicked() {
                    app.s.language = code.clone();
                    crate::i18n::load(code);
                }
            }
            if langs.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "No translations found. Place gettext .po files at\n{}/<lang>/LC_MESSAGES/default.po",
                        crate::i18n::locale_dir().display()
                    ))
                    .small()
                    .color(pal.text3),
                );
            }
        });
        done = buttons(ui, &pal, "Done", None).0;
    });
    if close || done {
        app.dlg.language = false;
    }
}

// ------------------------------------------------------------------ NTFS fast scan

fn ntfs_access(app: &mut App, ctx: &egui::Context) {
    let pal = app.pal;
    let mut d = app.dlg.ntfs.take().unwrap();
    let (mut fast, mut normal) = (false, false);
    let ((), close) = modal(ctx, &pal, "ntfs_access", 480.0, "Fast NTFS Scan", |ui| {
        ui.label(
            RichText::new(format!(
                "{} is an NTFS drive. WizTree can read its Master File Table directly — the same technique it uses on Windows — \
                 which is typically tens of times faster than walking every folder, especially on hard disks.",
                d.dev.display()
            ))
            .color(pal.text2),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new("Reading the raw device needs administrator rights. Only the file table is read, by a small helper; nothing is written.")
                .small()
                .color(pal.text3),
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Password").color(pal.text2));
            let r = ui.add(egui::TextEdit::singleline(&mut d.password).password(true).desired_width(f32::INFINITY).margin(Margin::symmetric(8, 5)));
            if !r.has_focus() && d.password.is_empty() {
                r.request_focus();
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                fast = true;
            }
        });
        if let Some(e) = &d.error {
            ui.add_space(4.0);
            ui.label(RichText::new(e).small().color(pal.danger));
        }
        ui.add_space(6.0);
        ui.checkbox(&mut d.remember, "Don't ask again: let my user read NTFS drives (installs a udev rule)");
        let (ok, cancel) = buttons(ui, &pal, "Scan Fast", Some("Scan Normally"));
        fast |= ok;
        normal |= cancel;
    });
    if fast && !d.password.is_empty() {
        let pw = std::mem::take(&mut d.password);
        if d.remember {
            super::install_ntfs_access(pw.clone());
        }
        app.elevation = crate::scan::Elevation::Sudo(pw);
        app.start_scan();
    } else if normal || close {
        app.ntfs_declined = true;
        app.start_scan();
    } else {
        app.dlg.ntfs = Some(d);
    }
}
