//! Tree View tab: folder tree (with file grouping), file types panel and
//! the treemap underneath.

use super::grid::{CellCtx, Col, Grid};
use super::{App, Cmd, chrome, icons, treemap_view};
use crate::config::Settings;
use crate::filter::Matcher;
use crate::format;
use crate::model::{Node, NodeId, Tree, flags, NONE};
use crate::theme;
use eframe::egui::{self, Color32, CornerRadius, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use rayon::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct Row {
    /// The node, or for group rows the folder whose files are grouped.
    pub node: NodeId,
    pub group: bool,
    pub depth: u16,
}

pub const TREE_COLS: [&str; 9] = ["Folder", "% of Parent", "Size", "Allocated", "Items", "Files", "Folders", "Modified", "Attributes"];

pub fn default_cols(s: &Settings) -> Vec<Col> {
    let w = [340.0, 130.0, 88.0, 88.0, 76.0, 76.0, 70.0, 140.0, 76.0];
    TREE_COLS
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut c = Col::new(*t, s.tree_columns.get(i).copied().unwrap_or(w[i]), i >= 2 && i <= 6);
            if i == 0 {
                c.min = 220.0;
            }
            c
        })
        .collect()
}

pub fn default_ext_cols() -> Vec<Col> {
    vec![
        Col::new("", 24.0, false),
        Col::new("Extension", 78.0, false),
        Col { min: 100.0, ..Col::new("File Type", 130.0, false) },
        Col::new("Percent", 104.0, false),
        Col::new("Size", 80.0, true),
        Col::new("Allocated", 80.0, true),
        Col::new("Files", 64.0, true),
    ]
}

pub fn has_group(t: &Tree, dir: NodeId, min: u32) -> bool {
    if min == 0 {
        return false;
    }
    let files = t.kids(dir).iter().filter(|&&k| !t.nodes[k as usize].is_dir()).count();
    files as u32 >= min
}

fn group_stats(t: &Tree, dir: NodeId) -> (u64, u64, u32, i64) {
    let (mut s, mut a, mut n, mut m) = (0u64, 0u64, 0u32, 0i64);
    for &k in t.kids(dir) {
        let kn = &t.nodes[k as usize];
        if !kn.is_dir() {
            s += kn.size;
            a += kn.alloc;
            n += 1;
            m = m.max(kn.mtime);
        }
    }
    (s, a, n, m)
}

/// Sort key for a row in the given column (higher = first when descending).
fn sort_items(t: &Tree, items: &mut [(NodeId, bool)], col: usize, desc: bool) {
    let key = |&(id, group): &(NodeId, bool)| -> (u64, i64) {
        if group {
            let (s, a, n, m) = group_stats(t, id);
            return match col {
                3 => (a, 0),
                4 | 5 => (n as u64, 0),
                6 => (0, 0),
                7 => (0, m),
                _ => (s, 0),
            };
        }
        let n = &t.nodes[id as usize];
        match col {
            3 => (n.alloc, 0),
            4 => ((n.files + n.folders) as u64 + if n.is_dir() { 0 } else { 1 }, 0),
            5 => (if n.is_dir() { n.files as u64 } else { 1 }, 0),
            6 => (n.folders as u64, 0),
            7 => (0, n.mtime),
            8 => (format::win_attrs(n.flags) as u64, 0),
            _ => (n.size, 0),
        }
    };
    if col == 0 {
        items.sort_by(|a, b| {
            let na = if a.1 { "" } else { t.name(a.0) };
            let nb = if b.1 { "" } else { t.name(b.0) };
            let o = (a.1.cmp(&b.1)).reverse().then_with(|| na.to_lowercase().cmp(&nb.to_lowercase()));
            if desc { o.reverse() } else { o }
        });
    } else {
        items.sort_by(|a, b| {
            let o = key(a).cmp(&key(b));
            if desc { o.reverse() } else { o }
        });
    }
}

