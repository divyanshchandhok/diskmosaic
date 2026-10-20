//! Squarified, cushion-shaded treemap.
//!
//! Layout produces a flat list of rectangles in depth-first order (each with
//! the index one past its subtree, so hit-testing can skip whole subtrees).
//! Rendering is a CPU raster pass parallelised over horizontal bands.

use crate::model::{NodeId, Tree};
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ItemKind {
    Node(NodeId),
    /// Synthetic free space block.
    FreeSpace,
}

#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub kind: ItemKind,
    pub rect: Rect,
    #[allow(dead_code)]
    pub depth: u16,
    /// Index one past the last descendant of this item in `Layout::items`.
    pub end: u32,
    /// Cushion surface coefficients (x², x, y², y).
    pub surface: [f32; 4],
    pub leaf: bool,
    /// Folder whose children don't cover its whole rectangle (label strip
    /// or pruned tiny items): painted underneath its children.
    pub backdrop: bool,
    /// Vertical space reserved at the top for the folder's label.
    pub label_h: f32,
}

#[derive(Clone, Default)]
pub struct Layout {
    pub items: Vec<Item>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct Params {
    pub use_alloc: bool,
    pub show_free: bool,
    pub free_bytes: u64,
    pub labels: bool,
    pub label_h: f32,
    pub min_px: f32,
    /// Cushion height and per-level scale factor.
    pub height: f32,
    pub scale: f32,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            use_alloc: false,
            show_free: false,
            free_bytes: 0,
            labels: false,
            label_h: 12.0,
            min_px: 0.6,
            // Recovered from WizTreeMac: root ridge 0.18, x0.91 per level.
            height: 0.18,
            scale: 0.91,
        }
    }
}

fn add_ridge(s: &mut [f32; 4], x0: f32, x1: f32, y0: f32, y1: f32, h: f32) {
    let w = x1 - x0;
    // van Wijk & van de Wetering: a parabolic ridge whose edge slope is 4h.
    if w > 0.0 {
        s[0] -= 4.0 * h / w;
        s[1] += 4.0 * h * (x1 + x0) / w;
    }
    let hh = y1 - y0;
    if hh > 0.0 {
        s[2] -= 4.0 * h / hh;
        s[3] += 4.0 * h * (y1 + y0) / hh;
    }
}

