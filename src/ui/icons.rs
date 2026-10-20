//! Small vector icons painted directly (crisp at any scale, theme aware).

use eframe::egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, Vec2, epaint::CubicBezierShape};

pub fn folder(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let w = s;
    let h = s * 0.78;
    let r = Rect::from_center_size(c + Vec2::new(0.0, s * 0.04), Vec2::new(w, h));
    let tab = Rect::from_min_size(r.min - Vec2::new(0.0, s * 0.10), Vec2::new(w * 0.44, s * 0.24));
    let dark = Color32::from_rgb(
        (color.r() as f32 * 0.78) as u8,
        (color.g() as f32 * 0.78) as u8,
        (color.b() as f32 * 0.78) as u8,
    );
    p.rect_filled(tab, CornerRadius::same((s * 0.12) as u8), dark);
    p.rect_filled(r, CornerRadius::same((s * 0.14) as u8), dark);
    let front = Rect::from_min_max(Pos2::new(r.left(), r.top() + h * 0.18), r.max);
    p.rect_filled(front, CornerRadius::same((s * 0.14) as u8), color);
}

pub fn file(p: &Painter, c: Pos2, s: f32, body: Color32, accent: Color32) {
    let w = s * 0.76;
    let h = s;
    let r = Rect::from_center_size(c, Vec2::new(w, h));
    let fold = s * 0.28;
    let pts = vec![
        r.left_top(),
        Pos2::new(r.right() - fold, r.top()),
        Pos2::new(r.right(), r.top() + fold),
        r.right_bottom(),
        r.left_bottom(),
    ];
    p.add(Shape::convex_polygon(pts, body, Stroke::NONE));
    p.add(Shape::convex_polygon(
        vec![Pos2::new(r.right() - fold, r.top()), Pos2::new(r.right(), r.top() + fold), Pos2::new(r.right() - fold, r.top() + fold)],
        Color32::from_white_alpha(90),
        Stroke::NONE,
    ));
    let band = Rect::from_min_max(Pos2::new(r.left(), r.bottom() - h * 0.30), r.right_bottom());
    p.rect_filled(band, CornerRadius::ZERO, accent);
}

pub fn chevron(p: &Painter, c: Pos2, s: f32, open: bool, color: Color32) {
    let h = s * 0.5;
    let pts = if open {
        vec![Pos2::new(c.x - h * 0.7, c.y - h * 0.35), Pos2::new(c.x, c.y + h * 0.4), Pos2::new(c.x + h * 0.7, c.y - h * 0.35)]
    } else {
        vec![Pos2::new(c.x - h * 0.35, c.y - h * 0.7), Pos2::new(c.x + h * 0.4, c.y), Pos2::new(c.x - h * 0.35, c.y + h * 0.7)]
    };
    p.add(Shape::line(pts, Stroke::new(1.6, color)));
}

pub fn search(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let r = s * 0.32;
    let cc = c - Vec2::splat(s * 0.08);
    p.circle_stroke(cc, r, Stroke::new(1.6, color));
    let d = Vec2::splat(r * 0.72);
    p.line_segment([cc + d, cc + d * 2.0], Stroke::new(1.8, color));
}

pub fn drive(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let r = Rect::from_center_size(c, Vec2::new(s, s * 0.62));
    p.rect_stroke(r, CornerRadius::same((s * 0.16) as u8), Stroke::new(1.5, color), eframe::egui::StrokeKind::Inside);
    p.circle_filled(Pos2::new(r.right() - s * 0.2, r.center().y + s * 0.06), s * 0.07, color);
    p.line_segment([Pos2::new(r.left() + s * 0.16, r.center().y + s * 0.06), Pos2::new(r.center().x, r.center().y + s * 0.06)], Stroke::new(1.5, color));
}

pub fn filter(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let w = s * 0.8;
    let top = c.y - s * 0.32;
    let pts = vec![
        Pos2::new(c.x - w / 2.0, top),
        Pos2::new(c.x + w / 2.0, top),
        Pos2::new(c.x + s * 0.08, c.y + s * 0.02),
        Pos2::new(c.x + s * 0.08, c.y + s * 0.34),
        Pos2::new(c.x - s * 0.08, c.y + s * 0.26),
        Pos2::new(c.x - s * 0.08, c.y + s * 0.02),
    ];
    p.add(Shape::closed_line(pts, Stroke::new(1.5, color)));
}