pub fn build_rows(app: &App) -> Vec<Row> {
    let Some(t) = &app.tree else { return Vec::new() };
    let mut rows = Vec::new();
    let (col, desc) = app.tree_sort;
    let min = app.s.file_group_min;
    let mut stack: Vec<(NodeId, bool, u16)> = Vec::new();
    let mut roots: Vec<(NodeId, bool)> = t.roots.iter().map(|&r| (r, false)).collect();
    sort_items(t, &mut roots, col, desc);
    for &(r, g) in roots.iter().rev() {
        stack.push((r, g, 0));
    }
    while let Some((id, group, depth)) = stack.pop() {
        rows.push(Row { node: id, group, depth });
        if group {
            if app.expanded_groups.contains(&id) {
                let mut files: Vec<(NodeId, bool)> = t.kids(id).iter().filter(|&&k| !t.nodes[k as usize].is_dir()).map(|&k| (k, false)).collect();
                if !(col == 2 && desc) {
                    sort_items(t, &mut files, col, desc);
                }
                for &(f, _) in files.iter().rev() {
                    stack.push((f, false, depth + 1));
                }
            }
            continue;
        }
        if !t.nodes[id as usize].is_dir() || !app.expanded.contains(&id) {
            continue;
        }
        let grouped = has_group(t, id, min);
        let mut items: Vec<(NodeId, bool)> = Vec::with_capacity(t.kids(id).len());
        for &k in t.kids(id) {
            if grouped && !t.nodes[k as usize].is_dir() {
                continue;
            }
            items.push((k, false));
        }
        if grouped {
            items.push((id, true));
        }
        if !(col == 2 && desc && !grouped) {
            sort_items(t, &mut items, col, desc);
        }
        for &(k, g) in items.iter().rev() {
            stack.push((k, g, depth + 1));
        }
    }
    rows
}

pub fn is_expandable(t: &Tree, row: &Row) -> bool {
    if row.group {
        return true;
    }
    let n = &t.nodes[row.node as usize];
    n.is_dir() && n.child_count > 0
}

// ------------------------------------------------------------------ layout

/// Height of the treemap pane. Hidden in very short windows: the fixed
/// minimums would otherwise make `clamp` panic (min > max).
fn treemap_height(full_h: f32, frac: f32) -> f32 {
    const MIN_TM: f32 = 120.0;
    const MIN_TOP: f32 = 140.0;
    if !(full_h >= MIN_TM + MIN_TOP) || !frac.is_finite() {
        return 0.0;
    }
    let h = full_h * frac;
    if !h.is_finite() {
        return 0.0;
    }
    h.clamp(MIN_TM, full_h - MIN_TOP)
}

/// Width of the file-type panel, or 0 when the window is too narrow for it.
fn ext_panel_width(full_w: f32, want: f32) -> f32 {
    const MIN_EXT: f32 = 260.0;
    let max = full_w * 0.6;
    if max < MIN_EXT || !max.is_finite() || !want.is_finite() {
        return 0.0;
    }
    want.clamp(MIN_EXT, max)
}

