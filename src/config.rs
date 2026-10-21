//! Persistent settings (JSON). Portable mode: if `diskmosaic.json` exists next
//! to the executable it is used instead of the per-user config directory.

use crate::format::SizeUnit;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ColorBy {
    #[default]
    Size,
    Extension,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DupMode {
    #[default]
    Off,
    NameSize,
    NameSizeDate,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub size_unit: SizeUnit,
    pub binary_units: bool,
    pub decimals: usize,
    pub auto_scan: bool,
    pub check_updates: bool,
    pub run_as_admin: bool,
    pub context_menu: bool,
    pub dont_show_admin_warning: bool,
    pub show_header: bool,
    pub show_treemap: bool,
    pub show_file_types: bool,
    pub treemap_allocated: bool,
    pub treemap_free_space: bool,
    pub treemap_names: bool,
    pub show_filter_panel: bool,
    pub treemap_height_frac: f32,
    pub ext_panel_width: f32,

    // Colors
    pub color_by: ColorBy,
    /// UI theme name (see `theme::THEMES`) or "Auto".
    pub theme: String,
    /// Accent colour name (see `theme::ACCENTS`) or "" for the theme default.
    pub accent: String,
    /// Treemap colour scheme name (see `theme::SCHEMES`) or "Custom".
    pub treemap_scheme: String,
    /// Custom palette used when `treemap_scheme == "Custom"`.
    pub palette: Vec<[u8; 3]>,
    /// Extension lists for "Color by extension" (comma separated) and colors.
    pub ext_lists: Vec<String>,
    pub ext_colors: Vec<[u8; 3]>,

    // Tweaks
    pub drag_drop: bool,
    pub double_click_open: bool,
    pub monitor_changes: bool,
    pub max_modified_date: bool,
    pub classic_theme: bool,
    pub horizontal_lines: bool,
    pub alt_row_colors: bool,
    pub gradient: bool,
    pub mft_for_folders: bool,
    pub file_group_min: u32,
    pub enable_rename: bool,
    pub one_filesystem: bool,

    // File view
    pub match_path: bool,
    pub max_files: usize,
    pub show_folders_in_files: bool,
    pub dup_mode: DupMode,
    pub dups_only: bool,
    pub search_history: Vec<String>,

    // Filter
    pub include_filter: String,
    pub exclude_filter: String,
    pub filter_match_path: bool,
    pub include_history: Vec<String>,
    pub exclude_history: Vec<String>,

    // Export
    pub export_dates: bool,
    pub export_folder_sizes: bool,
    pub export_split_names: bool,
    pub export_drive_size: bool,
    pub export_percent: bool,
    pub export_max_depth: u32,
    pub export_file_ids: bool,
    pub export_files: bool,
    pub export_folders: bool,
    pub export_dir: String,
    pub treemap_export_w: u32,
    pub treemap_export_h: u32,
    pub treemap_export_gray: bool,
    pub treemap_export_free: bool,

    pub custom_folders: Vec<String>,
    pub multi_checked: Vec<String>,
    pub last_selection: String,
    pub language: String,
    pub tree_columns: Vec<f32>,
    pub file_columns: Vec<f32>,
    pub window_w: f32,
    pub window_h: f32,
    pub window_maximized: bool,
    /// Bumped when a default changes so older settings files are migrated.
    /// Field-level default 0 (not the struct default) so files written before
    /// this key existed can be recognised as old.
    #[serde(default)]
    pub settings_version: u32,
}

pub const DEFAULT_PALETTE: [[u8; 3]; 13] = [
    [0x2f, 0x6f, 0xd8],
    [0x2b, 0xa8, 0x4a],
    [0xe0, 0x3c, 0x31],
    [0xf5, 0xb8, 0x00],
    [0x9a, 0x4c, 0xd6],
    [0x00, 0xb5, 0xc8],
    [0xf2, 0x6d, 0x21],
    [0xd8, 0x3f, 0x9a],
    [0x7c, 0xb3, 0x42],
    [0x5c, 0x6b, 0xc0],
    [0xa1, 0x88, 0x7f],
    [0x26, 0xa6, 0x9a],
    [0xc0, 0xca, 0x33],
];

pub const COLOR_BLIND_PALETTE: [[u8; 3]; 13] = [
    [0x00, 0x72, 0xb2],
    [0xe6, 0x9f, 0x00],
    [0x56, 0xb4, 0xe9],
    [0x00, 0x9e, 0x73],
    [0xf0, 0xe4, 0x42],
    [0xd5, 0x5e, 0x00],
    [0xcc, 0x79, 0xa7],
    [0x99, 0x99, 0x99],
    [0x33, 0x22, 0x88],
    [0x88, 0xcc, 0xee],
    [0x44, 0xaa, 0x99],
    [0x99, 0x99, 0x33],
    [0xaa, 0x44, 0x99],
];

pub fn default_ext_lists() -> Vec<String> {
    [
        "mp3,ogg,wav,wma,m4a,aac,flac,aiff,aif,caf,opus",
        "mkv,mp4,mpeg,mpg,mov,ogm,ogv,swf,webm,wmv,m4v,avi",
        "bmp,gif,jpeg,jpg,pcx,png,tga,tif,tiff,heic,heif,webp,icns",
        "7z,ace,arj,cab,gz,gzip,jar,rar,tar,tgz,z,zip,bz2,xz,zst,tbz,tbz2,txz,sit,sitx",
        "vdi,vhd,vhdx,qcow,qcow2,vmdk,hdd",
        "iso,nrg,bin,dmg,sparseimage,cdr,toast,img",
        "doc,dot,wbk,docx,xls,xlsx,ppt,pptx,pub,pages,numbers,key,pdf,odt,ods,odp",
        "exe,dll,sys,so,dylib,msi,app,deb,rpm",
        "",
        "",
        "",
        "",
        "",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            size_unit: SizeUnit::Dynamic,
            binary_units: true,
            decimals: 1,
            auto_scan: true,
            check_updates: false,
            run_as_admin: false,
            context_menu: false,
            dont_show_admin_warning: false,
            show_header: true,
            show_treemap: true,
            show_file_types: true,
            treemap_allocated: false,
            treemap_free_space: false,
            treemap_names: false,
            show_filter_panel: false,
            treemap_height_frac: 0.42,
            ext_panel_width: 440.0,
            color_by: ColorBy::Size,
            theme: "Auto".into(),
            accent: String::new(),
            treemap_scheme: "Vivid".into(),
            palette: DEFAULT_PALETTE.to_vec(),
            ext_lists: default_ext_lists(),
            ext_colors: DEFAULT_PALETTE.to_vec(),
            drag_drop: true,
            double_click_open: true,
            monitor_changes: true,
            max_modified_date: true,
            classic_theme: false,
            horizontal_lines: false,
            alt_row_colors: true,
            gradient: true,
            mft_for_folders: false,
            file_group_min: 4,
            enable_rename: true,
            one_filesystem: true,
            match_path: false,
            max_files: 1000,
            show_folders_in_files: false,
            dup_mode: DupMode::Off,
            dups_only: false,
            search_history: Vec::new(),
            include_filter: String::new(),
            exclude_filter: String::new(),
            filter_match_path: true,
            include_history: Vec::new(),
            exclude_history: Vec::new(),
            export_dates: false,
            export_folder_sizes: false,
            export_split_names: false,
            export_drive_size: true,
            export_percent: false,
            export_max_depth: 0,
            export_file_ids: false,
            export_files: true,
            export_folders: true,
            export_dir: String::new(),
            treemap_export_w: 1920,
            treemap_export_h: 1080,
            treemap_export_gray: false,
            treemap_export_free: false,
            custom_folders: Vec::new(),
            multi_checked: Vec::new(),
            last_selection: String::new(),
            language: String::new(),
            tree_columns: Vec::new(),
            file_columns: Vec::new(),
            window_w: 1280.0,
            window_h: 800.0,
            window_maximized: false,
            settings_version: SETTINGS_VERSION,
        }
    }
}

