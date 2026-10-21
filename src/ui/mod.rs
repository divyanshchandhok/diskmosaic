//! The desktop application: state, commands and the main frame.

mod chrome;
pub mod dialogs;
mod file_view;
pub mod grid;
pub mod icons;
mod monitor;
pub mod tree_view;
mod treemap_view;

use crate::config::{ColorBy, Settings};
use crate::model::{NodeId, Tree};
use crate::scan::{self, Progress, ScanOptions, ScanResult};
use crate::theme::{self, Palette};
use crate::volumes::{self, VolumeInfo};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Volume(PathBuf),
    Folder(PathBuf),
    Multi(Vec<PathBuf>),
    Csv(PathBuf),
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Source::Volume(p) | Source::Folder(p) => p.display().to_string(),
            Source::Multi(v) => format!("{} locations", v.len()),
            Source::Csv(p) => p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        }
    }
    pub fn paths(&self) -> Vec<PathBuf> {
        match self {
            Source::Volume(p) | Source::Folder(p) | Source::Csv(p) => vec![p.clone()],
            Source::Multi(v) => v.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Tree,
    Files,
    About,
}

/// Every user action, shared by menus, shortcuts, toolbar and context menus.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Scan,
    Cancel,
    SelectFolder,
    SelectMultiple,
    SelectCsv,
    NewInstance,
    ResetTreeExpansion,
    ExpandAll,
    SelectParent,
    ReselectChild,
    ZoomTreemap,
    UnzoomTreemap,
    ZoomOutOne,
    SaveTreemapImage,
    Open,
    Reveal,
    Terminal,
    CopyPath,
    Copy,
    Cut,
    DeleteToTrash,
    DeletePermanently,
    Rename,
    Properties,
    ExportCsv,
    CopyVisibleTree,
    CopyFileInfo,
    ExportFileTypes,
    ResetConfig,
    RestartAdmin,
    ToggleFilterPanel,
    ApplyFilter,
    ClearFilter,
    ToggleHeader,
    ToggleTreemap,
    ToggleFileTypes,
    ToggleTreemapAllocated,
    ToggleTreemapFree,
    ToggleTreemapNames,
    Appearance,
    Tweaks,
    Language,
    About,
    Exit,
    SelectExtension(u16),
    FocusSearch,
}

pub struct ScanJob {
    pub progress: Arc<Progress>,
    pub rx: crossbeam_channel::Receiver<Result<ScanResult, String>>,
    pub started: Instant,
    pub source: Source,
}

pub struct Msg {
    pub title: String,
    pub body: String,
    pub error: bool,
}

pub enum Confirm {
    Delete { ids: Vec<NodeId>, permanent: bool },
    OpenMany { ids: Vec<NodeId>, what: Cmd },
    ResetConfig,
}

pub struct App {
    pub s: Settings,
    pub pal: Palette,
    style_key: String,
    pub volumes: Vec<VolumeInfo>,
    pub source: Option<Source>,
    pub tree: Option<Tree>,
    /// Unfiltered tree while a scan-result filter is applied.
    pub base_tree: Option<Tree>,
    pub scan: Option<ScanJob>,
    pub status: String,
    pub last_elapsed: Option<f64>,
    pub tab: Tab,

    // Tree view
    pub expanded: HashSet<NodeId>,
    pub expanded_groups: HashSet<NodeId>,
    pub rows: Vec<tree_view::Row>,
    pub rows_dirty: bool,
    pub sel: Vec<NodeId>,
    pub anchor: Option<usize>,
    pub reselect_stack: Vec<NodeId>,
    pub tree_sort: (usize, bool),
    pub scroll_tree_to: Option<NodeId>,
    pub tree_cols: Vec<grid::Col>,

    // Extensions panel
    pub ext_sort: (usize, bool),
    pub ext_cols: Vec<grid::Col>,
    pub ext_sel: Option<u16>,
    pub ext_rows: Vec<usize>,

    // Treemap
    pub tm: treemap_view::TreemapState,
    pub hover_path: String,

    // File view
    pub fv: file_view::FileViewState,

    // Filter panel
    pub filter_include: String,
    pub filter_exclude: String,

    // Dialogs
    pub dlg: dialogs::Dialogs,
    pub msg: Option<Msg>,
    pub confirm: Option<Confirm>,
    pub rename: Option<(NodeId, String)>,
    pub properties: Option<NodeId>,
    pub denied_dismissed: bool,
    pub admin_dismissed: bool,