pub fn show(app: &mut App, ui: &mut Ui) {
    if app.rows_dirty {
        app.rows = build_rows(app);
        app.rows_dirty = false;
    }
    let full = ui.max_rect();
    let gap = 10.0;
    let tm_h = if app.s.show_treemap { treemap_height(full.height(), app.s.treemap_height_frac) } else { 0.0 };
    let show_tm = tm_h > 0.0;
    let top = Rect::from_min_max(full.min, Pos2::new(full.max.x, full.max.y - if show_tm { tm_h + gap } else { 0.0 }));
    // Horizontal splitter between top area and treemap.
    if show_tm {
        let split = Rect::from_min_max(Pos2::new(full.left(), top.bottom()), Pos2::new(full.right(), top.bottom() + gap));
        let r = ui.interact(split, Id::unique("tm_split"), Sense::drag());
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            ui.painter().line_segment(
                [Pos2::new(split.center().x - 20.0, split.center().y), Pos2::new(split.center().x + 20.0, split.center().y)],
                Stroke::new(3.0, app.pal.border),
            );
        }
        if r.dragged() {
            let ny = (full.bottom() - r.interact_pointer_pos().map(|p| p.y).unwrap_or(top.bottom())) / full.height();
            app.s.treemap_height_frac = ny.clamp(0.15, 0.85);
        }
    }
    let ext_w = if app.s.show_file_types { ext_panel_width(top.width(), app.s.ext_panel_width) } else { 0.0 };
    let tree_r = Rect::from_min_max(top.min, Pos2::new(top.max.x - if ext_w > 0.0 { ext_w + gap } else { 0.0 }, top.max.y));
    if ext_w > 0.0 {
        let split = Rect::from_min_max(Pos2::new(tree_r.right(), top.top()), Pos2::new(tree_r.right() + gap, top.bottom()));
        let r = ui.interact(split, Id::unique("ext_split"), Sense::drag());
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if r.dragged() {
            app.s.ext_panel_width = (app.s.ext_panel_width - r.drag_delta().x).clamp(260.0, 900.0);
        }
        let ext_r = Rect::from_min_max(Pos2::new(tree_r.right() + gap, top.top()), top.max);
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(ext_r));
        ext_panel(app, &mut c);
    }
    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(tree_r));
    tree_grid(app, &mut c);
    if show_tm {
        let tm_r = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - tm_h), full.max);
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(tm_r));
        treemap_view::show(app, &mut c);
    }
}

fn card_bg(ui: &mut Ui, pal: &theme::Palette) -> Rect {
    let r = ui.max_rect();
    ui.painter().rect(r, CornerRadius::same(12), pal.card, Stroke::new(1.0, pal.separator), egui::StrokeKind::Inside);
    r
}

