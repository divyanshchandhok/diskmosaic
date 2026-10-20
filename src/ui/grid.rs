//! Virtualised, custom-painted table used by the tree, file and type lists.
//! Only visible rows are laid out, so millions of rows cost nothing.

use crate::theme::{self, Palette};
use eframe::egui::{self, Align2, Color32, CornerRadius, Id, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Debug)]
pub struct Col {
    pub title: String,
    pub width: f32,
    pub right: bool,
    pub min: f32,
}

impl Col {
    pub fn new(title: impl Into<String>, width: f32, right: bool) -> Col {
        Col { title: title.into(), width, right, min: 28.0 }
    }
}

pub struct GridOut {
    pub rows: Vec<(usize, Response)>,
    pub clicked: Option<(usize, egui::Modifiers)>,
    pub double_clicked: Option<usize>,
    pub secondary: Option<usize>,
    pub header_clicked: Option<usize>,
    pub hovered: Option<usize>,
    pub widths_changed: bool,
    pub body_rect: Rect,
    pub drag_started: Option<usize>,
}

impl Default for GridOut {
    fn default() -> Self {
        GridOut {
            rows: Vec::new(),
            clicked: None,
            double_clicked: None,
            secondary: None,
            header_clicked: None,
            hovered: None,
            widths_changed: false,
            body_rect: Rect::NOTHING,
            drag_started: None,
        }
    }
}

pub struct Grid<'a> {
    pub id: Id,
    pub cols: &'a mut [Col],
    pub sort: Option<(usize, bool)>,
    pub rows: usize,
    pub row_h: f32,
    pub scroll_to: Option<usize>,
    pub alt_rows: bool,
    pub hlines: bool,
    /// Column that absorbs spare (or missing) width so the table fits.
    pub flex: Option<usize>,
}