/// 2: folders show the newest file date below them by default (as WizTree).
pub const SETTINGS_VERSION: u32 = 2;

pub fn portable_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join("diskmosaic.json");
    p.exists().then_some(p)
}

pub fn settings_path() -> PathBuf {
    if let Some(p) = portable_path() {
        return p;
    }
    let dir = directories::ProjectDirs::from("com", "diskmosaic", "DiskMosaic")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("diskmosaic"));
    dir.join("settings.json")
}

impl Settings {
    pub fn load() -> Settings {
        if cfg!(test) {
            return Settings::default();
        }
        let p = settings_path();
        let mut s: Settings = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.fix();
        s
    }

    pub fn fix(&mut self) {
        // v2: WizTree shows a folder's newest file date instead of the
        // folder's own date. Older settings files still carry the old
        // `max_modified_date: false` default, so turn it on once.
        if self.settings_version < 2 {
            self.max_modified_date = true;
        }
        self.settings_version = SETTINGS_VERSION;
        if self.palette.len() < 13 {
            self.palette = DEFAULT_PALETTE.to_vec();
        }
        if self.ext_lists.len() < 13 {
            self.ext_lists.resize(13, String::new());
        }
        if self.ext_colors.len() < 13 {
            self.ext_colors = DEFAULT_PALETTE.to_vec();
        }
        self.decimals = self.decimals.min(4);
    }

    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        let p = settings_path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }

    pub fn reset() {
        let _ = std::fs::remove_file(settings_path());
    }

    pub fn fmt(&self) -> crate::format::Fmt {
        crate::format::Fmt { unit: self.size_unit, decimals: self.decimals, base: if self.binary_units { 1024 } else { 1000 } }
    }

    pub fn push_history(list: &mut Vec<String>, s: &str) {
        let s = s.trim();
        if s.is_empty() {
            return;
        }
        list.retain(|x| x != s);
        list.insert(0, s.to_string());
        list.truncate(25);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Settings files written before v2 lack `settings_version`, so they must
    /// read as 0 and get the new "newest file below" folder date default.
    #[test]
    fn migrates_old_settings() {
        let mut s: Settings = serde_json::from_str(r#"{"max_modified_date": false}"#).unwrap();
        assert_eq!(s.settings_version, 0);
        s.fix();
        assert_eq!(s.settings_version, SETTINGS_VERSION);
        assert!(s.max_modified_date);
    }

    /// A deliberate choice made under the current version is left alone.
    #[test]
    fn keeps_current_settings() {
        let mut s: Settings =
            serde_json::from_str(r#"{"max_modified_date": false, "settings_version": 2}"#).unwrap();
        s.fix();
        assert!(!s.max_modified_date);
        assert_eq!(s.settings_version, SETTINGS_VERSION);
    }
}
