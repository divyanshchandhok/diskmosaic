//! Treemap panel: cushion treemap texture plus interactive overlay.

use super::{App, Cmd, icons, tree_view};
use crate::model::{NodeId, Tree};
use crate::theme;
use crate::treemap::{self, Colors, ItemKind, Layout, Params};
use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

#[derive(Default)]
pub struct TreemapState {
    pub layout: Layout,
    pub tex: Option<egui::TextureHandle>,
    pub px_size: (u32, u32),
    pub dirty: bool,
    pub zoom: Option<NodeId>,
    pub hover: Option<usize>,
    pub rev: u64,
    pub ppp: f32,
}

pub fn colors_for<'a>(app: &App, ext_rgb: &'a [[u8; 3]], gray: bool) -> Colors<'a> {
    let pal = &app.pal;
    let bg = pal.treemap_bg;
    let other = if app.s.treemap_scheme == "Custom" {
        app.s.palette.get(12).copied().unwrap_or([72, 72, 72])
    } else {
        theme::scheme(&app.s.treemap_scheme).colors[12]
    };
    Colors {
        ext_rgb,
        default_rgb: other,
        folder_rgb: other,
        free_rgb: if pal.dark { [0x60, 0x60, 0x66] } else { [0x9C, 0x9C, 0xA4] },
        gray,
        gradient: app.s.gradient,
        background: [bg.r(), bg.g(), bg.b()],
        gain: app.scheme_gain(),
        highlight_ext: app.ext_sel,
    }
}

pub fn params_for(app: &App, labels_scale: f32) -> Params {
    let t = app.tree.as_ref();
    Params {
        use_alloc: app.s.treemap_allocated,
        show_free: app.s.treemap_free_space && app.tm.zoom.is_none(),
        free_bytes: t.map(|t| t.volume.free).unwrap_or(0),
        labels: app.s.treemap_names,
        label_h: 13.0 * labels_scale,
        ..Default::default()
    }
}

pub fn roots_for(app: &App) -> Vec<NodeId> {
    match (app.tm.zoom, &app.tree) {
        (Some(z), _) => vec![z],
        (None, Some(t)) => t.roots.clone(),
        _ => Vec::new(),
    }
}