fn tree_grid(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let r = card_bg(ui, &pal);
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(Vec2::new(2.0, 4.0))));
    let t = app.tree.as_ref().unwrap();
    let scroll_to = app.scroll_tree_to.take().and_then(|id| app.rows.iter().position(|r| !r.group && r.node == id));
    let fmt = app.fmt();
    let ext_colors = app.ext_colors();
    let gain = app.scheme_gain();
    let sel: std::collections::HashSet<NodeId> = app.sel.iter().copied().collect();
    let rows = &app.rows;
    let mut toggle: Option<usize> = None;
    let out = Grid {
        id: Id::unique("tree_grid"),
        cols: &mut app.tree_cols,
        sort: Some(app.tree_sort),
        rows: rows.len(),
        row_h: 24.0,
        scroll_to,
        alt_rows: app.s.alt_row_colors,
        hlines: app.s.horizontal_lines,
        flex: Some(0),
    }
    .show(
        &mut inner,
        &pal,
        |i| !rows[i].group && sel.contains(&rows[i].node),
        |cc: &mut CellCtx, i, col| {
            let row = rows[i];
            paint_tree_cell(cc, t, &row, col, &fmt, &ext_colors, gain, app.expanded.contains(&row.node) || (row.group && app.expanded_groups.contains(&row.node)), &mut toggle, i);
        },
    );
    if out.widths_changed {
        app.s.tree_columns = app.tree_cols.iter().map(|c| c.width).collect();
    }
    if let Some(c) = out.header_clicked {
        app.tree_sort = if app.tree_sort.0 == c { (c, !app.tree_sort.1) } else { (c, c != 0) };
        app.rows_dirty = true;
    }
    // Chevron hit test: the chevron occupies [indent, indent+16] of column 0.
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let mut chevron_click = None;
    if let Some((row, _)) = out.clicked {
        if let (Some(pos), Some((_, resp))) = (pointer, out.rows.iter().find(|(r, _)| *r == row)) {
            let depth = rows[row].depth as f32;
            let x0 = resp.rect.left() + 8.0 + 8.0 + depth * 16.0;
            if pos.x >= x0 - 4.0 && pos.x <= x0 + 16.0 && is_expandable(t, &rows[row]) {
                chevron_click = Some(row);
            }
        }
    }
    let ctx = ui.ctx().clone();
    if let Some(row) = chevron_click.or(toggle) {
        toggle_row(app, row);
    } else if let Some((row, m)) = out.clicked {
        click_select(app, row, m);
    }
    if let Some(row) = out.double_clicked {
        let r = app.rows[row];
        let t = app.tree.as_ref().unwrap();
        if is_expandable(t, &r) {
            toggle_row(app, row);
        } else if app.s.double_click_open && !r.group {
            app.sel = vec![r.node];
            app.run(Cmd::Open, &ctx);
        }
    }
    if let Some(row) = out.hovered {
        let r = app.rows[row];
        if !r.group {
            app.hover_path = app.tree.as_ref().unwrap().path(r.node);
        }
    }
    // Context menus
    for (row, resp) in &out.rows {
        let r = app.rows[*row];
        if resp.secondary_clicked() && !r.group && !app.sel.contains(&r.node) {
            app.sel = vec![r.node];
        }
        let mut cmd = None;
        resp.context_menu(|ui| {
            cmd = node_menu(ui, app);
        });
        if let Some(c) = cmd {
            app.run(c, &ctx);
        }
    }
    // Keyboard navigation
    if out.body_rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default())) || ui.ctx().memory(|m| m.focused().is_none()) {
        keyboard_nav(app, ui);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_tree_cell(
    cc: &mut CellCtx,
    t: &Tree,
    row: &Row,
    col: usize,
    fmt: &format::Fmt,
    ext_colors: &[[u8; 3]],
    gain: f32,
    expanded: bool,
    _toggle: &mut Option<usize>,
    _i: usize,
) {
    let pal = cc.pal;
    let (size, alloc, files, folders, mtime, fl, parent_size) = if row.group {
        let (s, a, n, m) = group_stats(t, row.node);
        (s, a, n as u64, 0u64, m, 0u32, t.nodes[row.node as usize].size)
    } else {
        let n = &t.nodes[row.node as usize];
        let ps = if t.is_root(row.node) || n.parent == NONE { 0 } else { t.nodes[n.parent as usize].size };
        (n.size, n.alloc, if n.is_dir() { n.files as u64 } else { 1 }, n.folders as u64, n.mtime, n.flags, ps)
    };
    let is_dir = !row.group && fl & flags::DIR != 0;
    match col {
        0 => {
            let x = cc.rect.left() + row.depth as f32 * 16.0;
            let cy = cc.rect.center().y;
            if is_expandable(t, row) {
                icons::chevron(&cc.painter, Pos2::new(x + 6.0, cy), 10.0, expanded, pal.text3);
            }
            let ix = x + 24.0;
            if row.group {
                icons::file(&cc.painter, Pos2::new(ix, cy), 15.0, pal.text3, pal.text2);
                icons::file(&cc.painter, Pos2::new(ix + 3.0, cy + 2.0), 15.0, theme::lerp(pal.text3, pal.card, 0.3), pal.text2);
            } else if is_dir {
                let c = if fl & flags::ACCESS_DENIED != 0 { pal.danger } else if fl & flags::MOUNT != 0 { pal.text3 } else { pal.accent };
                icons::folder(&cc.painter, Pos2::new(ix, cy), 16.0, c);
            } else {
                let e = t.nodes[row.node as usize].ext as usize;
                let rgb = ext_colors.get(e).copied().unwrap_or([128, 128, 128]);
                let g = (gain * 1.3).min(1.0);
                let c = Color32::from_rgb((rgb[0] as f32 * g) as u8, (rgb[1] as f32 * g) as u8, (rgb[2] as f32 * g) as u8);
                icons::file(&cc.painter, Pos2::new(ix, cy), 15.0, theme::lerp(pal.text3, pal.card, 0.45), c);
            }
            let name = if row.group {
                let n = group_stats(t, row.node).2;
                format!("[{} {} in {}]", format::thousands(n as u64), if n == 1 { "File" } else { "Files" }, t.name(row.node))
            } else {
                t.name(row.node).to_string()
            };
            let color = if fl & flags::HIDDEN != 0 || row.group { pal.text2 } else { pal.text };
            let font = if is_dir { theme::medium(12.5) } else { theme::regular(12.5) };
            cc.text_at(ix + 14.0, &name, font, color);
        }
        1 => {
            let frac = if parent_size > 0 { size as f64 / parent_size as f64 } else if t.is_root(row.node) { 1.0 } else { 0.0 };
            let c = depth_color(&pal, row.depth);
            cc.bar(frac as f32, c, &format!("{:.1} %", frac * 100.0));
        }
        2 => cc.text_font(&fmt.size(size), if is_dir { theme::medium(12.5) } else { theme::regular(12.5) }, pal.text),
        3 => cc.text(&fmt.size(alloc), pal.text2),
        4 => cc.text(&format::thousands(files + folders), pal.text2),
        5 => cc.text(&format::thousands(files), pal.text2),
        6 => cc.text(if is_dir { format::thousands(folders) } else { String::new() }.as_str(), pal.text2),
        7 => cc.text(&format::date(mtime), pal.text2),
        8 => cc.text(&format::attrs(fl), pal.text3),
        _ => {}
    }
}

pub fn depth_color(pal: &theme::Palette, depth: u16) -> Color32 {
    let t = (depth as f32 * 0.12).min(0.6);
    theme::lerp(pal.accent, pal.text3, t)
}

fn toggle_row(app: &mut App, row: usize) {
    let r = app.rows[row];
    if r.group {
        if !app.expanded_groups.remove(&r.node) {
            app.expanded_groups.insert(r.node);
        }
    } else if !app.expanded.remove(&r.node) {
        app.expanded.insert(r.node);
    }
    app.rows_dirty = true;
}

fn click_select(app: &mut App, row: usize, m: egui::Modifiers) {
    let r = app.rows[row];
    if r.group {
        return;
    }
    if m.shift {
        if let Some(a) = app.anchor {
            let (lo, hi) = if a <= row { (a, row) } else { (row, a) };
            app.sel = app.rows[lo..=hi.min(app.rows.len() - 1)].iter().filter(|r| !r.group).map(|r| r.node).collect();
            return;
        }
    }
    if m.command {
        if let Some(p) = app.sel.iter().position(|&x| x == r.node) {
            app.sel.remove(p);
        } else {
            app.sel.push(r.node);
        }
    } else {
        app.sel = vec![r.node];
        app.reselect_stack.clear();
    }
    app.anchor = Some(row);
}

fn keyboard_nav(app: &mut App, ui: &mut Ui) {
    if ui.ctx().egui_wants_keyboard_input() || app.rows.is_empty() {
        return;
    }
    let cur = app.sel.first().and_then(|&id| app.rows.iter().position(|r| !r.group && r.node == id));
    let (up, down, left, right, home, end, pgup, pgdn) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
            i.key_pressed(egui::Key::Home),
            i.key_pressed(egui::Key::End),
            i.key_pressed(egui::Key::PageUp),
            i.key_pressed(egui::Key::PageDown),
        )
    });
    let n = app.rows.len();
    let mut target: Option<usize> = None;
    let c = cur.unwrap_or(0);
    if up {
        target = Some(c.saturating_sub(1));
    } else if down {
        target = Some((c + 1).min(n - 1));
    } else if home {
        target = Some(0);
    } else if end {
        target = Some(n - 1);
    } else if pgup {
        target = Some(c.saturating_sub(20));
    } else if pgdn {
        target = Some((c + 20).min(n - 1));
    } else if right {
        if let Some(c) = cur {
            let r = app.rows[c];
            let t = app.tree.as_ref().unwrap();
            if is_expandable(t, &r) && !app.expanded.contains(&r.node) {
                toggle_row(app, c);
            } else {
                target = Some((c + 1).min(n - 1));
            }
        }
    } else if left {
        if let Some(c) = cur {
            let r = app.rows[c];
            if app.expanded.contains(&r.node) && !r.group {
                toggle_row(app, c);
            } else {
                // Jump to parent row.
                let d = r.depth;
                if let Some(p) = (0..c).rev().find(|&i| app.rows[i].depth < d) {
                    target = Some(p);
                }
            }
        }
    }
    if let Some(mut ti) = target {
        // Skip group rows when moving.
        if app.rows[ti].group {
            if down || pgdn || end {
                ti = (ti + 1).min(n - 1);
            } else if ti > 0 {
                ti -= 1;
            }
        }
        if !app.rows[ti].group {
            app.sel = vec![app.rows[ti].node];
            app.anchor = Some(ti);
            app.scroll_tree_to = Some(app.rows[ti].node);
        }
    }
}

