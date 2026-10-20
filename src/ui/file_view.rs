//! File View tab: instant search over every file, with duplicate detection.

use super::grid::{Col, Grid};
use super::{App, Cmd, chrome, icons, tree_view};
use crate::config::{DupMode, Settings};
use crate::filter::{DupInfo, Matcher};
use crate::format;
use crate::model::{NodeId, Tree, NONE};
use crate::theme;
use eframe::egui::{self, Color32, CornerRadius, Id, Pos2, RichText, Stroke, Ui, Vec2};
use rayon::prelude::*;
use std::collections::HashSet;
use std::time::Instant;

pub const FILE_COLS: [&str; 9] = ["File Name", "Path", "Percent of Drive", "Size", "Allocated", "Modified", "Attributes", "Dup Count", "Dup Size"];

pub struct Dups {
    pub mode: DupMode,
    pub rev: u64,
    /// Number of copies per node (0 = not a duplicate).
    pub count: Vec<u32>,
}

impl DupInfo for Dups {
    fn dup(&self, id: NodeId) -> (u32, u64) {
        (self.count.get(id as usize).copied().unwrap_or(0), 0)
    }
}

struct DupView<'a>(&'a Dups, &'a Tree);
impl DupInfo for DupView<'_> {
    fn dup(&self, id: NodeId) -> (u32, u64) {
        let c = self.0.count.get(id as usize).copied().unwrap_or(0);
        (c, c as u64 * self.1.nodes[id as usize].size)
    }
}

pub struct FileViewState {
    pub query: String,
    pub results: Vec<NodeId>,
    pub total: usize,
    pub sort: (usize, bool),
    pub cols: Vec<Col>,
    pub sel: HashSet<NodeId>,
    pub anchor: Option<usize>,
    pub dirty: bool,
    pub last_edit: Instant,
    pub dups: Option<Dups>,
    pub search_ms: f64,
    key: String,
}

impl FileViewState {
    pub fn new(s: &Settings) -> Self {
        let w = [260.0, 420.0, 140.0, 92.0, 92.0, 140.0, 76.0, 80.0, 92.0];
        FileViewState {
            query: String::new(),
            results: Vec::new(),
            total: 0,
            sort: (3, true),
            cols: FILE_COLS
                .iter()
                .enumerate()
                .map(|(i, t)| Col::new(*t, s.file_columns.get(i).copied().unwrap_or(w[i]), matches!(i, 3 | 4 | 7 | 8)))
                .collect(),
            sel: HashSet::new(),
            anchor: None,
            dirty: true,
            last_edit: Instant::now(),
            dups: None,
            search_ms: 0.0,
            key: String::new(),
        }
    }
}

pub fn compute_dups(t: &Tree, mode: DupMode) -> Dups {
    let mut ids: Vec<NodeId> = t.files_by_size.iter().copied().filter(|&i| t.nodes[i as usize].size > 0).collect();
    let with_date = mode == DupMode::NameSizeDate;
    ids.par_sort_unstable_by(|&a, &b| {
        let (na, nb) = (&t.nodes[a as usize], &t.nodes[b as usize]);
        na.size
            .cmp(&nb.size)
            .then_with(|| t.name_lower(a).cmp(t.name_lower(b)))
            .then_with(|| if with_date { na.mtime.cmp(&nb.mtime) } else { std::cmp::Ordering::Equal })
    });
    let mut count = vec![0u32; t.nodes.len()];
    let same = |a: NodeId, b: NodeId| {
        let (na, nb) = (&t.nodes[a as usize], &t.nodes[b as usize]);
        na.size == nb.size && t.name_lower(a) == t.name_lower(b) && (!with_date || na.mtime == nb.mtime)
    };
    let mut i = 0;
    while i < ids.len() {
        let mut j = i + 1;
        while j < ids.len() && same(ids[i], ids[j]) {
            j += 1;
        }
        if j - i > 1 {
            for &id in &ids[i..j] {
                count[id as usize] = (j - i) as u32;
            }
        }
        i = j;
    }
    Dups { mode, rev: t.revision, count }
}