impl<'a> Grid<'a> {
    pub fn show(
        self,
        ui: &mut Ui,
        pal: &Palette,
        is_selected: impl Fn(usize) -> bool,
        mut cell: impl FnMut(&mut CellCtx, usize, usize),
    ) -> GridOut {
        let mut out = GridOut::default();
        let header_h = 28.0;
        let cols = self.cols;
        let id = self.id;
        // Fit the flex column to the view: the stored width stays the
        // user's, only the painted width changes.
        let mut flex_delta = 0.0;
        if let Some(f) = self.flex.filter(|&f| f < cols.len()) {
            let sum: f32 = cols.iter().map(|c| c.width).sum::<f32>() + 16.0;
            let want = (cols[f].width + ui.available_width() - sum).max(cols[f].min);
            flex_delta = want - cols[f].width;
            cols[f].width = want;
        }
        let total_w: f32 = cols.iter().map(|c| c.width).sum::<f32>() + 8.0;
        egui::ScrollArea::horizontal().id_salt(id.with("h")).auto_shrink([false, false]).show(ui, |ui| {
            let avail_w = ui.available_width().max(total_w);
            // Header
            let (hrect, _) = ui.allocate_exact_size(Vec2::new(avail_w, header_h), Sense::hover());
            let painter = ui.painter_at(hrect);
            painter.line_segment(
                [Pos2::new(hrect.left(), hrect.bottom() - 0.5), Pos2::new(hrect.right(), hrect.bottom() - 0.5)],
                Stroke::new(1.0, pal.separator),
            );
            let mut x = hrect.left() + 8.0;
            for (ci, c) in cols.iter_mut().enumerate() {
                let r = Rect::from_min_size(Pos2::new(x, hrect.top()), Vec2::new(c.width, header_h));
                let resp = ui.interact(r.shrink2(Vec2::new(4.0, 0.0)), id.with(("hdr", ci)), Sense::click());
                if resp.hovered() {
                    painter.rect_filled(r.shrink2(Vec2::new(1.0, 4.0)), CornerRadius::same(5), pal.hover);
                }
                if resp.clicked() {
                    out.header_clicked = Some(ci);
                }
                let sorted = self.sort.filter(|s| s.0 == ci);
                let color = if sorted.is_some() { pal.text } else { pal.text2 };
                let font = theme::medium(11.5);
                let pad = 8.0;
                let text_rect = r.shrink2(Vec2::new(pad, 0.0));
                let painter_c = painter.with_clip_rect(text_rect.intersect(hrect));
                let arrow_w = if sorted.is_some() { 12.0 } else { 0.0 };
                if c.right {
                    painter_c.text(Pos2::new(text_rect.right() - arrow_w, r.center().y), Align2::RIGHT_CENTER, crate::i18n::tr(&c.title), font, color);
                } else {
                    painter_c.text(Pos2::new(text_rect.left(), r.center().y), Align2::LEFT_CENTER, crate::i18n::tr(&c.title), font, color);
                }
                if let Some((_, desc)) = sorted {
                    let ax = if c.right { text_rect.right() - 4.0 } else {
                        let g = painter.layout_no_wrap(crate::i18n::tr(&c.title), theme::medium(11.5), color);
                        (text_rect.left() + g.size().x + 8.0).min(text_rect.right() - 4.0)
                    };
                    let cy = r.center().y;
                    let pts = if desc {
                        vec![Pos2::new(ax - 3.5, cy - 1.5), Pos2::new(ax + 3.5, cy - 1.5), Pos2::new(ax, cy + 2.5)]
                    } else {
                        vec![Pos2::new(ax - 3.5, cy + 2.0), Pos2::new(ax + 3.5, cy + 2.0), Pos2::new(ax, cy - 2.0)]
                    };
                    painter.add(egui::Shape::convex_polygon(pts, pal.text2, Stroke::NONE));
                }
                // Resize handle
                let hr = Rect::from_center_size(Pos2::new(r.right(), r.center().y), Vec2::new(8.0, header_h));
                let h = ui.interact(hr, id.with(("rsz", ci)), Sense::drag());
                if h.hovered() || h.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    painter.line_segment([Pos2::new(r.right(), r.top() + 7.0), Pos2::new(r.right(), r.bottom() - 7.0)], Stroke::new(1.0, pal.border));
                } else {
                    painter.line_segment([Pos2::new(r.right(), r.top() + 9.0), Pos2::new(r.right(), r.bottom() - 9.0)], Stroke::new(1.0, pal.separator));
                }
                if h.dragged() {
                    c.width = (c.width + h.drag_delta().x).max(c.min);
                    out.widths_changed = true;
                }
                x += c.width;
            }

            // Body
            let mut sa = egui::ScrollArea::vertical().id_salt(id.with("v")).auto_shrink([false, false]);
            if let Some(r) = self.scroll_to {
                let avail = ui.available_height();
                let cur = egui::scroll_area::State::load(ui.ctx(), ui.make_persistent_id(id.with("v")))
                    .map(|s| s.clamped_offset().y)
                    .unwrap_or(0.0);
                let y = r as f32 * self.row_h;
                if y < cur || y + self.row_h > cur + avail - self.row_h {
                    sa = sa.vertical_scroll_offset((y - avail / 2.0).max(0.0));
                }
            }
            let row_h = self.row_h;
            let alt = self.alt_rows;
            let hlines = self.hlines;
            let nrows = self.rows;
            let so = sa.show_rows(ui, row_h, nrows, |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in range {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail_w, row_h), Sense::click_and_drag());
                    let selected = is_selected(row);
                    let painter = ui.painter_at(rect);
                    let bg_rect = Rect::from_min_max(Pos2::new(rect.left() + 4.0, rect.top()), Pos2::new(rect.right() - 4.0, rect.bottom()));
                    if selected {
                        let f = ui.ctx().memory(|m| m.focused().is_none());
                        let c = if f { theme::with_alpha(pal.accent, if pal.dark { 0.42 } else { 0.20 }) } else { theme::with_alpha(pal.accent, 0.18) };
                        painter.rect_filled(bg_rect, CornerRadius::same(6), c);
                    } else if resp.hovered() {
                        painter.rect_filled(bg_rect, CornerRadius::same(6), pal.hover);
                    } else if alt && row % 2 == 1 {
                        painter.rect_filled(bg_rect, CornerRadius::same(6), pal.row_alt);
                    }
                    if hlines {
                        painter.line_segment([Pos2::new(rect.left() + 8.0, rect.bottom() - 0.5), Pos2::new(rect.right() - 8.0, rect.bottom() - 0.5)], Stroke::new(1.0, pal.separator));
                    }
                    let mut x = rect.left() + 8.0;
                    for ci in 0..cols.len() {
                        let w = cols[ci].width;
                        let cr = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(w, row_h));
                        let mut cc = CellCtx {
                            painter: painter.with_clip_rect(cr.shrink2(Vec2::new(2.0, 0.0)).intersect(rect)),
                            rect: cr.shrink2(Vec2::new(8.0, 0.0)),
                            right: cols[ci].right,
                            pal: *pal,
                            selected,
                        };
                        cell(&mut cc, row, ci);
                        x += w;
                    }
                    if resp.clicked() {
                        out.clicked = Some((row, ui.input(|i| i.modifiers)));
                    }
                    if resp.double_clicked() {
                        out.double_clicked = Some(row);
                    }
                    if resp.secondary_clicked() {
                        out.secondary = Some(row);
                    }
                    if resp.hovered() {
                        out.hovered = Some(row);
                    }
                    if resp.drag_started() {
                        out.drag_started = Some(row);
                    }
                    out.rows.push((row, resp));
                }
            });
            out.body_rect = so.inner_rect;
        });
        if let Some(f) = self.flex.filter(|&f| f < cols.len()) {
            cols[f].width = (cols[f].width - flex_delta).max(cols[f].min);
        }
        out
    }
}