/// Render an image of the treemap at an arbitrary size (for PNG export).
pub fn render_image(app: &App, w: u32, h: u32, gray: bool, free: bool) -> Option<Vec<u8>> {
    let t = app.tree.as_ref()?;
    let mut p = params_for(app, 1.0);
    p.show_free = free && app.tm.zoom.is_none();
    let layout = treemap::layout(t, &roots_for(app), w, h, &p);
    let ext = app.ext_colors();
    let mut c = colors_for(app, &ext, gray);
    c.highlight_ext = None;
    Some(treemap::render(t, &layout, &c))
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let pal = app.pal;
    let full = ui.max_rect();
    ui.painter().rect(full, CornerRadius::same(12), pal.card, Stroke::new(1.0, pal.separator), StrokeKind::Inside);
    let strip = Rect::from_min_max(full.min + Vec2::new(10.0, 6.0), Pos2::new(full.max.x - 10.0, full.min.y + 34.0));
    let canvas = Rect::from_min_max(Pos2::new(full.min.x + 8.0, strip.max.y + 2.0), full.max - Vec2::new(8.0, 8.0));
    let ctx = ui.ctx().clone();

    // --- strip: navigation buttons + breadcrumb / hover info
    {
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(strip).layout(egui::Layout::left_to_right(egui::Align::Center)));
        c.spacing_mut().item_spacing.x = 2.0;
        let zoomed = app.tm.zoom.is_some();
        if small_btn(&mut c, &pal, zoomed, "Zoom out one level", |p, ce, col| icons::up(p, ce, 16.0, col)).clicked() {
            app.run(Cmd::ZoomOutOne, &ctx);
        }
        if small_btn(&mut c, &pal, zoomed, "Unzoom (F11)", |p, ce, col| icons::home(p, ce, 16.0, col)).clicked() {
            app.run(Cmd::UnzoomTreemap, &ctx);
        }
        if small_btn(&mut c, &pal, !app.sel.is_empty(), "Zoom to selected folder (F10)", |p, ce, col| icons::zoom(p, ce, 16.0, col, true)).clicked() {
            app.run(Cmd::ZoomTreemap, &ctx);
        }
        c.add_space(8.0);
        let t = app.tree.as_ref().unwrap();
        let f = app.fmt();
        let text = match app.tm.hover.and_then(|i| app.tm.layout.items.get(i)) {
            Some(it) => match it.kind {
                ItemKind::Node(id) => {
                    let n = &t.nodes[id as usize];
                    format!("{}   ·   {}   ·   {} allocated", t.path(id), f.size(n.size), f.size(n.alloc))
                }
                ItemKind::FreeSpace => format!("Free Space   ·   {}", f.size(t.volume.free)),
            },
            None => match app.tm.zoom {
                Some(z) => format!("{}   ·   {}", t.path(z), f.size(t.nodes[z as usize].size)),
                None => "Treemap".to_string(),
            },
        };
        let hover = app.tm.hover.is_some();
        c.add(egui::Label::new(egui::RichText::new(text).font(theme::regular(12.0)).color(if hover { pal.text } else { pal.text2 })).truncate());
    }

    // --- texture
    let ppp = ctx.pixels_per_point();
    let pw = (canvas.width() * ppp).round().max(1.0) as u32;
    let ph = (canvas.height() * ppp).round().max(1.0) as u32;
    let rev = app.tree.as_ref().map(|t| t.revision).unwrap_or(0);
    let resizing = app.tm.px_size != (pw, ph);
    if app.tm.dirty || resizing || app.tm.rev != rev || app.tm.ppp != ppp {
        let t = app.tree.as_ref().unwrap();
        let p = params_for(app, ppp);
        let layout = treemap::layout(t, &roots_for(app), pw, ph, &p);
        let ext = app.ext_colors();
        let colors = colors_for(app, &ext, false);
        let rgba = treemap::render(t, &layout, &colors);
        let img = egui::ColorImage::from_rgba_unmultiplied([pw as usize, ph as usize], &rgba);
        match &mut app.tm.tex {
            Some(tex) => tex.set(img, egui::TextureOptions::NEAREST),
            None => app.tm.tex = Some(ctx.load_texture("treemap", img, egui::TextureOptions::NEAREST)),
        }
        app.tm.layout = layout;
        app.tm.px_size = (pw, ph);
        app.tm.dirty = false;
        app.tm.rev = rev;
        app.tm.ppp = ppp;
        app.tm.hover = None;
    }
    if let Some(tex) = &app.tm.tex {
        egui::Image::new((tex.id(), canvas.size())).corner_radius(CornerRadius::same(7)).paint_at(ui, canvas);
    }

    // --- interaction
    let resp = ui.interact(canvas, ui.scope_id().with("treemap_canvas"), Sense::click());
    let to_px = |p: Pos2| ((p.x - canvas.left()) * ppp, (p.y - canvas.top()) * ppp);
    let to_pt = |r: &treemap::Rect| Rect::from_min_size(Pos2::new(canvas.left() + r.x / ppp, canvas.top() + r.y / ppp), Vec2::new(r.w / ppp, r.h / ppp));
    app.tm.hover = None;
    if let Some(pos) = resp.hover_pos() {
        let (x, y) = to_px(pos);
        app.tm.hover = treemap::hit_test(&app.tm.layout, x, y);
        if let Some(i) = app.tm.hover {
            if let ItemKind::Node(id) = app.tm.layout.items[i].kind {
                app.hover_path = app.tree.as_ref().unwrap().path(id);
            }
        }
    }
    let painter = ui.painter_at(canvas);
    let t = app.tree.as_ref().unwrap();

    // Labels
    if app.s.treemap_names {
        draw_labels(&painter, t, &app.tm.layout, &to_pt, ppp);
    }
    // Selection outlines
    for &sid in &app.sel {
        if let Some(i) = treemap::find_item(&app.tm.layout, t, sid) {
            let r = to_pt(&app.tm.layout.items[i].rect);
            painter.rect_stroke(r.expand(1.0), CornerRadius::ZERO, Stroke::new(1.0, Color32::from_black_alpha(160)), StrokeKind::Outside);
            painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
        }
    }
    // Hover outline
    if let Some(i) = app.tm.hover {
        let r = to_pt(&app.tm.layout.items[i].rect);
        painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, Color32::from_white_alpha(220)), StrokeKind::Inside);
        // Parent folder outline gives context.
        if let ItemKind::Node(id) = app.tm.layout.items[i].kind {
            let p = t.parent(id);
            if p != crate::model::NONE {
                if let Some(pi) = treemap::find_item(&app.tm.layout, t, p) {
                    let pr = to_pt(&app.tm.layout.items[pi].rect);
                    painter.rect_stroke(pr, CornerRadius::ZERO, Stroke::new(1.0, Color32::from_white_alpha(90)), StrokeKind::Inside);
                }
            }
        }
    }
    if app.tree.as_ref().map(|t| t.total_size() == 0).unwrap_or(true) {
        painter.text(canvas.center(), Align2::CENTER_CENTER, "Nothing to show", theme::regular(13.0), pal.text3);
    }

    // Tooltip
    if let Some(i) = app.tm.hover {
        let f = app.fmt();
        let t = app.tree.as_ref().unwrap();
        let tip = match app.tm.layout.items[i].kind {
            ItemKind::Node(id) => {
                let n = &t.nodes[id as usize];
                format!("{}\nSize: {}\nAllocated: {}", t.name(id), f.size(n.size), f.size(n.alloc))
            }
            ItemKind::FreeSpace => format!("Free Space\nSize: {}", f.size(t.volume.free)),
        };
        resp.clone().on_hover_text_at_pointer(tip);
    }

    if resp.clicked() {
        if let Some(i) = app.tm.hover {
            if let ItemKind::Node(id) = app.tm.layout.items[i].kind {
                app.select_node(id);
            }
        }
    }
    if resp.double_clicked() {
        if let Some(i) = app.tm.hover {
            if let ItemKind::Node(id) = app.tm.layout.items[i].kind {
                let t = app.tree.as_ref().unwrap();
                if !t.nodes[id as usize].is_dir() && app.s.double_click_open {
                    app.sel = vec![id];
                    app.run(Cmd::Open, &ctx);
                } else {
                    let z = if t.nodes[id as usize].is_dir() { id } else { t.parent(id) };
                    if z != crate::model::NONE {
                        app.tm.zoom = Some(z);
                        app.tm.dirty = true;
                    }
                }
            }
        }
    }
    if resp.secondary_clicked() {
        if let Some(i) = app.tm.hover {
            if let ItemKind::Node(id) = app.tm.layout.items[i].kind {
                app.select_node(id);
            }
        }
    }
    let mut cmd = None;
    resp.context_menu(|ui| {
        let has_node = !app.sel.is_empty();
        if has_node {
            cmd = tree_view::node_menu(ui, app);
            ui.separator();
        }
        let mut tog = |ui: &mut Ui, label: &str, on: bool, c: Cmd| {
            if ui.add(egui::Button::new(label).selected(on)).clicked() {
                cmd = Some(c);
                ui.close();
            }
        };
        tog(ui, "Show Treemap", app.s.show_treemap, Cmd::ToggleTreemap);
        tog(ui, "Show Allocated Size", app.s.treemap_allocated, Cmd::ToggleTreemapAllocated);
        tog(ui, "Show Free Space", app.s.treemap_free_space, Cmd::ToggleTreemapFree);
        tog(ui, "Show Folder and File Names", app.s.treemap_names, Cmd::ToggleTreemapNames);
        ui.separator();
        if ui.button("Save Treemap Image…").clicked() {
            cmd = Some(Cmd::SaveTreemapImage);
            ui.close();
        }
    });
    if let Some(c) = cmd {
        app.run(c, &ctx);
    }
}