/// Context menu for files / folders. Returns the chosen command.
pub fn node_menu(ui: &mut Ui, app: &App) -> Option<Cmd> {
    let mut cmd = None;
    let mut it = |ui: &mut Ui, label: &str, sc: &str, c: Cmd| {
        if ui.add(egui::Button::new(label).shortcut_text(sc)).clicked() {
            cmd = Some(c);
            ui.close();
        }
    };
    it(ui, "Open", "Enter", Cmd::Open);
    it(ui, chrome::reveal_label(), "Ctrl+E", Cmd::Reveal);
    it(ui, chrome::terminal_label(), "Ctrl+P", Cmd::Terminal);
    it(ui, "Copy Path", "Ctrl+Alt+C", Cmd::CopyPath);
    ui.separator();
    it(ui, "Copy", "Ctrl+C", Cmd::Copy);
    it(ui, "Cut", "Ctrl+X", Cmd::Cut);
    if app.s.enable_rename {
        it(ui, "Rename", "F2", Cmd::Rename);
    }
    it(ui, chrome::trash_label(), "Del", Cmd::DeleteToTrash);
    it(ui, "Delete Permanently", "Shift+Del", Cmd::DeletePermanently);
    ui.separator();
    it(ui, "Zoom Treemap", "F10", Cmd::ZoomTreemap);
    it(ui, "Unzoom Treemap", "F11", Cmd::UnzoomTreemap);
    it(ui, "Export to CSV File…", "Ctrl+Alt+E", Cmd::ExportCsv);
    it(ui, "Copy File and Size Info", "Ctrl+Shift+C", Cmd::CopyFileInfo);
    ui.separator();
    it(ui, "Properties", "Alt+Enter", Cmd::Properties);
    cmd
}