pub struct CellCtx {
    pub painter: egui::Painter,
    pub rect: Rect,
    pub right: bool,
    pub pal: Palette,
    #[allow(dead_code)]
    pub selected: bool,
}

impl CellCtx {
    pub fn text(&self, s: &str, color: Color32) {
        self.text_font(s, theme::regular(12.5), color);
    }
    pub fn text_font(&self, s: &str, font: egui::FontId, color: Color32) {
        if self.right {
            self.painter.text(Pos2::new(self.rect.right(), self.rect.center().y), Align2::RIGHT_CENTER, s, font, color);
        } else {
            self.painter.text(Pos2::new(self.rect.left(), self.rect.center().y), Align2::LEFT_CENTER, s, font, color);
        }
    }
    pub fn text_at(&self, x: f32, s: &str, font: egui::FontId, color: Color32) {
        self.painter.text(Pos2::new(x, self.rect.center().y), Align2::LEFT_CENTER, s, font, color);
    }
    /// A rounded percentage bar with label.
    pub fn bar(&self, frac: f32, color: Color32, label: &str) {
        let h = 6.0;
        let label_w = 46.0;
        let bw = (self.rect.width() - label_w - 6.0).max(10.0);
        let r = Rect::from_min_size(Pos2::new(self.rect.left(), self.rect.center().y - h / 2.0), Vec2::new(bw, h));
        self.painter.rect_filled(r, CornerRadius::same(3), theme::with_alpha(self.pal.text3, 0.22));
        let fw = (bw * frac.clamp(0.0, 1.0)).max(if frac > 0.0 { 2.0 } else { 0.0 });
        if fw > 0.0 {
            self.painter.rect_filled(Rect::from_min_size(r.min, Vec2::new(fw, h)), CornerRadius::same(3), color);
        }
        self.painter.text(
            Pos2::new(self.rect.right(), self.rect.center().y),
            Align2::RIGHT_CENTER,
            label,
            theme::regular(11.5),
            self.pal.text2,
        );
    }
}