fn run_search(app: &mut App) {
    let Some(t) = &app.tree else { return };
    let start = Instant::now();
    let m = Matcher::new(&app.fv.query, app.s.match_path);
    // dupcount / dupsize terms need duplicate data even when the column is off.
    let mode = if app.s.dup_mode == DupMode::Off && m.needs_dups { DupMode::NameSize } else { app.s.dup_mode };
    if mode != DupMode::Off {
        let stale = app.fv.dups.as_ref().map(|d| d.mode != mode || d.rev != t.revision).unwrap_or(true);
        if stale {
            app.fv.dups = Some(compute_dups(t, mode));
        }
    } else {
        app.fv.dups = None;
    }
    let need_path = m.needs_path;
    let dups = app.fv.dups.as_ref();
    let dups_only = app.s.dups_only && dups.is_some();
    // Candidates in size order (files, plus folders when requested).
    let cands: Vec<NodeId> = if app.s.show_folders_in_files {
        let mut v = Vec::with_capacity(t.files_by_size.len() + t.dirs_by_size.len());
        let (mut a, mut b) = (0, 0);
        let (f, d) = (&t.files_by_size, &t.dirs_by_size);
        while a < f.len() || b < d.len() {
            let take_f = b >= d.len() || (a < f.len() && t.nodes[f[a] as usize].size >= t.nodes[d[b] as usize].size);
            if take_f {
                v.push(f[a]);
                a += 1;
            } else {
                v.push(d[b]);
                b += 1;
            }
        }
        v
    } else {
        Vec::new()
    };
    let src: &[NodeId] = if app.s.show_folders_in_files { &cands } else { &t.files_by_size };
    let mut res: Vec<NodeId> = if m.is_empty() && !dups_only {
        src.iter().copied().filter(|&i| !t.nodes[i as usize].is_deleted()).collect()
    } else {
        src.par_iter()
            .copied()
            .map_init(
                || (Vec::new(), Vec::new()),
                |(buf, stack), id| {
                    let n = &t.nodes[id as usize];
                    if n.is_deleted() {
                        return None;
                    }
                    if dups_only && dups.map(|d| d.count[id as usize] < 2).unwrap_or(true) {
                        return None;
                    }
                    let p = if need_path {
                        t.path_lower_into(id, buf, stack);
                        Some(&buf[..])
                    } else {
                        None
                    };
                    let ok = match dups {
                        Some(d) => m.matches_with(t, id, p, &DupView(d, t)),
                        None => m.matches(t, id, p),
                    };
                    ok.then_some(id)
                },
            )
            .flatten()
            .collect()
    };
    let (col, desc) = app.fv.sort;
    if !(col == 3 && desc) {
        sort_results(t, &mut res, col, desc, dups);
    }
    app.fv.total = res.len();
    if app.s.max_files > 0 && res.len() > app.s.max_files {
        res.truncate(app.s.max_files);
    }
    app.fv.results = res;
    app.fv.search_ms = start.elapsed().as_secs_f64() * 1000.0;
}

fn sort_results(t: &Tree, v: &mut [NodeId], col: usize, desc: bool, dups: Option<&Dups>) {
    let dc = |id: NodeId| dups.map(|d| d.count[id as usize]).unwrap_or(0);
    match col {
        0 => v.par_sort_by(|&a, &b| t.name_lower(a).cmp(t.name_lower(b))),
        1 => v.par_sort_by_cached_key(|&a| t.path(t.parent(a)).to_lowercase()),
        4 => v.par_sort_by_key(|&a| t.nodes[a as usize].alloc),
        5 => v.par_sort_by_key(|&a| t.nodes[a as usize].mtime),
        6 => v.par_sort_by_key(|&a| format::win_attrs(t.nodes[a as usize].flags)),
        7 => v.par_sort_by_key(|&a| (dc(a), t.nodes[a as usize].size)),
        8 => v.par_sort_by_key(|&a| dc(a) as u64 * t.nodes[a as usize].size),
        _ => v.par_sort_by_key(|&a| t.nodes[a as usize].size),
    }
    if desc {
        v.reverse();
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let key = format!(
        "{}|{}|{}|{}|{:?}|{}|{:?}|{}",
        app.fv.query,
        app.s.match_path,
        app.s.max_files,
        app.s.show_folders_in_files,
        app.s.dup_mode,
        app.s.dups_only,
        app.fv.sort,
        app.tree.as_ref().map(|t| t.revision).unwrap_or(0)
    );
    if key != app.fv.key {
        app.fv.dirty = true;
    }
    if app.fv.dirty {
        // Debounce while typing in very large trees.
        let big = app.tree.as_ref().map(|t| t.files_by_size.len() > 2_000_000).unwrap_or(false);
        if !big || app.fv.last_edit.elapsed().as_millis() > 120 {
            run_search(app);
            app.fv.key = key;
            app.fv.dirty = false;
            if let Some(t) = &app.tree {
                app.fv.sel.retain(|&i| !t.nodes[i as usize].is_deleted());
            }
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(130));
        }
    }

    let full = ui.max_rect();
    ui.painter().rect(full, CornerRadius::same(12), pal.card, Stroke::new(1.0, pal.separator), egui::StrokeKind::Inside);
    let bar = egui::Rect::from_min_max(full.min + Vec2::new(14.0, 8.0), Pos2::new(full.max.x - 14.0, full.min.y + 46.0));
    {
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
        options_bar(app, &mut c);
    }
    let list = egui::Rect::from_min_max(Pos2::new(full.min.x + 2.0, bar.max.y + 4.0), full.max - Vec2::new(2.0, 4.0));
    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(list));
    grid(app, &mut c);
}