// ------------------------------------------------------------------ file types panel

fn ext_panel(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let r = card_bg(ui, &pal);
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(Vec2::new(2.0, 4.0))));
    let t = app.tree.as_ref().unwrap();
    if app.ext_rows.len() != t.ext_stats.len() {
        app.ext_rows = (0..t.ext_stats.len()).collect();
        sort_ext(app);
    }
    let t = app.tree.as_ref().unwrap();
    let total = t.total_size().max(1);
    let fmt = app.fmt();
    let colors = app.ext_colors();
    let gain = app.scheme_gain();
    let rows = &app.ext_rows;
    let sel = app.ext_sel;
    let out = Grid {
        id: Id::unique("ext_grid"),
        cols: &mut app.ext_cols,
        sort: Some(app.ext_sort),
        rows: rows.len(),
        row_h: 24.0,
        scroll_to: None,
        alt_rows: app.s.alt_row_colors,
        hlines: app.s.horizontal_lines,
        flex: Some(2),
    }
    .show(
        &mut inner,
        &pal,
        |i| Some(t.ext_stats[rows[i]].ext) == sel,
        |cc, i, col| {
            let st = &t.ext_stats[rows[i]];
            let name = &t.exts.names[st.ext as usize];
            match col {
                0 => {
                    let rgb = colors.get(st.ext as usize).copied().unwrap_or([128, 128, 128]);
                    let g = (gain * 1.3).min(1.0);
                    let c = Color32::from_rgb((rgb[0] as f32 * g) as u8, (rgb[1] as f32 * g) as u8, (rgb[2] as f32 * g) as u8);
                    let r = Rect::from_center_size(cc.rect.center(), Vec2::splat(12.0));
                    cc.painter.rect_filled(r, CornerRadius::same(3), c);
                }
                1 => cc.text_font(if name.is_empty() { "(none)" } else { name }, theme::medium(12.5), cc.pal.text),
                2 => cc.text(&if name.is_empty() { "(No Extension)".to_string() } else { crate::filetypes::describe(name) }, cc.pal.text2),
                3 => {
                    let f = st.size as f64 / total as f64;
                    cc.bar(f as f32, cc.pal.accent, &format!("{:.1} %", f * 100.0));
                }
                4 => cc.text(&fmt.size(st.size), cc.pal.text),
                5 => cc.text(&fmt.size(st.alloc), cc.pal.text2),
                6 => cc.text(&format::thousands(st.files), cc.pal.text2),
                _ => {}
            }
        },
    );
    if let Some(c) = out.header_clicked {
        if c > 0 {
            app.ext_sort = if app.ext_sort.0 == c { (c, !app.ext_sort.1) } else { (c, c >= 3) };
            sort_ext(app);
        }
    }
    let ctx = ui.ctx().clone();
    if let Some((row, _)) = out.clicked {
        let e = app.tree.as_ref().unwrap().ext_stats[app.ext_rows[row]].ext;
        app.ext_sel = if app.ext_sel == Some(e) { None } else { Some(e) };
        app.tm.dirty = true;
    }
    if let Some(row) = out.double_clicked {
        let e = app.tree.as_ref().unwrap().ext_stats[app.ext_rows[row]].ext;
        app.run(Cmd::SelectExtension(e), &ctx);
    }
    for (row, resp) in &out.rows {
        let e = app.tree.as_ref().unwrap().ext_stats[app.ext_rows[*row]].ext;
        let mut cmd = None;
        resp.context_menu(|ui| {
            if ui.button("Select in File View").clicked() {
                cmd = Some(Cmd::SelectExtension(e));
                ui.close();
            }
            if ui.button("Export File Types to CSV File…").clicked() {
                cmd = Some(Cmd::ExportFileTypes);
                ui.close();
            }
            ui.separator();
            if ui.add(egui::Button::new("Show File Types").selected(true)).clicked() {
                cmd = Some(Cmd::ToggleFileTypes);
                ui.close();
            }
        });
        if let Some(c) = cmd {
            app.run(c, &ctx);
        }
    }
}