pub fn gear(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let ro = s * 0.42;
    let ri = s * 0.30;
    let mut pts = Vec::new();
    let n = 8;
    for i in 0..n * 4 {
        let a = i as f32 / (n * 4) as f32 * std::f32::consts::TAU;
        let r = if (i / 2) % 2 == 0 { ro } else { ri };
        pts.push(c + Vec2::angled(a) * r);
    }
    p.add(Shape::closed_line(pts, Stroke::new(1.4, color)));
    p.circle_stroke(c, s * 0.12, Stroke::new(1.4, color));
}

pub fn stop(p: &Painter, c: Pos2, s: f32, color: Color32) {
    p.rect_filled(Rect::from_center_size(c, Vec2::splat(s * 0.42)), CornerRadius::same(2), color);
}

pub fn play(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let h = s * 0.26;
    p.add(Shape::convex_polygon(
        vec![Pos2::new(c.x - h * 0.8, c.y - h), Pos2::new(c.x + h, c.y), Pos2::new(c.x - h * 0.8, c.y + h)],
        color,
        Stroke::NONE,
    ));
}

pub fn up(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let h = s * 0.3;
    p.add(Shape::line(vec![Pos2::new(c.x - h, c.y + h * 0.3), Pos2::new(c.x, c.y - h * 0.6), Pos2::new(c.x + h, c.y + h * 0.3)], Stroke::new(1.6, color)));
}

pub fn home(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let h = s * 0.34;
    p.add(Shape::closed_line(
        vec![
            Pos2::new(c.x - h, c.y),
            Pos2::new(c.x, c.y - h),
            Pos2::new(c.x + h, c.y),
            Pos2::new(c.x + h * 0.7, c.y),
            Pos2::new(c.x + h * 0.7, c.y + h * 0.9),
            Pos2::new(c.x - h * 0.7, c.y + h * 0.9),
            Pos2::new(c.x - h * 0.7, c.y),
        ],
        Stroke::new(1.4, color),
    ));
}

pub fn zoom(p: &Painter, c: Pos2, s: f32, color: Color32, plus: bool) {
    search(p, c, s, color);
    let cc = c - Vec2::splat(s * 0.08);
    let l = s * 0.14;
    p.line_segment([cc - Vec2::new(l, 0.0), cc + Vec2::new(l, 0.0)], Stroke::new(1.4, color));
    if plus {
        p.line_segment([cc - Vec2::new(0.0, l), cc + Vec2::new(0.0, l)], Stroke::new(1.4, color));
    }
}

pub fn warning(p: &Painter, c: Pos2, s: f32, color: Color32) {
    let h = s * 0.45;
    p.add(Shape::convex_polygon(
        vec![Pos2::new(c.x, c.y - h), Pos2::new(c.x + h * 1.05, c.y + h * 0.8), Pos2::new(c.x - h * 1.05, c.y + h * 0.8)],
        color,
        Stroke::NONE,
    ));
    p.line_segment([Pos2::new(c.x, c.y - h * 0.35), Pos2::new(c.x, c.y + h * 0.3)], Stroke::new(1.6, Color32::WHITE));
    p.circle_filled(Pos2::new(c.x, c.y + h * 0.56), 1.1, Color32::WHITE);
}

pub fn spinner(p: &Painter, c: Pos2, s: f32, color: Color32, t: f64) {
    let r = s * 0.38;
    let start = (t * 5.0) as f32;
    let n = 24;
    let pts: Vec<Pos2> = (0..=n).map(|i| c + Vec2::angled(start + i as f32 / n as f32 * 4.2) * r).collect();
    p.add(Shape::line(pts, Stroke::new(2.0, color)));
}

#[allow(dead_code)]
pub fn curve(p: &Painter, a: Pos2, b: Pos2, color: Color32) {
    p.add(CubicBezierShape::from_points_stroke([a, Pos2::new(a.x, b.y), Pos2::new(a.x, b.y), b], false, Color32::TRANSPARENT, Stroke::new(1.0, color)));
}
