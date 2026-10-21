#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod actions;
mod config;
mod csvio;
mod filetypes;
mod filter;
mod format;
mod i18n;
mod model;
mod ntfs;
mod scan;
mod theme;
mod treemap;
mod ui;
mod volumes;

use std::path::PathBuf;
use std::sync::Arc;

/// Command line, in WizTree's syntax: `diskmosaic <path> /export="out.csv"
/// /exportfolders=0 ...`. `--name=value`, `-name=value` and `--name value`
/// are accepted too. Option names are case-insensitive.
#[derive(Default)]
struct Cli {
    paths: Vec<PathBuf>,
    opts: Vec<(String, String)>,
}

impl Cli {
    fn parse(args: impl Iterator<Item = String>) -> Cli {
        let mut cli = Cli::default();
        let args: Vec<String> = args.collect();
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            let body = a.strip_prefix("--").or_else(|| a.strip_prefix('/').filter(|b| is_opt_name(b))).or_else(|| a.strip_prefix('-'));
            match body {
                Some(b) if !b.is_empty() && !std::path::Path::new(a).exists() => {
                    let (k, v) = match b.split_once('=') {
                        Some((k, v)) => (k.to_string(), v.to_string()),
                        None if a.starts_with("--")
                            && i + 1 < args.len()
                            && !args[i + 1].starts_with('-')
                            && !args[i + 1].starts_with('/') =>
                        {
                            i += 1;
                            (b.to_string(), args[i].clone())
                        }
                        None => (b.to_string(), "1".to_string()),
                    };
                    cli.opts.push((k.to_lowercase(), v.trim_matches('"').to_string()));
                }
                _ => cli.paths.push(PathBuf::from(a.trim_matches('"'))),
            }
            i += 1;
        }
        cli
    }

    fn get(&self, k: &str) -> Option<&str> {
        self.opts.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    fn flag(&self, k: &str, default: bool) -> bool {
        self.get(k).map(|v| !matches!(v, "0" | "false" | "no" | "off")).unwrap_or(default)
    }

    fn num(&self, k: &str, default: u64) -> u64 {
        self.get(k).and_then(|v| v.parse().ok()).unwrap_or(default)
    }
}

/// A leading '/' introduces an option only for known option names, so that
/// absolute Unix paths still work as scan targets.
fn is_opt_name(b: &str) -> bool {
    let k = b.split('=').next().unwrap_or("").to_lowercase();
    k.starts_with("export")
        || k.starts_with("filter")
        || k.starts_with("treemap")
        || k.starts_with("sortby")
        || matches!(k.as_str(), "admin" | "nomft" | "threads" | "help" | "?" | "version" | "scan")
}

const HELP: &str = "\
DiskMosaic - fast disk space analyser

Usage: diskmosaic [PATH...] [OPTIONS]

Without export options the GUI starts and scans PATH (or shows the picker).
Options take the form /name=value (WizTree style) or --name=value:

  /export=FILE               Export the scan to CSV (TSV if FILE ends in .tsv)
  /exportfiletypes=1         Export the file type summary instead of files
  /exportfolders=0|1         Include folders (default 1)
  /exportfiles=0|1           Include files (default 1)
  /exportmaxdepth=N          Limit folder depth (0 = unlimited)
  /exportlimit=N             Limit the number of rows
  /exportalldates=1          Add last access and created dates
  /exportallsizes=1          Add folder-only size columns
  /exportsplitfilename=1     Add root/folder/name/extension columns
  /exportpercentofparent=1   Add a percent-of-parent column
  /exportdrivecapacity=0|1   Add drive capacity columns (default 1)
  /exportmftrecno=1          Add file id / parent file id columns
  /exportUTCTime=1           Write dates in UTC
  /sortby=0|1|2              Sort by size, name or modified date
  /sortbyfiletypes=0..3      File types: size, extension, count, allocated
  /filter=TEXT               Only export items matching the search syntax
  /filterexclude=TEXT        Exclude items matching the search syntax
  /filterfullpath=1          Match the filter against full paths
  /treemapimagefile=FILE     Save a treemap PNG
  /treemapimagewidth=N       Image width (default 1920)
  /treemapimageheight=N      Image height (default 1080)
  /treemapimagegray=1        Grayscale image
  /treemapimagefreespace=1   Include free space
  /treemapimageshowallocated=1  Size blocks by allocated space
  /treemapcolorbysize=0|1    Colour the 12 largest types (default 1)
  /admin=1                   Restart with elevated privileges
  /nomft=1                   Walk folders even on NTFS (skip the MFT fast path)
  /import=FILE.csv           Open a previously exported CSV in the GUI