fn small_btn(ui: &mut Ui, pal: &theme::Palette, enabled: bool, tip: &str, draw: impl FnOnce(&egui::Painter, Pos2, Color32)) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(26.0, 26.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(6), pal.hover);
    }
    draw(ui.painter(), r.center(), if enabled { pal.text2 } else { theme::with_alpha(pal.text3, 0.5) });
    resp.on_hover_text(tip)
}

fn draw_labels(p: &egui::Painter, t: &Tree, layout: &Layout, to_pt: &dyn Fn(&treemap::Rect) -> Rect, ppp: f32) {
    let font = theme::medium(10.5);
    for it in &layout.items {
        let ItemKind::Node(id) = it.kind else { continue };
        let r = to_pt(&it.rect);
        if it.label_h > 0.0 {
            let strip = Rect::from_min_size(r.min, Vec2::new(r.width(), it.label_h / ppp));
            p.rect_filled(strip, CornerRadius::ZERO, Color32::from_black_alpha(110));
            let clip = p.with_clip_rect(strip.intersect(p.clip_rect()));
            clip.text(Pos2::new(strip.left() + 4.0, strip.center().y), Align2::LEFT_CENTER, t.name(id), font.clone(), Color32::from_white_alpha(235));
        } else if it.leaf && r.width() > 48.0 && r.height() > 16.0 && !t.nodes[id as usize].is_dir() {
            let clip = p.with_clip_rect(r.shrink(2.0).intersect(p.clip_rect()));
            let c = r.center();
            clip.text(c + Vec2::new(0.6, 0.6), Align2::CENTER_CENTER, t.name(id), font.clone(), Color32::from_black_alpha(170));
            clip.text(c, Align2::CENTER_CENTER, t.name(id), font.clone(), Color32::from_white_alpha(240));
        }
    }
}