fn sort_ext(app: &mut App) {
    let Some(t) = &app.tree else { return };
    let (col, desc) = app.ext_sort;
    let st = &t.ext_stats;
    app.ext_rows.sort_by(|&a, &b| {
        let (x, y) = (&st[a], &st[b]);
        let o = match col {
            1 => t.exts.names[x.ext as usize].cmp(&t.exts.names[y.ext as usize]),
            2 => crate::filetypes::describe(&t.exts.names[x.ext as usize]).cmp(&crate::filetypes::describe(&t.exts.names[y.ext as usize])),
            5 => x.alloc.cmp(&y.alloc),
            6 => x.files.cmp(&y.files),
            _ => x.size.cmp(&y.size),
        };
        if desc { o.reverse() } else { o }
    });
}

// ------------------------------------------------------------------ filtering

/// Build a new tree containing only files matching `inc` and not `exc`.
pub fn filter_tree(base: &Tree, inc: &str, exc: &str, match_path: bool) -> Tree {
    let mi = Matcher::new(inc, match_path);
    let me = Matcher::new(exc, match_path);
    let n = base.nodes.len();
    let need_path = mi.needs_path || me.needs_path;
    // Per-node match results computed in parallel.
    let results: Vec<(bool, bool)> = (0..n as u32)
        .into_par_iter()
        .map_init(
            || (Vec::new(), Vec::new()),
            |(buf, stack), id| {
                let node = &base.nodes[id as usize];
                if node.is_deleted() || node.flags & flags::ROOT != 0 {
                    return (true, false);
                }
                let p = if need_path {
                    base.path_lower_into(id, buf, stack);
                    Some(&buf[..])
                } else {
                    None
                };
                let inc_ok = node.is_dir() || mi.is_empty() || mi.matches(base, id, p);
                let exc_hit = !me.is_empty() && me.matches(base, id, p);
                (inc_ok, exc_hit)
            },
        )
        .collect();
    let mut excluded = vec![false; n];
    let mut keep_file = vec![false; n];
    for i in 0..n {
        let node = &base.nodes[i];
        let p = node.parent;
        let parent_ex = p != NONE && node.flags & flags::ROOT == 0 && excluded[p as usize];
        excluded[i] = parent_ex || results[i].1 || node.is_deleted();
        if !node.is_dir() && !excluded[i] && results[i].0 {
            keep_file[i] = true;
        }
    }
    let mut has = vec![false; n];
    for i in (0..n).rev() {
        let node = &base.nodes[i];
        if keep_file[i] || (has[i] && !excluded[i]) || (node.is_dir() && mi.is_empty() && !excluded[i]) {
            has[i] = true;
            if node.parent != NONE && node.flags & flags::ROOT == 0 {
                has[node.parent as usize] = true;
            }
        }
    }
    let mut out = Tree::new();
    out.path_sep = base.path_sep;
    out.volume = base.volume.clone();
    let mut map = vec![NONE; n];
    for i in 0..n {
        let node = &base.nodes[i];
        let is_root = node.flags & flags::ROOT != 0;
        if !is_root && (!has[i] || excluded[i]) {
            continue;
        }
        let parent = if is_root { NONE } else { map[node.parent as usize] };
        if !is_root && parent == NONE {
            continue;
        }
        let nn = Node { ..*node };
        let id = out.add_node(parent, base.name(i as u32), nn);
        map[i] = id;
        if is_root {
            out.roots.push(id);
        }
    }
    out.finalize();
    out
}