/// Lay out `roots` (multi-select gives several) into a `w`×`h` canvas.
pub fn layout(tree: &Tree, roots: &[NodeId], w: u32, h: u32, p: &Params) -> Layout {
    let mut out = Layout { items: Vec::new(), width: w, height: h };
    if w == 0 || h == 0 || roots.is_empty() {
        return out;
    }
    let full = Rect { x: 0.0, y: 0.0, w: w as f32, h: h as f32 };
    let metric = |id: NodeId| tree.nodes[id as usize].metric(p.use_alloc);
    // Top level: roots + optional free space, laid out together.
    let mut top: Vec<(Option<NodeId>, u64)> = roots.iter().map(|&r| (Some(r), metric(r))).collect();
    if p.show_free && p.free_bytes > 0 {
        top.push((None, p.free_bytes));
    }
    top.sort_by(|a, b| b.1.cmp(&a.1));
    let total: u64 = top.iter().map(|t| t.1).sum();
    if total == 0 {
        return out;
    }
    let sizes: Vec<u64> = top.iter().map(|t| t.1).collect();
    let rects = squarify(&sizes, full);
    let surface = [0.0f32; 4];
    for (i, (item, _)) in top.iter().enumerate() {
        let r = rects[i];
        match item {
            Some(id) => lay_node(tree, *id, r, surface, p.height, 0, p, &mut out),
            None => {
                let mut s = surface;
                add_ridge(&mut s, r.x, r.x + r.w, r.y, r.y + r.h, p.height);
                let idx = out.items.len() as u32;
                out.items.push(Item { kind: ItemKind::FreeSpace, rect: r, depth: 0, end: idx + 1, surface: s, leaf: true, backdrop: false, label_h: 0.0 });
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn lay_node(tree: &Tree, id: NodeId, r: Rect, surface: [f32; 4], h: f32, depth: u16, p: &Params, out: &mut Layout) {
    let mut s = surface;
    if depth > 0 {
        add_ridge(&mut s, r.x, r.x + r.w, r.y, r.y + r.h, h);
    }
    let idx = out.items.len();
    let node = &tree.nodes[id as usize];
    let kids = tree.kids(id);
    let is_dir = node.is_dir();
    out.items.push(Item { kind: ItemKind::Node(id), rect: r, depth, end: 0, surface: s, leaf: !is_dir || kids.is_empty(), backdrop: false, label_h: 0.0 });
    if is_dir && !kids.is_empty() && r.w >= p.min_px * 2.0 && r.h >= p.min_px * 2.0 {
        let mut inner = r;
        if p.labels && r.h > p.label_h + 2.0 && r.w >= 80.0 {
            inner = Rect { x: r.x + 2.0, y: r.y + p.label_h + 1.0, w: (r.w - 4.0).max(0.0), h: (r.h - p.label_h - 2.0).max(0.0) };
            out.items[idx].label_h = p.label_h;
            out.items[idx].backdrop = true;
        }
        let total = node.metric(p.use_alloc);
        if total > 0 {
            // Children are pre-sorted by size; with allocated metric the order
            // may differ slightly, which squarify tolerates.
            let area = inner.w as f64 * inner.h as f64;
            let min_area = (p.min_px * p.min_px) as f64;
            let mut sizes: Vec<u64> = Vec::with_capacity(kids.len());
            let mut ids: Vec<NodeId> = Vec::with_capacity(kids.len());
            let mut ordered: Vec<NodeId> = kids.to_vec();
            if p.use_alloc {
                ordered.sort_by(|a, b| tree.nodes[*b as usize].alloc.cmp(&tree.nodes[*a as usize].alloc));
            }
            for &k in &ordered {
                if ids.len() >= 50_000 {
                    break;
                }
                let m = tree.nodes[k as usize].metric(p.use_alloc);
                if m == 0 {
                    continue;
                }
                if (m as f64 / total as f64) * area < min_area {
                    continue;
                }
                sizes.push(m);
                ids.push(k);
            }
            if !sizes.is_empty() {
                // Scale the laid-out area so tiny pruned items keep their share of space.
                let laid: u64 = sizes.iter().sum();
                let frac = (laid as f64 / total as f64) as f32;
                let mut sub = inner;
                if frac < 0.999 {
                    out.items[idx].backdrop = true;
                    if sub.w >= sub.h {
                        sub.w *= frac;
                    } else {
                        sub.h *= frac;
                    }
                }
                let rects = squarify(&sizes, sub);
                for (i, &k) in ids.iter().enumerate() {
                    lay_node(tree, k, rects[i], s, h * p.scale, depth + 1, p, out);
                }
            } else {
                out.items[idx].leaf = true;
            }
        }
    } else if is_dir {
        out.items[idx].leaf = true;
    }
    let end = out.items.len() as u32;
    out.items[idx].end = end;
}

/// Squarified treemap layout for sizes sorted descending.
pub fn squarify(sizes: &[u64], r: Rect) -> Vec<Rect> {
    let mut out = vec![Rect::default(); sizes.len()];
    let total: f64 = sizes.iter().map(|&s| s as f64).sum();
    if total <= 0.0 || sizes.is_empty() {
        return out;
    }
    let scale = (r.w as f64 * r.h as f64) / total;
    let (mut x, mut y, mut w, mut h) = (r.x as f64, r.y as f64, r.w as f64, r.h as f64);
    let mut i = 0;
    while i < sizes.len() {
        let side = w.min(h);
        if side <= 0.0 {
            for o in out.iter_mut().skip(i) {
                *o = Rect { x: x as f32, y: y as f32, w: 0.0, h: 0.0 };
            }
            break;
        }
        // Grow the row while the worst aspect ratio improves.
        let mut j = i;
        let mut row_sum = 0.0;
        let mut best = f64::INFINITY;
        while j < sizes.len() {
            let a = sizes[j] as f64 * scale;
            let s2 = row_sum + a;
            let maxv = sizes[i] as f64 * scale;
            let minv = a;
            let side2 = side * side;
            let worst = (side2 * maxv / (s2 * s2)).max(s2 * s2 / (side2 * minv));
            if worst > best && j > i {
                break;
            }
            best = worst;
            row_sum = s2;
            j += 1;
        }
        let thick = row_sum / side;
        let mut off = 0.0;
        let last_row = j >= sizes.len();
        for k in i..j {
            let a = sizes[k] as f64 * scale;
            let len = if thick > 0.0 { a / thick } else { 0.0 };
            out[k] = if w >= h {
                // vertical strip on the left
                Rect { x: x as f32, y: (y + off) as f32, w: if last_row { w } else { thick } as f32, h: len as f32 }
            } else {
                Rect { x: (x + off) as f32, y: y as f32, w: len as f32, h: if last_row { h } else { thick } as f32 }
            };
            off += len;
        }
        if w >= h {
            x += thick;
            w -= thick;
        } else {
            y += thick;
            h -= thick;
        }
        i = j;
    }
    out
}

/// Colour lookup supplied by the caller (index by extension id).
pub struct Colors<'a> {
    pub ext_rgb: &'a [[u8; 3]],
    pub default_rgb: [u8; 3],
    pub folder_rgb: [u8; 3],
    pub free_rgb: [u8; 3],
    pub gray: bool,
    pub gradient: bool,
    pub background: [u8; 3],
    /// Colour multiplier applied before shading (see `theme::Scheme::gain`).
    pub gain: f32,
    /// Optional highlight: items with this extension get a light overlay.
    pub highlight_ext: Option<u16>,
}

/// Light vector (normalised in `render`).
const LX: f32 = -1.0;
const LY: f32 = -1.0;
const LZ: f32 = 10.0;

pub fn item_color(tree: &Tree, item: &Item, c: &Colors) -> [u8; 3] {
    match item.kind {
        ItemKind::FreeSpace => c.free_rgb,
        ItemKind::Node(id) => {
            let n = &tree.nodes[id as usize];
            if n.is_dir() {
                c.folder_rgb
            } else if c.gray {
                [200, 200, 200]
            } else {
                c.ext_rgb.get(n.ext as usize).copied().unwrap_or(c.default_rgb)
            }
        }
    }
}

/// Render the layout into an RGBA buffer (row-major, `width*height*4`).
pub fn render(tree: &Tree, layout: &Layout, c: &Colors) -> Vec<u8> {
    let (w, h) = (layout.width as usize, layout.height as usize);
    let mut buf = vec![0u8; w * h * 4];
    for px in buf.chunks_exact_mut(4) {
        px[0] = c.background[0];
        px[1] = c.background[1];
        px[2] = c.background[2];
        px[3] = 255;
    }
    if w == 0 || h == 0 {
        return buf;
    }
    let len = (LX * LX + LY * LY + LZ * LZ).sqrt();
    let (lx, ly, lz) = (LX / len, LY / len, LZ / len);
    // Leaves plus partially covered folders. Items are in depth-first order,
    // so a folder backdrop is always painted before its children.
    let leaves: Vec<&Item> = layout.items.iter().filter(|i| (i.leaf || i.backdrop) && i.rect.w > 0.0 && i.rect.h > 0.0).collect();
    // Bucket leaves per band so each band only touches its own rows.
    let band_h = 32usize;
    let bands = h.div_ceil(band_h);
    let mut per_band: Vec<Vec<usize>> = vec![Vec::new(); bands];
    for (i, it) in leaves.iter().enumerate() {
        let y0 = it.rect.y.max(0.0) as usize;
        let y1 = ((it.rect.y + it.rect.h).ceil() as usize).min(h);
        if y1 <= y0 {
            continue;
        }
        for b in (y0 / band_h)..=((y1 - 1) / band_h).min(bands - 1) {
            per_band[b].push(i);
        }
    }
    buf.par_chunks_mut(band_h * w * 4).enumerate().for_each(|(b, chunk)| {
        let by0 = b * band_h;
        for &li in &per_band[b] {
            let it = leaves[li];
            let base = item_color(tree, it, c);
            let hl = matches!((c.highlight_ext, it.kind), (Some(e), ItemKind::Node(id)) if !tree.nodes[id as usize].is_dir() && tree.nodes[id as usize].ext == e);
            let x0 = it.rect.x.round().max(0.0) as usize;
            let x1 = ((it.rect.x + it.rect.w).round() as usize).min(w);
            let y0 = (it.rect.y.round().max(0.0) as usize).max(by0);
            let y1 = ((it.rect.y + it.rect.h).round() as usize).min(by0 + band_h).min(h);
            let (x0, x1) = if x1 <= x0 && x0 < w { (x0, x0 + 1) } else { (x0, x1) };
            let s = it.surface;
            // Rectangles bigger than 4px get a darker right/bottom edge.
            let edge = if (x1 - x0) as f32 > 4.0 && (it.rect.h) > 4.0 { 0.8 } else { 1.0 };
            let last_x = x1.saturating_sub(1);
            let last_y = ((it.rect.y + it.rect.h).round() as usize).min(h).saturating_sub(1);
            for y in y0..y1 {
                let row = (y - by0) * w * 4;
                let fy = y as f32 + 0.5;
                let gy = 2.0 * s[2] * fy + s[3];
                for x in x0..x1 {
                    let mut v = if c.gradient {
                        let fx = x as f32 + 0.5;
                        let gx = 2.0 * s[0] * fx + s[1];
                        let cos = (lz - gy * ly - gx * lx) / (gx * gx + gy * gy + 1.0).sqrt();
                        cos.min(1.0)
                    } else {
                        0.85
                    };
                    if x == last_x || y == last_y {
                        v *= edge;
                    }
                    let v = (v * 0.87 + 0.13) * 1.466_666_7 * c.gain;
                    let o = row + x * 4;
                    let mut r = base[0] as f32 * v;
                    let mut g = base[1] as f32 * v;
                    let mut bb = base[2] as f32 * v;
                    if hl {
                        r = r * 0.35 + 255.0 * 0.65;
                        g = g * 0.35 + 255.0 * 0.65;
                        bb = bb * 0.35 + 255.0 * 0.65;
                    }
                    chunk[o] = r.min(255.0) as u8;
                    chunk[o + 1] = g.min(255.0) as u8;
                    chunk[o + 2] = bb.min(255.0) as u8;
                }
            }
        }
    });
    buf
}

/// Find the deepest item containing (x, y); returns item index.
pub fn hit_test(layout: &Layout, x: f32, y: f32) -> Option<usize> {
    let items = &layout.items;
    let mut i = 0usize;
    let mut found = None;
    let mut end = items.len();
    while i < end {
        let it = &items[i];
        if it.rect.contains(x, y) {
            found = Some(i);
            end = it.end as usize;
            i += 1;
        } else {
            i = it.end as usize;
        }
    }
    found
}

/// Index of the item for a given node (if laid out).
pub fn find_item(layout: &Layout, tree: &Tree, id: NodeId) -> Option<usize> {
    // Walk down following the node's ancestor chain for O(depth * siblings).
    let chain = tree.chain(id);
    let items = &layout.items;
    let mut i = 0usize;
    let mut end = items.len();
    let mut ci = 0usize;
    // Multi-root: first level matches any root in chain[0].
    while i < end && ci < chain.len() {
        let it = &items[i];
        match it.kind {
            ItemKind::Node(n) if n == chain[ci] => {
                if ci == chain.len() - 1 {
                    return Some(i);
                }
                ci += 1;
                end = it.end as usize;
                i += 1;
            }
            _ => i = it.end as usize,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squarify_covers_area() {
        let sizes = [600u64, 300, 50, 30, 20];
        let r = Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 };
        let rects = squarify(&sizes, r);
        let area: f32 = rects.iter().map(|r| r.w * r.h).sum();
        assert!((area - 5000.0).abs() < 1.0, "{area}");
        for (s, rr) in sizes.iter().zip(&rects) {
            let expect = *s as f32 / 1000.0 * 5000.0;
            assert!((rr.w * rr.h - expect).abs() < 1.0);
            assert!(rr.x >= -0.01 && rr.y >= -0.01 && rr.x + rr.w <= 100.01 && rr.y + rr.h <= 50.01);
        }
    }
}
