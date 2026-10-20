//! Offscreen renders of the main screens for visual review:
//! `WT_SNAP_PATH=/some/dir cargo test --release snapshots -- --ignored`
//! PNGs are written to `$WT_SNAP_OUT` (default `target/snapshots`).

use super::{App, Tab};
use eframe::egui;
use egui_kittest::Harness;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn out_dir() -> PathBuf {
    let d = std::env::var("WT_SNAP_OUT").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("target/snapshots"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn click(h: &mut Harness<'_, App>, pos: egui::Pos2, button: egui::PointerButton) {
    h.hover_at(pos);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos, button, pressed, modifiers: Default::default() });
        h.run_steps(1);
    }
}

fn shot(h: &mut Harness<'_, App>, name: &str) {
    h.run_steps(4);
    let img = h.render().expect("render");
    img.save(out_dir().join(format!("{name}.png"))).unwrap();
}

#[test]
#[ignore]
fn snapshots() {
    let path = std::env::var("WT_SNAP_PATH").unwrap_or_else(|_| "/usr".into());
    let mut h = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_pixels_per_point(1.0)
        .with_max_steps(100_000)
        .with_accessibility_check(false)
        .allow_missing_glyphs()
        .wgpu()
        .build_eframe(|cc| App::new(cc, vec![PathBuf::from(path)]));
    let only = std::env::var("WT_SNAP_ONLY").unwrap_or_default();
    let want = |n: &str| only.is_empty() || only.split(',').any(|o| n.starts_with(o));

    h.state_mut().s.theme = "Porcelain".into();
    h.run_steps(2);
    if want("scanning") {
        shot(&mut h, "scanning");
    }
    let t0 = Instant::now();
    while h.state().scan.is_some() || h.state().tree.is_none() {
        h.step();
        std::thread::sleep(Duration::from_millis(20));
        assert!(t0.elapsed() < Duration::from_secs(120), "scan timed out");
    }
    h.run_steps(6);
    for theme in ["Porcelain", "Graphite", "Midnight", "Obsidian", "Nord", "Rosé Pine", "Sand", "Solarized", "Mint"] {
        let n = format!("tree_{}", theme.split(' ').next().unwrap().to_lowercase());
        if want(&n) {
            h.state_mut().s.theme = theme.into();
            shot(&mut h, &n);
        }
    }
    h.state_mut().s.theme = "Porcelain".into();
    for scheme in ["Classic", "Pastel", "Ocean", "Neon"] {
        let n = format!("scheme_{}", scheme.to_lowercase());
        if want(&n) {
            h.state_mut().s.treemap_scheme = scheme.into();
            h.state_mut().tm.dirty = true;
            shot(&mut h, &n);
        }
    }
    h.state_mut().s.treemap_scheme = "Vivid".into();
    h.state_mut().tm.dirty = true;
    if want("files") {
        h.state_mut().tab = Tab::Files;
        h.state_mut().fv.query = "*.so >100k".into();
        h.state_mut().fv.dirty = true;
        shot(&mut h, "files");
        h.state_mut().s.theme = "Midnight".into();
        shot(&mut h, "files_dark");
        h.state_mut().s.theme = "Porcelain".into();
    }
    if want("about") {
        h.state_mut().tab = Tab::About;
        shot(&mut h, "about");
    }
    h.state_mut().tab = Tab::Tree;
    if want("appearance") {
        h.state_mut().dlg.appearance = true;
        shot(&mut h, "appearance");
        h.state_mut().dlg.appearance = false;
    }
    // Interactions: expand a folder, hover the treemap, right-click a row.
    if want("interact") {
        let ctx = h.ctx.clone();
        h.state_mut().run(super::Cmd::ExpandAll, &ctx);
        shot(&mut h, "interact_expanded");
        h.hover_at(egui::pos2(500.0, 720.0));
        shot(&mut h, "interact_hover");
        click(&mut h, egui::pos2(200.0, 262.0), egui::PointerButton::Secondary);
        shot(&mut h, "interact_menu");
        h.key_press(egui::Key::Escape);
        click(&mut h, egui::pos2(160.0, 57.0), egui::PointerButton::Primary);
        shot(&mut h, "interact_source");
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
    }
    for (name, f) in [
        ("dlg_tweaks", (|a: &mut App| a.dlg.open_tweaks(&a.s.clone())) as fn(&mut App)),
        ("dlg_export", |a: &mut App| a.dlg.open_export(&a.s.clone(), false)),
        ("dlg_png", |a: &mut App| a.dlg.treemap_export = true),
        ("dlg_multi", |a: &mut App| {
            let v = a.volumes.clone();
            a.dlg.open_multi(&a.s.clone(), &v)
        }),
        ("dlg_language", |a: &mut App| a.dlg.language = true),
        ("dlg_ntfs", |a: &mut App| {
            a.dlg.ntfs = Some(super::dialogs::NtfsAccess { dev: "/dev/sda1".into(), password: String::new(), remember: false, error: None })
        }),
    ] {
        if want(name) {
            f(h.state_mut());
            shot(&mut h, name);
            h.state_mut().dlg = Default::default();
            h.run_steps(2);
        }
    }
    if want("filter") {
        let ctx = h.ctx.clone();
        h.state_mut().run(super::Cmd::ToggleFilterPanel, &ctx);
        shot(&mut h, "filter");
    }
}