pub fn visible_tree_text(app: &App) -> String {
    let Some(t) = &app.tree else { return String::new() };
    let f = app.fmt();
    let mut s = String::new();
    for r in &app.rows {
        let name = if r.group {
            let n = group_stats(t, r.node).2;
            format!("[{n} Files in {}]", t.name(r.node))
        } else {
            t.name(r.node).to_string()
        };
        let size = if r.group { group_stats(t, r.node).0 } else { t.nodes[r.node as usize].size };
        s.push_str(&"  ".repeat(r.depth as usize));
        s.push_str(&format!("{}\t{}\n", name, f.size(size)));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_keeps_matching_files_and_their_folders() {
        let mut t = Tree::new();
        t.path_sep = '/';
        let r = t.add_node(NONE, "/r", Node { flags: flags::DIR | flags::ROOT, ..Default::default() });
        t.roots.push(r);
        let a = t.add_node(r, "a", Node { flags: flags::DIR, ..Default::default() });
        t.add_node(a, "x.mp4", Node { size: 100, ..Default::default() });
        t.add_node(a, "y.txt", Node { size: 10, ..Default::default() });
        let b = t.add_node(r, "b", Node { flags: flags::DIR, ..Default::default() });
        t.add_node(b, "z.txt", Node { size: 5, ..Default::default() });
        t.finalize();
        let f = filter_tree(&t, "*.mp4", "", false);
        assert_eq!(f.total_size(), 100);
        assert_eq!(f.nodes.len(), 3);
        let f = filter_tree(&t, "", "b", false);
        assert_eq!(f.total_size(), 110);
    }

    /// Regression: a short window used to call `f32::clamp(120, h - 140)`
    /// with min > max, aborting the whole app during a scan.
    #[test]
    fn layout_never_panics_on_small_windows() {
        for h in [0.0, 1.0, 185.0, 186.0, 259.0, 260.0, 300.0, 800.0] {
            for frac in [0.0, 0.15, 0.42, 1.0] {
                let tm = treemap_height(h, frac);
                assert!((0.0..=h).contains(&tm), "h={h} frac={frac} tm={tm}");
            }
        }
        assert_eq!(treemap_height(186.0, 0.42), 0.0);
        assert!(treemap_height(800.0, 0.42) >= 120.0);
        for w in [0.0, 100.0, 432.0, 433.0, 480.0, 1280.0] {
            for want in [0.0, 260.0, 440.0, 900.0] {
                let e = ext_panel_width(w, want);
                assert!((0.0..=w).contains(&e), "w={w} want={want} e={e}");
            }
        }
        assert_eq!(ext_panel_width(400.0, 440.0), 0.0);
        assert!(ext_panel_width(1280.0, 440.0) >= 260.0);
    }
}