fn options_bar(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    ui.spacing_mut().item_spacing.x = 14.0;
    let mut mp = app.s.match_path;
    if chrome::segmented(ui, &pal, &mut mp, &[(false, "Name"), (true, "Full Path")]) {
        app.s.match_path = mp;
    }
    ui.checkbox(&mut app.s.show_folders_in_files, "Folders");
    ui.label(RichText::new("Max").color(pal.text2));
    let label = if app.s.max_files == 0 { "All".to_string() } else { format::thousands(app.s.max_files as u64) };
    egui::ComboBox::from_id_salt("maxfiles").selected_text(label).width(84.0).show_ui(ui, |ui| {
        for v in [100usize, 1000, 10_000, 100_000, 1_000_000, 0] {
            let l = if v == 0 { "All".to_string() } else { format::thousands(v as u64) };
            ui.selectable_value(&mut app.s.max_files, v, l);
        }
    });
    ui.label(RichText::new("Duplicates").color(pal.text2));
    let dl = match app.s.dup_mode {
        DupMode::Off => "Off",
        DupMode::NameSize => "Name + Size",
        DupMode::NameSizeDate => "Name + Size + Date",
    };
    egui::ComboBox::from_id_salt("dupmode").selected_text(dl).width(150.0).show_ui(ui, |ui| {
        ui.selectable_value(&mut app.s.dup_mode, DupMode::Off, "Don't locate duplicates");
        ui.selectable_value(&mut app.s.dup_mode, DupMode::NameSize, "Locate by Name, Size");
        ui.selectable_value(&mut app.s.dup_mode, DupMode::NameSizeDate, "Locate by Name, Size, Date");
    });
    ui.add_enabled(app.s.dup_mode != DupMode::Off, egui::Checkbox::new(&mut app.s.dups_only, "Duplicates only"));
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if !app.fv.query.is_empty() && chrome::ghost_button(ui, &pal, "Clear").clicked() {
            app.fv.query.clear();
            app.fv.dirty = true;
        }
        let shown = app.fv.results.len();
        let txt = if shown < app.fv.total {
            format!("{} of {} · {:.0} ms", format::thousands(shown as u64), format::thousands(app.fv.total as u64), app.fv.search_ms)
        } else {
            format!("{} files · {:.0} ms", format::thousands(app.fv.total as u64), app.fv.search_ms)
        };
        ui.label(RichText::new(txt).color(pal.text3).font(theme::regular(11.5)));
    });
}