/// Timing of the operations that must feel instant:
/// `WT_SNAP_PATH=~ cargo test --release bench_views -- --ignored --nocapture`
#[test]
#[ignore]
fn bench_views() {
    use crate::filter::Matcher;
    use crate::treemap;
    let path = std::env::var("WT_SNAP_PATH").unwrap_or_else(|_| "/usr".into());
    let t0 = Instant::now();
    let tree = crate::scan::scan(&[PathBuf::from(path)], &Default::default(), Default::default()).tree;
    println!("scan: {} nodes in {:?}", tree.nodes.len(), t0.elapsed());

    for q in ["a", "*.jpg", ">10m", "/cache/ *.json", "\\\\.*_v\\d+\\\\"] {
        for full in [false, true] {
            let t = Instant::now();
            let m = Matcher::new(q, full);
            let (mut buf, mut stack) = (Vec::new(), Vec::new());
            let mut n = 0usize;
            for &id in &tree.files_by_size {
                let p = if m.needs_path {
                    tree.path_lower_into(id, &mut buf, &mut stack);
                    Some(buf.as_slice())
                } else {
                    None
                };
                if m.matches(&tree, id, p) {
                    n += 1;
                }
            }
            println!("search {q:?} full={full}: {n} hits in {:?}", t.elapsed());
        }
    }
    for (w, h) in [(1400u32, 300u32), (2800, 600)] {
        let t = Instant::now();
        let p = treemap::Params::default();
        let layout = treemap::layout(&tree, &tree.roots, w, h, &p);
        let tl = t.elapsed();
        let ext = vec![[100u8, 150, 200]; tree.exts.names.len()];
        let c = treemap::Colors {
            ext_rgb: &ext,
            default_rgb: [72, 72, 72],
            folder_rgb: [72, 72, 72],
            free_rgb: [150, 150, 150],
            gray: false,
            gradient: true,
            background: [0, 0, 0],
            gain: 1.0,
            highlight_ext: None,
        };
        let _ = treemap::render(&tree, &layout, &c);
        println!("treemap {w}x{h}: {} items, layout {tl:?}, total {:?}", layout.items.len(), t.elapsed());
    }
    let t = Instant::now();
    let f = super::tree_view::filter_tree(&tree, "*.json", "", false);
    println!("filter_tree *.json: {} nodes in {:?}", f.nodes.len(), t.elapsed());
    let t = Instant::now();
    let d = super::file_view::compute_dups(&tree, crate::config::DupMode::NameSize);
    let _ = &d;
    println!("dups name+size in {:?}", t.elapsed());
}

/// Regression: a short window used to call `f32::clamp(120, h - 140)` with
/// min > max in the tree view, aborting the app during a scan.
#[test]
#[ignore]
fn tiny_window() {
    let path = std::env::var("WT_SNAP_PATH").unwrap_or_else(|_| "/usr".into());
    for size in [[420.0, 170.0], [760.0, 300.0]] {
        let mut h = Harness::builder()
            .with_size(size)
            .with_pixels_per_point(1.0)
            .with_max_steps(100_000)
            .with_accessibility_check(false)
            .allow_missing_glyphs()
            .wgpu()
            .build_eframe(|cc| App::new(cc, vec![PathBuf::from(&path)]));
        let t0 = Instant::now();
        while h.state().scan.is_some() || h.state().tree.is_none() {
            h.step();
            std::thread::sleep(Duration::from_millis(20));
            assert!(t0.elapsed() < Duration::from_secs(120), "scan timed out");
        }
        h.run_steps(4);
    }
}