";

/// Append panics to `crash.log` next to the settings file, keeping the usual
/// message on stderr too. A GUI run started from a menu has no terminal, so
/// without this a crash leaves nothing to read.
fn install_crash_log() {
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        let file = config::settings_path().with_file_name("crash.log");
        let at = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());
        let text = format!(
            "--- {} v{} ---\n{info}\nat {at}\n{}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            env!("CARGO_PKG_VERSION"),
            std::backtrace::Backtrace::force_capture(),
        );
        if let Some(d) = file.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
            let _ = f.write_all(text.as_bytes());
        }
        let _ = std::io::stderr().write_all(text.as_bytes());
    }));
}

fn main() {
    install_crash_log();
    // Rust ignores SIGPIPE, so a closed pipe turns into a panic ("failed
    // printing to stderr") and aborts. Standard tools just die quietly.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    // Privileged helper modes (run through sudo / pkexec by the app itself).
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("--mft-dump") {
        std::process::exit(helper_mft_dump(raw.get(1).map(String::as_str).unwrap_or("")));
    }
    if raw.first().map(String::as_str) == Some("--install-ntfs-access") {
        std::process::exit(helper_install_ntfs_access());
    }
    let cli = Cli::parse(std::env::args().skip(1));
    if cli.get("help").is_some() || cli.get("?").is_some() || cli.get("h").is_some() {
        print!("{HELP}");
        return;
    }
    if cli.get("version").is_some() {
        println!("{} {}", csvio::APP_NAME, csvio::APP_VERSION);
        return;
    }
    if cli.flag("admin", false) && !actions::is_elevated() {
        let rest: Vec<String> = std::env::args().skip(1).filter(|a| !a.to_lowercase().contains("admin")).collect();
        if actions::restart_elevated(&rest).is_ok() {
            return;
        }
    }

    if cli.get("export").is_some() || cli.get("treemapimagefile").is_some() {
        std::process::exit(headless(&cli));
    }

    let mut start = cli.paths.clone();
    if let Some(f) = cli.get("import") {
        start = vec![PathBuf::from(f)];
    }
    run_gui(start);
}

fn run_gui(start: Vec<PathBuf>) {
    let settings = config::Settings::load();
    if !settings.language.is_empty() {
        i18n::load(&settings.language);
    }
    let mut vp = egui::ViewportBuilder::default()
        .with_title(csvio::APP_NAME)
        .with_app_id("diskmosaic")
        .with_inner_size([settings.window_w.max(960.0), settings.window_h.max(600.0)])
        .with_min_inner_size([760.0, 480.0])
        .with_icon(Arc::new(app_icon(128)))
        .with_drag_and_drop(true);
    if settings.window_maximized {
        vp = vp.with_maximized(true);
    }
    let opts = eframe::NativeOptions { viewport: vp, centered: true, ..Default::default() };
    if let Err(e) = eframe::run_native(csvio::APP_NAME, opts, Box::new(move |cc| Ok(Box::new(ui::App::new(cc, start))))) {
        eprintln!("failed to start the user interface: {e}");
        std::process::exit(1);
    }
}

use eframe::egui;

/// Procedural app icon: a rounded tile holding a small cushion treemap.
fn app_icon(n: u32) -> egui::IconData {
    let mut rgba = vec![0u8; (n * n * 4) as usize];
    let s = n as f32;
    let radius = s * 0.22;
    let blocks: [(f32, f32, f32, f32, [u8; 3]); 5] = [
        (0.14, 0.14, 0.44, 0.72, [0x0A, 0x84, 0xFF]),
        (0.60, 0.14, 0.26, 0.38, [0xFF, 0x9F, 0x0A]),
        (0.60, 0.54, 0.26, 0.32, [0x30, 0xD1, 0x58]),
        (0.14, 0.88, 0.0, 0.0, [0, 0, 0]),
        (0.0, 0.0, 0.0, 0.0, [0, 0, 0]),
    ];
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded-square mask with 1px anti-aliasing.
            let dx = (radius - fx).max(fx - (s - radius)).max(0.0);
            let dy = (radius - fy).max(fy - (s - radius)).max(0.0);
            let d = (dx * dx + dy * dy).sqrt() - radius;
            let a = (0.5 - d).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let t = fy / s;
            let mut c = [(0x26 as f32 - 10.0 * t), (0x28 as f32 - 10.0 * t), (0x30 as f32 - 10.0 * t)];
            let (u, v) = (fx / s, fy / s);
            for &(bx, by, bw, bh, col) in &blocks {
                if bw > 0.0 && u >= bx && u < bx + bw && v >= by && v < by + bh {
                    // Cushion shading: brighter towards the upper left.
                    let px = (u - bx) / bw;
                    let py = (v - by) / bh;
                    let sh = 0.72 + 0.5 * (1.0 - (2.0 * px - 0.7).powi(2) * 0.5 - (2.0 * py - 0.7).powi(2) * 0.5).max(0.0);
                    c = [col[0] as f32 * sh, col[1] as f32 * sh, col[2] as f32 * sh];
                }
            }
            let i = ((y * n + x) * 4) as usize;
            rgba[i] = c[0].clamp(0.0, 255.0) as u8;
            rgba[i + 1] = c[1].clamp(0.0, 255.0) as u8;
            rgba[i + 2] = c[2].clamp(0.0, 255.0) as u8;
            rgba[i + 3] = (a * 255.0) as u8;
        }
    }
    egui::IconData { rgba, width: n, height: n }
}