fn grid(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let t = app.tree.as_ref().unwrap();
    let fmt = app.fmt();
    let colors = app.ext_colors();
    let gain = app.scheme_gain();
    let drive = if t.volume.total > 0 { t.volume.total } else { t.total_size().max(1) };
    let res = &app.fv.results;
    let sel = &app.fv.sel;
    let dups = app.fv.dups.as_ref();
    let show_dups = dups.is_some();
    let ncols = if show_dups { 9 } else { 7 };
    let out = Grid {
        id: Id::unique("file_grid"),
        cols: &mut app.fv.cols[..ncols],
        sort: Some(app.fv.sort),
        rows: res.len(),
        row_h: 24.0,
        scroll_to: None,
        alt_rows: app.s.alt_row_colors,
        hlines: app.s.horizontal_lines,
        flex: Some(1),
    }
    .show(
        ui,
        &pal,
        |i| sel.contains(&res[i]),
        |cc, i, col| {
            let id = res[i];
            let n = &t.nodes[id as usize];
            match col {
                0 => {
                    let cy = cc.rect.center().y;
                    let x = cc.rect.left();
                    if n.is_dir() {
                        icons::folder(&cc.painter, Pos2::new(x + 8.0, cy), 16.0, cc.pal.accent);
                    } else {
                        let rgb = colors.get(n.ext as usize).copied().unwrap_or([128, 128, 128]);
                        let g = (gain * 1.3).min(1.0);
                        let c = Color32::from_rgb((rgb[0] as f32 * g) as u8, (rgb[1] as f32 * g) as u8, (rgb[2] as f32 * g) as u8);
                        icons::file(&cc.painter, Pos2::new(x + 8.0, cy), 15.0, theme::lerp(cc.pal.text3, cc.pal.card, 0.45), c);
                    }
                    cc.text_at(x + 22.0, t.name(id), theme::regular(12.5), cc.pal.text);
                }
                1 => {
                    let p = t.parent(id);
                    let s = if p == NONE || t.is_root(id) { String::new() } else { t.path(p) };
                    cc.text(&s, cc.pal.text2);
                }
                2 => {
                    let f = n.size as f64 / drive as f64;
                    cc.bar(f as f32, cc.pal.accent, &format!("{:.2} %", f * 100.0));
                }
                3 => cc.text_font(&fmt.size(n.size), theme::medium(12.5), cc.pal.text),
                4 => cc.text(&fmt.size(n.alloc), cc.pal.text2),
                5 => cc.text(&format::date(n.mtime), cc.pal.text2),
                6 => cc.text(&format::attrs(n.flags), cc.pal.text3),
                7 => {
                    let c = dups.map(|d| d.count[id as usize]).unwrap_or(0);
                    cc.text(if c > 1 { format::thousands(c as u64) } else { String::new() }.as_str(), cc.pal.text2);
                }
                8 => {
                    let c = dups.map(|d| d.count[id as usize]).unwrap_or(0);
                    cc.text(if c > 1 { fmt.size(c as u64 * n.size) } else { String::new() }.as_str(), cc.pal.text2);
                }
                _ => {}
            }
        },
    );
    if out.widths_changed {
        app.s.file_columns = app.fv.cols.iter().map(|c| c.width).collect();
    }
    if let Some(c) = out.header_clicked {
        app.fv.sort = if app.fv.sort.0 == c { (c, !app.fv.sort.1) } else { (c, !matches!(c, 0 | 1)) };
        app.fv.dirty = true;
    }
    let ctx = ui.ctx().clone();
    if let Some((row, m)) = out.clicked {
        let id = app.fv.results[row];
        if m.shift {
            if let Some(a) = app.fv.anchor {
                let (lo, hi) = if a <= row { (a, row) } else { (row, a) };
                app.fv.sel = app.fv.results[lo..=hi].iter().copied().collect();
            }
        } else if m.command {
            if !app.fv.sel.remove(&id) {
                app.fv.sel.insert(id);
            }
            app.fv.anchor = Some(row);
        } else {
            app.fv.sel.clear();
            app.fv.sel.insert(id);
            app.fv.anchor = Some(row);
        }
    }
    if let Some(row) = out.double_clicked {
        let id = app.fv.results[row];
        if app.s.double_click_open {
            app.fv.sel = [id].into_iter().collect();
            app.run(Cmd::Open, &ctx);
        }
    }
    if let Some(row) = out.hovered {
        app.hover_path = app.tree.as_ref().unwrap().path(app.fv.results[row]);
    }
    for (row, resp) in &out.rows {
        let id = app.fv.results[*row];
        if resp.secondary_clicked() && !app.fv.sel.contains(&id) {
            app.fv.sel.clear();
            app.fv.sel.insert(id);
        }
        let mut cmd = None;
        let mut show_in_tree = false;
        resp.context_menu(|ui| {
            if ui.button("Show in Tree View").clicked() {
                show_in_tree = true;
                ui.close();
            }
            ui.separator();
            cmd = tree_view::node_menu(ui, app);
        });
        if show_in_tree {
            app.tab = super::Tab::Tree;
            app.select_node(id);
        }
        if let Some(c) = cmd {
            app.run(c, &ctx);
        }
    }
    // Ctrl+A selects all results.
    if !ui.ctx().egui_wants_keyboard_input() && ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::A))) {
        app.fv.sel = app.fv.results.iter().copied().collect();
    }
}