    pub monitor: Option<monitor::Monitor>,
    pub exit_requested: bool,
    pub start_args: Vec<PathBuf>,
    pub last_save: Instant,
    pub focus_search: bool,
    /// Elevation for the NTFS MFT helper, kept for the session.
    pub elevation: crate::scan::Elevation,
    /// The user chose the normal folder walk for NTFS this session.
    pub ntfs_declined: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, start: Vec<PathBuf>) -> App {
        theme::install_fonts(&cc.egui_ctx);
        let s = Settings::load();
        crate::model::MAX_MTIME.store(s.max_modified_date, Ordering::Relaxed);
        let system_dark = cc.egui_ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(true);
        let pal = theme::resolve(&s.theme, &s.accent, system_dark);
        theme::apply(&cc.egui_ctx, &pal);
        let volumes = volumes::list_volumes();
        let mut app = App {
            tree_cols: tree_view::default_cols(&s),
            ext_cols: tree_view::default_ext_cols(),
            fv: file_view::FileViewState::new(&s),
            filter_include: s.include_filter.clone(),
            filter_exclude: s.exclude_filter.clone(),
            s,
            pal,
            style_key: String::new(),
            volumes,
            source: None,
            tree: None,
            base_tree: None,
            scan: None,
            status: "Ready".into(),
            last_elapsed: None,
            tab: Tab::Tree,
            expanded: HashSet::new(),
            expanded_groups: HashSet::new(),
            rows: Vec::new(),
            rows_dirty: true,
            sel: Vec::new(),
            anchor: None,
            reselect_stack: Vec::new(),
            tree_sort: (2, true),
            scroll_tree_to: None,
            ext_sort: (4, true),
            ext_sel: None,
            ext_rows: Vec::new(),
            tm: treemap_view::TreemapState::default(),
            hover_path: String::new(),
            dlg: dialogs::Dialogs::default(),
            msg: None,
            confirm: None,
            rename: None,
            properties: None,
            denied_dismissed: false,
            admin_dismissed: false,
            monitor: None,
            exit_requested: false,
            start_args: start,
            last_save: Instant::now(),
            focus_search: false,
            elevation: crate::scan::Elevation::None,
            ntfs_declined: false,
        };
        // Initial source: command line, else last selection, else boot volume.
        if !app.start_args.is_empty() {
            let args = std::mem::take(&mut app.start_args);
            let src = if args.len() > 1 {
                Source::Multi(args)
            } else if args[0].extension().map(|e| e.eq_ignore_ascii_case("csv") || e.eq_ignore_ascii_case("tsv")).unwrap_or(false) {
                Source::Csv(args[0].clone())
            } else if app.volumes.iter().any(|v| v.path == args[0]) {
                Source::Volume(args[0].clone())
            } else {
                Source::Folder(args[0].clone())
            };
            app.source = Some(src);
            app.start_scan();
        } else if let Some(v) = app.volumes.iter().find(|v| v.path.display().to_string() == app.s.last_selection) {
            app.source = Some(Source::Volume(v.path.clone()));
        } else if !app.s.last_selection.is_empty() && std::path::Path::new(&app.s.last_selection).is_dir() {
            app.source = Some(Source::Folder(PathBuf::from(&app.s.last_selection)));
        } else if let Some(v) = app.volumes.iter().find(|v| v.is_boot).or(app.volumes.first()) {
            app.source = Some(Source::Volume(v.path.clone()));
        }
        app
    }

    pub fn fmt(&self) -> crate::format::Fmt {
        self.s.fmt()
    }

    // ----------------------------------------------------------------- scanning

    pub fn set_source(&mut self, src: Source) {
        if let Source::Volume(p) | Source::Folder(p) = &src {
            self.s.last_selection = p.display().to_string();
        }
        self.source = Some(src);
        if self.s.auto_scan {
            self.start_scan();
        }
    }

    pub fn start_scan(&mut self) {
        let Some(src) = self.source.clone() else { return };
        #[cfg(target_os = "linux")]
        if let (Source::Volume(p) | Source::Folder(p), crate::scan::Elevation::None, false) = (&src, &self.elevation, self.ntfs_declined) {
            if let Some(t) = crate::ntfs::linux_target(p) {
                if !crate::ntfs::can_read_directly(&t.dev) {
                    self.dlg.ntfs = Some(dialogs::NtfsAccess { dev: t.dev, password: String::new(), remember: false, error: None });
                    return;
                }
            }
        }
        if let Some(j) = &self.scan {
            j.progress.cancel.store(true, Ordering::Relaxed);
        }
        self.monitor = None;
        let progress = Arc::new(Progress::default());
        let (tx, rx) = crossbeam_channel::bounded(1);
        let opts = ScanOptions {
            one_filesystem: self.s.one_filesystem,
            use_mft: true,
            mft_for_folders: self.s.mft_for_folders,
            threads: 0,
            elevation: self.elevation.clone(),
        };
        crate::model::MAX_MTIME.store(self.s.max_modified_date, Ordering::Relaxed);
        let p2 = progress.clone();
        let src2 = src.clone();
        std::thread::Builder::new()
            .name("scan".into())
            .spawn(move || {
                let res = match &src2 {
                    Source::Csv(path) => {
                        let start = Instant::now();
                        crate::csvio::import(path, Some(&p2))
                            .map(|tree| ScanResult { tree, elapsed: start.elapsed().as_secs_f64(), cancelled: false, used_mft: false, note: None })
                            .map_err(|e| format!("Could not import {}: {e}", path.display()))
                    }
                    other => Ok(scan::scan(&other.paths(), &opts, p2)),
                };
                let _ = tx.send(res);
            })
            .expect("spawn scan thread");
        self.status = "Scanning…".into();
        self.scan = Some(ScanJob { progress, rx, started: Instant::now(), source: src });
    }

    pub fn cancel_scan(&mut self) {
        if let Some(j) = &self.scan {
            j.progress.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn poll_scan(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.scan else { return };
        match job.rx.try_recv() {
            Ok(Ok(res)) => {
                let src = job.source.clone();
                self.scan = None;
                if res.cancelled && res.note.as_deref() == Some("authentication failed") {
                    self.elevation = crate::scan::Elevation::None;
                    #[cfg(target_os = "linux")]
                    if let Source::Volume(p) | Source::Folder(p) = &src {
                        if let Some(t) = crate::ntfs::linux_target(p) {
                            self.dlg.ntfs = Some(dialogs::NtfsAccess {
                                dev: t.dev,
                                password: String::new(),
                                remember: false,
                                error: Some("Incorrect password, try again.".into()),
                            });
                        }
                    }
                    self.status = "Ready".into();
                    return;
                }
                self.last_elapsed = Some(res.elapsed);
                self.status = if res.cancelled {
                    format!("Scan canceled after {:.2} seconds", res.elapsed)
                } else {
                    format!("Scan complete in {:.2} seconds{}", res.elapsed, if res.used_mft { " (MFT)" } else { "" })
                };
                if let Some(n) = &res.note {
                    self.status = format!("{} · {n}", self.status);
                }
                self.install_tree(res.tree);
                if self.s.monitor_changes && !matches!(src, Source::Csv(_)) && !res.cancelled {
                    if let Some(t) = &self.tree {
                        let roots: Vec<PathBuf> = t.roots.iter().map(|&r| PathBuf::from(t.path(r))).collect();
                        self.monitor = monitor::Monitor::start(&roots, ctx.clone());
                    }
                }
            }
            Ok(Err(e)) => {
                self.scan = None;
                self.status = "Scan failed".into();
                self.msg = Some(Msg { title: "Scan Failed".into(), body: e, error: true });
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(60));
            }
            Err(_) => {
                self.scan = None;
            }
        }
    }

    pub fn install_tree(&mut self, tree: Tree) {
        self.base_tree = None;
        self.tree = Some(tree);
        self.reset_views();
        if !self.s.include_filter.trim().is_empty() || !self.s.exclude_filter.trim().is_empty() {
            if self.s.show_filter_panel {
                self.apply_filter();
            }
        }
        self.denied_dismissed = false;
        crate::release_memory();
    }

    pub fn reset_views(&mut self) {
        self.expanded.clear();
        self.expanded_groups.clear();
        self.sel.clear();
        self.reselect_stack.clear();
        self.ext_sel = None;
        self.tm.zoom = None;
        self.tm.dirty = true;
        self.rows_dirty = true;
        self.fv.dirty = true;
        self.fv.dups = None;
        self.ext_rows.clear();
        if let Some(t) = &self.tree {
            for &r in &t.roots {
                self.expanded.insert(r);
            }
            if let Some(&r) = t.roots.first() {
                self.sel = vec![r];
            }
        }
    }

    /// Called after any in-place modification of the tree (delete, rename, monitor).
    pub fn tree_changed(&mut self) {
        self.rows_dirty = true;
        self.tm.dirty = true;
        self.fv.dirty = true;
        self.ext_rows.clear();
        if let Some(t) = &self.tree {
            self.sel.retain(|&id| !t.nodes[id as usize].is_deleted());
            if let Some(z) = self.tm.zoom {
                if t.nodes[z as usize].is_deleted() {
                    self.tm.zoom = None;
                }
            }
        }
    }

    // ----------------------------------------------------------------- filter

    pub fn apply_filter(&mut self) {
        let inc = self.filter_include.trim().to_string();
        let exc = self.filter_exclude.trim().to_string();
        Settings::push_history(&mut self.s.include_history, &inc);
        Settings::push_history(&mut self.s.exclude_history, &exc);
        self.s.include_filter = inc.clone();
        self.s.exclude_filter = exc.clone();
        let base = match self.base_tree.take() {
            Some(b) => b,
            None => match self.tree.take() {
                Some(t) => t,
                None => return,
            },
        };
        if inc.is_empty() && exc.is_empty() {
            self.tree = Some(base);
            self.reset_views();
            return;
        }
        let filtered = tree_view::filter_tree(&base, &inc, &exc, self.s.filter_match_path);
        self.base_tree = Some(base);
        self.tree = Some(filtered);
        self.reset_views();
    }

    pub fn clear_filter(&mut self) {
        self.filter_include.clear();
        self.filter_exclude.clear();
        self.s.include_filter.clear();
        self.s.exclude_filter.clear();
        if let Some(b) = self.base_tree.take() {
            self.tree = Some(b);
            self.reset_views();
        }
    }

    // ----------------------------------------------------------------- targets

    /// Items the current command applies to (selection in the active view).
    pub fn targets(&self) -> Vec<NodeId> {
        match self.tab {
            Tab::Files => {
                let mut v: Vec<NodeId> = self.fv.sel.iter().copied().collect();
                v.sort_unstable();
                v
            }
            _ => self.sel.clone(),
        }
    }

    pub fn path_of(&self, id: NodeId) -> PathBuf {
        PathBuf::from(self.tree.as_ref().map(|t| t.path(id)).unwrap_or_default())
    }

    pub fn select_node(&mut self, id: NodeId) {
        let Some(t) = &self.tree else { return };
        for a in t.chain(id).iter().take(t.chain(id).len().saturating_sub(1)) {
            self.expanded.insert(*a);
        }
        // Expand the file group that contains this file if needed.
        if !t.nodes[id as usize].is_dir() {
            let p = t.parent(id);
            if p != crate::model::NONE && tree_view::has_group(t, p, self.s.file_group_min) {
                self.expanded_groups.insert(p);
            }
        }
        self.sel = vec![id];
        self.rows_dirty = true;
        self.scroll_tree_to = Some(id);
    }

    // ----------------------------------------------------------------- commands

    pub fn run(&mut self, cmd: Cmd, ctx: &egui::Context) {
        match cmd {
            Cmd::Scan => self.start_scan(),
            Cmd::Cancel => self.cancel_scan(),
            Cmd::SelectFolder => {
                if let Some(p) = rfd::FileDialog::new().set_title("Select Folder").pick_folder() {
                    self.set_source(Source::Folder(p));
                }
            }
            Cmd::SelectMultiple => self.dlg.open_multi(&self.s, &self.volumes),
            Cmd::SelectCsv => {
                if let Some(p) = rfd::FileDialog::new()
                    .set_title("Select CSV file")
                    .add_filter("CSV file", &["csv", "tsv", "txt"])
                    .pick_file()
                {
                    self.set_source(Source::Csv(p));
                }
            }
            Cmd::NewInstance => crate::actions::new_instance(),
            Cmd::ResetTreeExpansion => {
                self.expanded.clear();
                self.expanded_groups.clear();
                if let Some(t) = &self.tree {
                    for &r in &t.roots {
                        self.expanded.insert(r);
                    }
                }
                self.rows_dirty = true;
            }
            Cmd::ExpandAll => {
                if let Some(t) = &self.tree {
                    let start: Vec<NodeId> = if self.sel.is_empty() { t.roots.clone() } else { self.sel.clone() };
                    let mut stack = start;
                    let mut n = 0;
                    while let Some(id) = stack.pop() {
                        if n > 200_000 {
                            break;
                        }
                        n += 1;
                        if t.nodes[id as usize].is_dir() {
                            self.expanded.insert(id);
                            self.expanded_groups.insert(id);
                            stack.extend(t.kids(id).iter().filter(|&&k| t.nodes[k as usize].is_dir()));
                        }
                    }
                }
                self.rows_dirty = true;
            }
            Cmd::SelectParent => {
                if let (Some(t), Some(&cur)) = (&self.tree, self.sel.first()) {
                    if !t.is_root(cur) {
                        let p = t.parent(cur);
                        self.reselect_stack.push(cur);
                        self.select_node(p);
                    }
                }
            }
            Cmd::ReselectChild => {
                if let Some(c) = self.reselect_stack.pop() {
                    self.select_node(c);
                }
            }
            Cmd::ZoomTreemap => {
                if let (Some(t), Some(&cur)) = (&self.tree, self.sel.first()) {
                    let z = if t.nodes[cur as usize].is_dir() { cur } else { t.parent(cur) };
                    if z != crate::model::NONE {
                        self.tm.zoom = Some(z);
                        self.tm.dirty = true;
                    }
                }
            }
            Cmd::UnzoomTreemap => {
                self.tm.zoom = None;
                self.tm.dirty = true;
            }
            Cmd::ZoomOutOne => {
                if let (Some(t), Some(z)) = (&self.tree, self.tm.zoom) {
                    self.tm.zoom = if t.is_root(z) { None } else { Some(t.parent(z)) };
                    self.tm.dirty = true;
                }
            }
            Cmd::SaveTreemapImage => self.dlg.treemap_export = true,
            Cmd::Open => self.open_targets(false),
            Cmd::Reveal => self.open_targets(true),
            Cmd::Terminal => {
                let ids = self.targets();
                if ids.len() > 5 {
                    self.confirm = Some(Confirm::OpenMany { ids, what: Cmd::Terminal });
                } else {
                    for id in ids {
                        let p = self.path_of(id);
                        if let Err(e) = crate::actions::terminal_here(&p) {
                            self.error("Couldn't open a terminal", &e.to_string());
                        }
                    }
                }
            }
            Cmd::CopyPath => {
                let paths: Vec<String> = self.targets().iter().map(|&i| self.path_of(i).display().to_string()).collect();
                if !paths.is_empty() {
                    ctx.copy_text(paths.join("\n"));
                }
            }
            Cmd::Copy | Cmd::Cut => {
                let paths: Vec<PathBuf> = self.targets().iter().map(|&i| self.path_of(i)).collect();
                if !paths.is_empty() {
                    if let Err(e) = crate::actions::clipboard_files(&paths, cmd == Cmd::Cut) {
                        self.error("Copy", &e);
                    }
                }
            }
            Cmd::DeleteToTrash | Cmd::DeletePermanently => {
                let ids: Vec<NodeId> = self
                    .targets()
                    .into_iter()
                    .filter(|&i| self.tree.as_ref().map(|t| !t.is_root(i)).unwrap_or(false))
                    .collect();
                if !ids.is_empty() {
                    self.confirm = Some(Confirm::Delete { ids, permanent: cmd == Cmd::DeletePermanently });
                }
            }
            Cmd::Rename => {
                if !self.s.enable_rename {
                    return;
                }
                if let (Some(t), Some(&id)) = (&self.tree, self.targets().first()) {
                    if !t.is_root(id) {
                        self.rename = Some((id, t.name(id).to_string()));
                    }
                }
            }
            Cmd::Properties => {
                if let Some(&id) = self.targets().first() {
                    let p = self.path_of(id);
                    if !crate::actions::properties(&p) {
                        self.properties = Some(id);
                    }
                }
            }
            Cmd::ExportCsv => self.dlg.open_export(&self.s, self.tab == Tab::Files),
            Cmd::CopyVisibleTree => {
                let text = tree_view::visible_tree_text(self);
                ctx.copy_text(text);
            }
            Cmd::CopyFileInfo => {
                let text = self.file_info_text();
                ctx.copy_text(text);
            }
            Cmd::ExportFileTypes => {
                if self.tree.is_some() {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_title("Export File Types to CSV file")
                        .add_filter("CSV file", &["csv"])
                        .add_filter("Tab Separated Values (.tsv)", &["tsv"])
                        .set_file_name(format!("DiskMosaic_FileTypes_{}.csv", chrono::Local::now().format("%Y%m%d%H%M%S")))
                        .save_file()
                    {
                        let tsv = p.extension().map(|e| e == "tsv").unwrap_or(false);
                        let r = crate::csvio::export_file_types(self.tree.as_ref().unwrap(), &p, tsv, 0);
                        match r {
                            Ok(()) => self.status = "Export complete.".into(),
                            Err(e) => self.error("Couldn't Export", &e.to_string()),
                        }
                    }
                }
            }
            Cmd::ResetConfig => self.confirm = Some(Confirm::ResetConfig),
            Cmd::RestartAdmin => {
                let args: Vec<String> = self.source.as_ref().map(|s| s.paths().iter().map(|p| p.display().to_string()).collect()).unwrap_or_default();
                match crate::actions::restart_elevated(&args) {
                    Ok(()) => {
                        self.s.save();
                        self.exit_requested = true;
                    }
                    Err(e) => self.error("Couldn't Restart", &e),
                }
            }
            Cmd::ToggleFilterPanel => self.s.show_filter_panel = !self.s.show_filter_panel,
            Cmd::ApplyFilter => self.apply_filter(),
            Cmd::ClearFilter => self.clear_filter(),
            Cmd::ToggleHeader => self.s.show_header = !self.s.show_header,
            Cmd::ToggleTreemap => self.s.show_treemap = !self.s.show_treemap,
            Cmd::ToggleFileTypes => self.s.show_file_types = !self.s.show_file_types,
            Cmd::ToggleTreemapAllocated => {
                self.s.treemap_allocated = !self.s.treemap_allocated;
                self.tm.dirty = true;
            }
            Cmd::ToggleTreemapFree => {
                self.s.treemap_free_space = !self.s.treemap_free_space;
                self.tm.dirty = true;
            }
            Cmd::ToggleTreemapNames => {
                self.s.treemap_names = !self.s.treemap_names;
                self.tm.dirty = true;
            }
            Cmd::Appearance => self.dlg.appearance = true,
            Cmd::Tweaks => self.dlg.open_tweaks(&self.s),
            Cmd::Language => self.dlg.language = true,
            Cmd::About => self.tab = Tab::About,
            Cmd::Exit => self.exit_requested = true,
            Cmd::SelectExtension(e) => {
                if let Some(t) = &self.tree {
                    let name = &t.exts.names[e as usize];
                    self.fv.query = if name.is_empty() { "!*.*".into() } else { format!("*.{name}") };
                    if name.is_empty() {
                        // Files without an extension: names containing no dot.
                        self.fv.query = "\\^[^.]+$\\".into();
                    }
                    self.fv.dirty = true;
                    self.tab = Tab::Files;
                }
            }
            Cmd::FocusSearch => {
                self.tab = Tab::Files;
                self.focus_search = true;
            }
        }
    }

    fn open_targets(&mut self, reveal: bool) {
        let ids = self.targets();
        if ids.len() > 5 {
            self.confirm = Some(Confirm::OpenMany { ids, what: if reveal { Cmd::Reveal } else { Cmd::Open } });
            return;
        }
        self.open_ids(&ids, reveal);
    }

    pub fn open_ids(&mut self, ids: &[NodeId], reveal: bool) {
        for &id in ids {
            let p = self.path_of(id);
            let r = if reveal { crate::actions::reveal(&p) } else { crate::actions::open_path(&p) };
            if let Err(e) = r {
                self.error("Couldn't open", &format!("{}: {e}", p.display()));
                break;
            }
        }
    }

    pub fn error(&mut self, title: &str, body: &str) {
        self.msg = Some(Msg { title: title.into(), body: body.into(), error: true });
    }

    pub fn file_info_text(&self) -> String {
        let Some(t) = &self.tree else { return String::new() };
        let f = self.fmt();
        let mut out = String::new();
        for id in self.targets() {
            let n = &t.nodes[id as usize];
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                t.path(id),
                f.size(n.size),
                f.size(n.alloc),
                crate::format::date(n.mtime)
            ));
        }
        out
    }

    pub fn delete_now(&mut self, ids: Vec<NodeId>, permanent: bool) {
        let Some(t) = &self.tree else { return };
        let paths: Vec<(NodeId, PathBuf)> = ids.iter().map(|&i| (i, PathBuf::from(t.path(i)))).collect();
        let mut errors = Vec::new();
        let mut done = Vec::new();
        if permanent {
            for (id, p) in &paths {
                match crate::actions::delete_permanently(p) {
                    Ok(()) => done.push(*id),
                    Err(e) => errors.push(format!("{}: {e}", p.display())),
                }
            }
        } else {
            let list: Vec<PathBuf> = paths.iter().map(|x| x.1.clone()).collect();
            match crate::actions::move_to_trash(&list) {
                Ok(()) => done.extend(paths.iter().map(|x| x.0)),
                Err(e) => {
                    // Fall back to one-by-one to find out which ones failed.
                    for (id, p) in &paths {
                        if !p.exists() {
                            done.push(*id);
                        } else if crate::actions::move_to_trash(std::slice::from_ref(p)).is_ok() {
                            done.push(*id);
                        } else {
                            errors.push(format!("\"{}\" can't be moved to the Trash. ({e})", p.display()));
                        }
                    }
                }
            }
        }
        if let Some(t) = &mut self.tree {
            for id in &done {
                t.mark_deleted(*id);
            }
            t.rebuild_indexes();
        }
        if let Some(m) = &self.monitor {
            m.ignore_paths(paths.iter().map(|p| p.1.clone()));
        }
        self.fv.sel.retain(|i| !done.contains(i));
        self.tree_changed();
        if !errors.is_empty() {
            self.error("Delete", &errors.join("\n"));
        }
    }

    pub fn do_rename(&mut self, id: NodeId, new_name: &str) {
        let p = self.path_of(id);
        match crate::actions::rename(&p, new_name) {
            Ok(np) => {
                if let Some(t) = &mut self.tree {
                    t.rename(id, new_name);
                }
                if let Some(m) = &self.monitor {
                    m.ignore_paths([p, np]);
                }
                self.tree_changed();
            }
            Err(e) => self.error(&format!("Couldn't rename \"{}\"", p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()), &e.to_string()),
        }
    }

    // ----------------------------------------------------------------- colors

    /// RGB per extension id, following the "Color by" settings.
    pub fn ext_colors(&self) -> Vec<[u8; 3]> {
        let Some(t) = &self.tree else { return Vec::new() };
        let colors: [[u8; 3]; 13] = if self.s.treemap_scheme == "Custom" {
            let mut c = [[0u8; 3]; 13];
            for (i, x) in self.s.palette.iter().take(13).enumerate() {
                c[i] = *x;
            }
            c
        } else {
            theme::scheme(&self.s.treemap_scheme).colors
        };
        let other = colors[12];
        let mut out = vec![other; t.exts.names.len()];
        match self.s.color_by {
            ColorBy::Size => {
                // Twelve largest file types get distinct colours (WizTree's colorRankBySize).
                for (rank, st) in t.ext_stats.iter().take(12).enumerate() {
                    out[st.ext as usize] = colors[rank];
                }
            }
            ColorBy::Extension => {
                for (i, list) in self.s.ext_lists.iter().enumerate().take(13) {
                    let c = self.s.ext_colors.get(i).copied().unwrap_or(other);
                    for e in list.split([',', ';', ' ']).map(|x| x.trim().trim_start_matches("*.").trim_start_matches('.').to_lowercase()) {
                        if e.is_empty() {
                            continue;
                        }
                        if let Some(id) = t.exts.get(&e) {
                            out[id as usize] = c;
                        }
                    }
                }
            }
        }
        out
    }

    pub fn scheme_gain(&self) -> f32 {
        if self.s.treemap_scheme == "Custom" { 0.72 } else { theme::scheme(&self.s.treemap_scheme).gain }
    }

    // ----------------------------------------------------------------- shortcuts

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let mut cmds: Vec<Cmd> = Vec::new();
        ctx.input_mut(|i| {
            let mut sc = |m: Modifiers, k: Key, c: Cmd| {
                if i.consume_shortcut(&KeyboardShortcut::new(m, k)) {
                    cmds.push(c);
                }
            };
            let none = Modifiers::NONE;
            let ctrl = Modifiers::COMMAND;
            let shift = Modifiers::SHIFT;
            let alt = Modifiers::ALT;
            let cs = Modifiers::COMMAND | Modifiers::SHIFT;
            let ca = Modifiers::COMMAND | Modifiers::ALT;
            // Recovered from WizTree's action list (TActionList in the main form).
            sc(none, Key::F5, Cmd::Scan);
            sc(none, Key::F3, Cmd::SelectFolder);
            sc(shift, Key::F3, Cmd::SelectCsv);
            sc(alt, Key::F3, Cmd::SelectMultiple);
            sc(none, Key::F4, Cmd::ResetTreeExpansion);
            sc(shift, Key::F4, Cmd::ExpandAll);
            sc(none, Key::F6, Cmd::SelectParent);
            sc(none, Key::F7, Cmd::ReselectChild);
            sc(none, Key::F8, Cmd::ToggleFileTypes);
            sc(none, Key::F9, Cmd::ToggleTreemap);
            sc(none, Key::F10, Cmd::ZoomTreemap);
            sc(none, Key::F11, Cmd::UnzoomTreemap);
            sc(shift, Key::F5, Cmd::ApplyFilter);
            sc(ctrl, Key::N, Cmd::NewInstance);
            sc(ctrl, Key::S, Cmd::SaveTreemapImage);
            sc(ctrl, Key::H, Cmd::ToggleHeader);
            sc(cs, Key::S, Cmd::ToggleTreemapFree);
            sc(cs, Key::N, Cmd::ToggleTreemapNames);
            sc(cs, Key::F, Cmd::ToggleFilterPanel);
            sc(cs, Key::C, Cmd::CopyFileInfo);
            sc(ca, Key::E, Cmd::ExportCsv);
            sc(ca, Key::F, Cmd::ExportFileTypes);
            sc(ctrl, Key::F, Cmd::FocusSearch);
            sc(Modifiers::ALT, Key::F4, Cmd::Exit);
            if !typing {
                sc(ca, Key::C, Cmd::CopyPath);
                sc(ctrl, Key::E, Cmd::Reveal);
                sc(ctrl, Key::P, Cmd::Terminal);
                sc(ctrl, Key::C, Cmd::Copy);
                sc(ctrl, Key::X, Cmd::Cut);
                sc(none, Key::Enter, Cmd::Open);
                sc(none, Key::Delete, Cmd::DeleteToTrash);
                sc(shift, Key::Delete, Cmd::DeletePermanently);
                sc(none, Key::F2, Cmd::Rename);
                sc(alt, Key::Enter, Cmd::Properties);
                sc(none, Key::Backspace, Cmd::SelectParent);
            }
        });
        // Copy/Cut arrive as events rather than key presses on some platforms.
        if !typing {
            let (copy, cut) = ctx.input(|i| {
                (i.events.iter().any(|e| matches!(e, egui::Event::Copy)), i.events.iter().any(|e| matches!(e, egui::Event::Cut)))
            });
            if copy && !cmds.contains(&Cmd::Copy) {
                cmds.push(Cmd::Copy);
            }
            if cut && !cmds.contains(&Cmd::Cut) {
                cmds.push(Cmd::Cut);
            }
        }
        for c in cmds {
            self.run(c, ctx);
        }
    }

    fn update_style(&mut self, ctx: &egui::Context) {
        let system_dark = ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(true);
        let key = format!("{}|{}|{}", self.s.theme, self.s.accent, system_dark);
        if key != self.style_key {
            self.pal = theme::resolve(&self.s.theme, &self.s.accent, system_dark);
            theme::apply(ctx, &self.pal);
            self.style_key = key;
            self.tm.dirty = true;
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if dropped.is_empty() || !self.s.drag_drop {
            return;
        }
        let src = if dropped.len() > 1 {
            Source::Multi(dropped)
        } else if dropped[0].is_dir() {
            Source::Folder(dropped[0].clone())
        } else if dropped[0].extension().map(|e| e.eq_ignore_ascii_case("csv")).unwrap_or(false) {
            Source::Csv(dropped[0].clone())
        } else {
            return;
        };
        self.source = Some(src);
        self.start_scan();
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.update_style(&ctx);
        self.poll_scan(&ctx);
        if let Some(m) = &mut self.monitor {
            if let Some(t) = &mut self.tree {
                if m.apply(t) {
                    self.rows_dirty = true;
                    self.tm.dirty = true;
                    self.fv.dirty = true;
                    self.ext_rows.clear();
                }
            }
        }
        self.handle_drops(&ctx);
        if self.dlg.none_open() && self.confirm.is_none() && self.rename.is_none() {
            self.shortcuts(&ctx);
        }

        chrome::menu_bar(self, ui);
        chrome::toolbar(self, ui);
        chrome::status_bar(self, ui);
        chrome::banners(self, ui);
        if self.s.show_filter_panel {
            chrome::filter_panel(self, ui);
        }
        if self.s.show_header && self.tab != Tab::About {
            chrome::header(self, ui);
        }

        let pal = self.pal;
        egui::CentralPanel::no_frame()
            .show(ui, |ui| {
                let r = ui.max_rect();
                ui.painter().rect_filled(r, 0.0, pal.bg);
                let inner = r.shrink2(egui::vec2(14.0, 10.0));
                let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
                if self.scan.is_some() && self.tree.is_none() {
                    chrome::scanning_view(self, &mut cui);
                } else {
                    match self.tab {
                        Tab::Tree => {
                            if self.tree.is_some() {
                                tree_view::show(self, &mut cui);
                            } else {
                                chrome::empty_state(self, &mut cui);
                            }
                        }
                        Tab::Files => {
                            if self.tree.is_some() {
                                file_view::show(self, &mut cui);
                            } else {
                                chrome::empty_state(self, &mut cui);
                            }
                        }
                        Tab::About => chrome::about(self, &mut cui),
                    }
                }
            });

        dialogs::show_all(self, &ctx);

        // Remember the window geometry for the next start.
        ctx.input(|i| {
            let vp = i.viewport();
            if let Some(m) = vp.maximized {
                self.s.window_maximized = m;
            }
            if let (Some(r), false) = (vp.inner_rect, vp.maximized.unwrap_or(false) || vp.fullscreen.unwrap_or(false)) {
                self.s.window_w = r.width();
                self.s.window_h = r.height();
            }
        });

        if self.exit_requested {
            self.s.save();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.last_save.elapsed().as_secs() > 30 {
            self.s.save();
            self.last_save = Instant::now();
        }
    }

    fn on_exit(&mut self) {
        self.s.save();
    }
}

#[cfg(test)]
mod snapshots;

/// Install the udev rule that grants NTFS read access (runs in the background).
pub fn install_ntfs_access(password: String) {
    std::thread::spawn(move || {
        let Ok(exe) = std::env::current_exe() else { return };
        let child = std::process::Command::new("sudo")
            .args(["-S", "-p", "", "--"])
            .arg(exe)
            .arg("--install-ntfs-access")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if let Ok(mut c) = child {
            if let Some(mut i) = c.stdin.take() {
                use std::io::Write;
                let _ = writeln!(i, "{password}");
            }
            let _ = c.wait();
        }
    });
}