/// `--mft-dump DEV`: stream the raw MFT of an NTFS block device to stdout.
/// This is the only code that runs elevated; it reads nothing but the MFT.
fn helper_mft_dump(dev: &str) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let ok = std::fs::metadata(dev).map(|m| m.file_type().is_block_device()).unwrap_or(false);
        if !ok {
            eprintln!("{dev}: not a block device");
            return 2;
        }
    }
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::with_capacity(8 << 20, stdout.lock());
    match ntfs::dump_mft(std::path::Path::new(dev), &mut out) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{dev}: {e}");
            3
        }
    }
}

pub const NTFS_RULE_PATH: &str = "/etc/udev/rules.d/70-diskmosaic-ntfs.rules";

/// `--install-ntfs-access`: let the logged-in desktop user read NTFS block
/// devices (systemd `uaccess` ACL), so fast scans need no password later.
fn helper_install_ntfs_access() -> i32 {
    let rule = "# Installed by DiskMosaic: lets the active local user read NTFS partitions\n\
                # so the MFT can be scanned directly. Delete this file to revoke.\n\
                SUBSYSTEM==\"block\", ENV{ID_FS_TYPE}==\"ntfs\", TAG+=\"uaccess\"\n";
    if let Err(e) = std::fs::write(NTFS_RULE_PATH, rule) {
        eprintln!("{NTFS_RULE_PATH}: {e}");
        return 3;
    }
    let ok = std::process::Command::new("udevadm").args(["control", "--reload"]).status().is_ok_and(|s| s.success())
        && std::process::Command::new("udevadm")
            .args(["trigger", "--subsystem-match=block", "--action=change"])
            .status()
            .is_ok_and(|s| s.success());
    let _ = std::process::Command::new("udevadm").arg("settle").status();
    if ok { 0 } else { 4 }
}

/// Return memory freed after a scan (transient directory listings) to the
/// OS, so the resident size reflects only the in-memory index.
pub fn release_memory() {
    #[cfg(feature = "mimalloc")]
    {
        unsafe extern "C" {
            fn mi_collect(force: bool);
        }
        // SAFETY: mi_collect is thread-safe and only releases unused pages.
        unsafe { mi_collect(true) };
    }
}

/// Scan and export without a window. Returns the process exit code.
fn headless(cli: &Cli) -> i32 {
    let roots: Vec<PathBuf> = if cli.paths.is_empty() {
        vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
    } else {
        cli.paths.clone()
    };
    let settings = config::Settings::load();
    model::MAX_MTIME.store(settings.max_modified_date, std::sync::atomic::Ordering::Relaxed);
    let started = std::time::Instant::now();
    let mut tree = if roots.len() == 1 && roots[0].extension().is_some_and(|e| e.eq_ignore_ascii_case("csv") || e.eq_ignore_ascii_case("tsv")) && roots[0].is_file() {
        match csvio::import(&roots[0], None) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("cannot read {}: {e}", roots[0].display());
                return 2;
            }
        }
    } else {
        let opts = scan::ScanOptions {
            one_filesystem: settings.one_filesystem,
            mft_for_folders: settings.mft_for_folders,
            use_mft: !cli.flag("nomft", false),
            threads: cli.num("threads", 0) as usize,
            elevation: if std::io::IsTerminal::is_terminal(&std::io::stdin()) { scan::Elevation::SudoTty } else { scan::Elevation::None },
            ..Default::default()
        };
        let res = scan::scan(&roots, &opts, Arc::new(scan::Progress::default()));
        if let Some(n) = &res.note {
            eprintln!("{n}");
        }
        if res.cancelled {
            return 1;
        }
        if res.used_mft {
            eprintln!("Read the NTFS MFT directly");
        }
        res.tree
    };
    eprintln!("Scanned {} items in {:.2}s", format::thousands(tree.nodes.len() as u64), started.elapsed().as_secs_f64());
    release_memory();

    let inc = cli.get("filter").unwrap_or("");
    let exc = cli.get("filterexclude").unwrap_or("");
    if !inc.is_empty() || !exc.is_empty() {
        tree = ui::tree_view::filter_tree(&tree, inc, exc, cli.flag("filterfullpath", false));
    }

    let mut code = 0;
    if let Some(out) = cli.get("export") {
        let out = PathBuf::from(out);
        let tsv = out.extension().is_some_and(|e| e.eq_ignore_ascii_case("tsv") || e.eq_ignore_ascii_case("txt"));
        let res = if cli.flag("exportfiletypes", false) {
            csvio::export_file_types(&tree, &out, tsv, cli.num("sortbyfiletypes", 0) as u8).map(|_| tree.ext_stats.len())
        } else {
            let o = csvio::ExportOptions {
                dates: cli.flag("exportalldates", false),
                folder_sizes: cli.flag("exportallsizes", false),
                split_names: cli.flag("exportsplitfilename", false),
                drive_size: cli.flag("exportdrivecapacity", true),
                percent_of_parent: cli.flag("exportpercentofparent", false),
                max_depth: cli.num("exportmaxdepth", 0) as u32,
                file_ids: cli.flag("exportmftrecno", false) || cli.flag("exportfileid", false),
                files: cli.flag("exportfiles", true),
                folders: cli.flag("exportfolders", true),
                utc: cli.flag("exportutctime", false),
                tsv,
                sort_by: cli.num("sortby", 0) as u8,
                limit: cli.num("exportlimit", 0) as usize,
            };
            let roots = tree.roots.clone();
            csvio::export(&tree, &roots, true, &o, &out, None)
        };
        match res {
            Ok(n) => eprintln!("Exported {} rows to {}", format::thousands(n as u64), out.display()),
            Err(e) => {
                eprintln!("export to {} failed: {e}", out.display());
                code = 3;
            }
        }
    }

    if let Some(out) = cli.get("treemapimagefile") {
        let (w, h) = (cli.num("treemapimagewidth", 1920).clamp(16, 16384) as u32, cli.num("treemapimageheight", 1080).clamp(16, 16384) as u32);
        let p = treemap::Params {
            use_alloc: cli.flag("treemapimageshowallocated", false),
            show_free: cli.flag("treemapimagefreespace", false),
            free_bytes: tree.volume.free,
            ..Default::default()
        };
        let scheme = theme::scheme("Classic");
        let other = scheme.colors[12];
        let mut ext = vec![other; tree.exts.names.len()];
        if cli.flag("treemapcolorbysize", true) {
            for (rank, st) in tree.ext_stats.iter().take(12).enumerate() {
                ext[st.ext as usize] = scheme.colors[rank];
            }
        } else {
            // Stable colour per extension, independent of size ranking.
            for (i, c) in ext.iter_mut().enumerate().skip(1) {
                *c = scheme.colors[i % 12];
            }
        }
        let c = treemap::Colors {
            ext_rgb: &ext,
            default_rgb: other,
            folder_rgb: other,
            free_rgb: [0x9C, 0x9C, 0xA4],
            gray: cli.flag("treemapimagegray", false),
            gradient: true,
            background: [0, 0, 0],
            gain: scheme.gain,
            highlight_ext: None,
        };
        let layout = treemap::layout(&tree, &tree.roots, w, h, &p);
        let rgba = treemap::render(&tree, &layout, &c);
        match ui::dialogs::write_png(std::path::Path::new(out), w, h, &rgba) {
            Ok(()) => eprintln!("Saved {w}x{h} treemap to {out}"),
            Err(e) => {
                eprintln!("saving {out} failed: {e}");
                code = 3;
            }
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parsing() {
        let a = ["/home", "/export=\"out.csv\"", "/exportfolders=0", "--sortby", "1", "-filter=*.txt", "/tmp"].map(String::from);
        let c = Cli::parse(a.into_iter());
        assert_eq!(c.paths, vec![PathBuf::from("/home"), PathBuf::from("/tmp")]);
        assert_eq!(c.get("export"), Some("out.csv"));
        assert!(!c.flag("exportfolders", true));
        assert_eq!(c.num("sortby", 0), 1);
        assert_eq!(c.get("filter"), Some("*.txt"));
    }
}
