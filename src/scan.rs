//! Parallel file system scanner.
//!
//! Directory reads run on a thread pool; a single collector thread appends
//! the results to the [`Tree`] arena (so `parent < child` always holds) and
//! schedules the sub-directories it discovers. The collector only does
//! cheap pushes, so throughput is bounded by the kernel, not by us.

use crate::model::{Node, NodeId, Tree, VolumeStats, flags, NONE};
use crate::volumes;
use crossbeam_channel::unbounded;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub folders: AtomicU64,
    pub bytes: AtomicU64,
    /// Estimated total bytes (used space of the volume) for the progress bar.
    pub expected: AtomicU64,
    pub cancel: AtomicBool,
    pub current: Mutex<String>,
}

impl Progress {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Do not descend into other file systems / mount points.
    pub one_filesystem: bool,
    /// Try reading the NTFS MFT directly (NTFS volumes only).
    pub use_mft: bool,
    /// Use the MFT also when a folder (not a whole drive) is selected.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub mft_for_folders: bool,
    pub threads: usize,
    /// How to get raw read access to an NTFS device when the user lacks it.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub elevation: Elevation,
}

/// Privilege escalation used to read the raw MFT through the helper mode
/// (`diskmosaic --mft-dump DEV`), which streams only the MFT bytes back.
#[derive(Clone, Debug, Default)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub enum Elevation {
    /// Only use the device if it can be opened directly.
    #[default]
    None,
    /// `sudo -S` with this password written to its stdin.
    Sudo(String),
    /// `sudo` prompting on the controlling terminal (command line use).
    SudoTty,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            one_filesystem: true,
            use_mft: true,
            mft_for_folders: false,
            threads: 0,
            elevation: Elevation::None,
        }
    }
}

pub struct ScanResult {
    pub tree: Tree,
    pub elapsed: f64,
    pub cancelled: bool,
    pub used_mft: bool,
    /// Why the fast path was not used, or why the scan stopped.
    pub note: Option<String>,
}

/// One directory entry as returned by the platform layer.
#[derive(Clone, Debug, Default)]
pub struct RawEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub alloc: u64,
    pub mtime: i64,
    pub atime: i64,
    pub ctime: i64,
    pub file_id: u64,
    pub dev: u64,
    pub nlink: u64,
    pub flags: u32,
}

struct DirMsg {
    dir: NodeId,
    path: PathBuf,
    entries: Vec<RawEntry>,
    denied: bool,
}

pub fn scan(roots: &[PathBuf], opts: &ScanOptions, progress: Arc<Progress>) -> ScanResult {
    let start = Instant::now();

    #[allow(unused_mut)]
    let mut note = None;
    if opts.use_mft {
        #[cfg(windows)]
        if let Some(mut res) = crate::ntfs::try_scan_windows(roots, opts, &progress) {
            res.elapsed = start.elapsed().as_secs_f64();
            return res;
        }
        #[cfg(target_os = "linux")]
        {
            use crate::ntfs::MftMode;
            let mode = crate::ntfs::linux_policy(roots, opts);
            if mode == MftMode::Race {
                // A folder on a spinning NTFS disk: small folders walk faster
                // than the whole MFT can be read, big ones don't. Walk for a
                // moment and switch to the MFT if it isn't done by then.
                let inner = Arc::new(Progress::default());
                let (r2, o2, i2) = (roots.to_vec(), opts.clone(), inner.clone());
                let h = std::thread::spawn(move || walk(&r2, &o2, i2, start, None));
                let deadline = Instant::now() + std::time::Duration::from_secs(2);
                while !h.is_finished() && Instant::now() < deadline && !progress.cancelled() {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    progress.files.store(inner.files.load(Ordering::Relaxed), Ordering::Relaxed);
                    progress.folders.store(inner.folders.load(Ordering::Relaxed), Ordering::Relaxed);
                }
                if h.is_finished() || progress.cancelled() {
                    inner.cancel.store(progress.cancelled(), Ordering::Relaxed);
                    return h.join().expect("walk thread");
                }
                inner.cancel.store(true, Ordering::Relaxed);
                let _ = h.join();
                for c in [&progress.files, &progress.folders, &progress.bytes] {
                    c.store(0, Ordering::Relaxed);
                }
            }
            if mode != MftMode::Skip {
                match crate::ntfs::try_scan_linux(roots, opts, &progress) {
                    Ok(Some(mut res)) => {
                        res.elapsed = start.elapsed().as_secs_f64();
                        return res;
                    }
                    Ok(None) => {}
                    Err(e) => note = Some(e),
                }
            }
        }
    }
    walk(roots, opts, progress, start, note)
}

/// Parallel directory walk (every platform / file system).
fn walk(roots: &[PathBuf], opts: &ScanOptions, progress: Arc<Progress>, start: Instant, note: Option<String>) -> ScanResult {
    let mut tree = Tree::new();
    let mut vol = VolumeStats::default();
    let mut seen_vols: HashSet<String> = HashSet::new();
    for r in roots {
        if let Some(v) = volumes::volume_stats(r) {
            if seen_vols.insert(v.serial.clone() + &v.label + &v.total.to_string()) {
                vol.total += v.total;
                vol.free += v.free;
                vol.reserved += v.reserved;
                if vol.label.is_empty() {
                    vol.label = v.label.clone();
                    vol.fs_type = v.fs_type.clone();
                    vol.serial = v.serial.clone();
                }
            }
        }
    }
    progress.expected.store(vol.used(), Ordering::Relaxed);
    tree.volume = vol;

    let threads = if opts.threads > 0 {
        opts.threads
    } else {
        (std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4) * 2).clamp(4, 64)
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("scan-{i}"))
        .build()
        .expect("thread pool");

    let (tx, rx) = unbounded::<DirMsg>();
    let mut outstanding: usize = 0;
    let mut hardlinks: HashSet<(u64, u64)> = HashSet::new();
    let mut root_devs: Vec<u64> = Vec::new();

    for r in roots {
        let root_name = display_root(r);
        let meta = platform::stat_path(r);
        let mut node = Node { flags: flags::DIR | flags::ROOT, ..Default::default() };
        let mut dev = 0;
        if let Some(m) = &meta {
            node.mtime = m.mtime;
            node.atime = m.atime;
            node.ctime = m.ctime;
            node.file_id = m.file_id;
            dev = m.dev;
            if !m.is_dir {
                // A single file was selected: make it a root file.
                node = Node { flags: flags::ROOT, size: m.size, alloc: m.alloc, mtime: m.mtime, ..Default::default() };
            }
        }
        let id = tree.add_node(NONE, &root_name, node);
        tree.roots.push(id);
        root_devs.push(dev);
        if tree.nodes[id as usize].is_dir() {
            outstanding += 1;
            spawn_read(&pool, tx.clone(), id, r.clone(), progress.clone());
        }
    }

    let skip = platform::skip_paths(roots);
    let root_paths: Vec<PathBuf> = roots.to_vec();

    while outstanding > 0 {
        let msg = match rx.recv() {
            Ok(m) => m,
            Err(_) => break,
        };
        outstanding -= 1;
        if msg.denied {
            tree.nodes[msg.dir as usize].flags |= flags::ACCESS_DENIED;
        }
        let root_dev = {
            // Find the root device for this dir by walking to the root.
            let mut cur = msg.dir;
            while !tree.is_root(cur) {
                cur = tree.nodes[cur as usize].parent;
            }
            let idx = tree.roots.iter().position(|&r| r == cur).unwrap_or(0);
            root_devs.get(idx).copied().unwrap_or(0)
        };
        let mut files = 0u64;
        let mut dirs = 0u64;
        let mut bytes = 0u64;
        for e in msg.entries {
            let mut node = Node {
                size: e.size,
                alloc: e.alloc,
                mtime: e.mtime,
                atime: e.atime,
                ctime: e.ctime,
                file_id: e.file_id,
                flags: e.flags,
                ..Default::default()
            };
            if e.is_dir {
                node.flags |= flags::DIR;
                node.size = 0;
                node.alloc = 0;
                dirs += 1;
            } else {
                if e.nlink > 1 && !hardlinks.insert((e.dev, e.file_id)) {
                    node.flags |= flags::HARDLINK_DUP;
                    node.size = 0;
                    node.alloc = 0;
                }
                files += 1;
                bytes += node.alloc.max(node.size);
            }
            let name = e.name;
            let id = tree.add_node(msg.dir, &name, node);
            if e.is_dir && e.flags & flags::SYMLINK == 0 {
                let child = msg.path.join(&name);
                let other_dev = opts.one_filesystem && root_dev != 0 && e.dev != 0 && e.dev != root_dev;
                if (other_dev && !platform::allow_cross_device(&child, &root_paths)) || skip.iter().any(|s| s == &child) {
                    tree.nodes[id as usize].flags |= flags::MOUNT;
                    continue;
                }
                if !progress.cancelled() {
                    outstanding += 1;
                    spawn_read(&pool, tx.clone(), id, child, progress.clone());
                }
            }
        }
        progress.files.fetch_add(files, Ordering::Relaxed);
        progress.folders.fetch_add(dirs, Ordering::Relaxed);
        progress.bytes.fetch_add(bytes, Ordering::Relaxed);
    }
    drop(tx);
    let cancelled = progress.cancelled();
    tree.finalize();
    ScanResult { tree, elapsed: start.elapsed().as_secs_f64(), cancelled, used_mft: false, note }
}

fn spawn_read(
    pool: &rayon::ThreadPool,
    tx: crossbeam_channel::Sender<DirMsg>,
    dir: NodeId,
    path: PathBuf,
    progress: Arc<Progress>,
) {
    pool.spawn(move || {
        if progress.cancelled() {
            let _ = tx.send(DirMsg { dir, path, entries: Vec::new(), denied: false });
            return;
        }
        let (entries, denied) = match platform::read_dir(&path) {
            Ok(e) => (e, false),
            Err(_) => (Vec::new(), true),
        };
        if let Some(mut cur) = progress.current.try_lock() {
            cur.clear();
            cur.push_str(&path.to_string_lossy());
        }
        let _ = tx.send(DirMsg { dir, path, entries, denied });
    });
}

/// Re-read a single directory (non-recursively for files, recursively for new
/// sub folders). Used by the file system monitor.
pub fn scan_subtree(path: &Path) -> Option<Tree> {
    let progress = Arc::new(Progress::default());
    let opts = ScanOptions { use_mft: false, ..Default::default() };
    let res = scan(&[path.to_path_buf()], &opts, progress);
    Some(res.tree)
}

pub fn display_root(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        if s.len() == 2 && s.ends_with(':') {
            return format!("{s}\\");
        }
    }
    if s.len() > 1 {
        let t = s.trim_end_matches(['/', '\\']);
        if t.is_empty() {
            return s[..1].to_string();
        }
        #[cfg(windows)]
        if t.len() == 2 && t.ends_with(':') {
            return format!("{t}\\");
        }
        return t.to_string();
    }
    s
}

pub mod platform {
    use super::RawEntry;
    #[allow(unused_imports)]
    use crate::model::flags;
    use std::io;
    use std::path::{Path, PathBuf};

    pub struct Stat {
        pub is_dir: bool,
        pub size: u64,
        pub alloc: u64,
        pub mtime: i64,
        pub atime: i64,
        pub ctime: i64,
        pub file_id: u64,
        pub dev: u64,
    }

    #[cfg(unix)]
    pub fn stat_path(p: &Path) -> Option<Stat> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(p).ok()?;
        Some(Stat {
            is_dir: m.is_dir(),
            size: m.len(),
            alloc: m.blocks() * 512,
            mtime: m.mtime(),
            atime: m.atime(),
            ctime: created_unix(&m),
            file_id: m.ino(),
            dev: m.dev(),
        })
    }

    #[cfg(unix)]
    fn created_unix(m: &std::fs::Metadata) -> i64 {
        use std::os::unix::fs::MetadataExt;
        m.created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or_else(|| m.ctime())
    }

    #[cfg(unix)]
    pub fn read_dir(p: &Path) -> io::Result<Vec<RawEntry>> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let rd = std::fs::read_dir(p)?;
        let mut out = Vec::with_capacity(32);
        for ent in rd {
            let Ok(ent) = ent else { continue };
            let name = ent.file_name();
            let name = match name.into_string() {
                Ok(s) => s,
                Err(os) => os.to_string_lossy().into_owned(),
            };
            let Ok(m) = ent.metadata() else {
                out.push(RawEntry { name, ..Default::default() });
                continue;
            };
            let ft = m.file_type();
            let mut fl = 0;
            if ft.is_symlink() {
                fl |= flags::SYMLINK;
            }
            if name.starts_with('.') {
                fl |= flags::HIDDEN;
            }
            if m.mode() & 0o222 == 0 {
                fl |= flags::READONLY;
            }
            if ft.is_block_device() || ft.is_char_device() || ft.is_fifo() || ft.is_socket() {
                fl |= flags::SYSTEM;
            }
            #[cfg(target_os = "macos")]
            {
                use std::os::macos::fs::MetadataExt as _;
                // UF_HIDDEN = 0x8000, UF_COMPRESSED = 0x20
                let f = m.st_flags();
                if f & 0x8000 != 0 {
                    fl |= flags::HIDDEN;
                }
                if f & 0x20 != 0 {
                    fl |= flags::COMPRESSED;
                }
            }
            let blocks = m.blocks() * 512;
            if blocks < m.len() && m.len() > 0 && blocks > 0 {
                fl |= flags::SPARSE;
            }
            out.push(RawEntry {
                name,
                is_dir: ft.is_dir(),
                size: if ft.is_dir() { 0 } else { m.len() },
                alloc: blocks,
                mtime: m.mtime(),
                atime: m.atime(),
                ctime: created_unix(&m),
                file_id: m.ino(),
                dev: m.dev(),
                nlink: if ft.is_dir() { 1 } else { m.nlink() },
                flags: fl,
            });
        }
        Ok(out)
    }

    #[cfg(windows)]
    pub fn stat_path(p: &Path) -> Option<Stat> {
        use std::os::windows::fs::MetadataExt;
        let m = std::fs::metadata(p).ok()?;
        Some(Stat {
            is_dir: m.is_dir(),
            size: m.len(),
            alloc: round_cluster(p, m.len()),
            mtime: filetime_to_unix(m.last_write_time()),
            atime: filetime_to_unix(m.last_access_time()),
            ctime: filetime_to_unix(m.creation_time()),
            file_id: 0,
            dev: 0,
        })
    }

    #[cfg(windows)]
    pub fn filetime_to_unix(ft: u64) -> i64 {
        if ft == 0 {
            return 0;
        }
        (ft / 10_000_000) as i64 - 11_644_473_600
    }

    #[cfg(windows)]
    fn round_cluster(p: &Path, size: u64) -> u64 {
        let c = crate::volumes::cluster_size(p).max(1);
        size.div_ceil(c) * c
    }

    #[cfg(windows)]
    pub fn read_dir(p: &Path) -> io::Result<Vec<RawEntry>> {
        use std::os::windows::fs::MetadataExt;
        let cluster = crate::volumes::cluster_size(p).max(1);
        let rd = std::fs::read_dir(p)?;
        let mut out = Vec::with_capacity(32);
        for ent in rd {
            let Ok(ent) = ent else { continue };
            let name = ent.file_name().to_string_lossy().into_owned();
            let Ok(m) = ent.metadata() else { continue };
            let a = m.file_attributes();
            let mut fl = 0;
            if a & 0x2 != 0 { fl |= flags::HIDDEN; }
            if a & 0x1 != 0 { fl |= flags::READONLY; }
            if a & 0x4 != 0 { fl |= flags::SYSTEM; }
            if a & 0x20 != 0 { fl |= flags::ARCHIVE; }
            if a & 0x800 != 0 { fl |= flags::COMPRESSED; }
            if a & 0x200 != 0 { fl |= flags::SPARSE; }
            if a & 0x4000 != 0 { fl |= flags::ENCRYPTED; }
            if a & 0x1000 != 0 { fl |= flags::OFFLINE; }
            if a & 0x100 != 0 { fl |= flags::TEMPORARY; }
            if a & 0x2000 != 0 { fl |= flags::NOT_INDEXED; }
            if a & 0x400 != 0 { fl |= flags::REPARSE | flags::SYMLINK; }
            let is_dir = a & 0x10 != 0;
            let size = if is_dir { 0 } else { m.len() };
            out.push(RawEntry {
                name,
                is_dir,
                size,
                alloc: if a & 0x400 != 0 || a & 0x1000 != 0 { 0 } else { size.div_ceil(cluster) * cluster },
                mtime: filetime_to_unix(m.last_write_time()),
                atime: filetime_to_unix(m.last_access_time()),
                ctime: filetime_to_unix(m.creation_time()),
                file_id: 0,
                dev: 0,
                nlink: 1,
                flags: fl,
            });
        }
        Ok(out)
    }

    /// Paths never descended into for the given roots.
    pub fn skip_paths(roots: &[PathBuf]) -> Vec<PathBuf> {
        #[allow(unused_mut)]
        let mut v = Vec::new();
        for r in roots {
            #[cfg(target_os = "macos")]
            if r == Path::new("/") {
                // The data volume is reachable through firmlinks already.
                v.push(PathBuf::from("/System/Volumes/Data"));
                v.push(PathBuf::from("/Volumes"));
                v.push(PathBuf::from("/dev"));
            }
            #[cfg(target_os = "linux")]
            {
                let _ = r;
            }
            #[cfg(windows)]
            {
                let _ = r;
            }
        }
        v
    }

    /// Whether a mount on another device should still be descended into.
    pub fn allow_cross_device(_p: &Path, _roots: &[PathBuf]) -> bool {
        // macOS: the system and data volumes of an APFS volume group are
        // stitched together with firmlinks; treat them as one.
        #[cfg(target_os = "macos")]
        {
            let s = _p.to_string_lossy();
            return !(s.starts_with("/Volumes") || s.starts_with("/dev") || s.starts_with("/System/Volumes/Data"));
        }
        // Linux: btrfs subvolumes (and bind mounts) of the same block device
        // belong to the same volume; nested subvolumes that aren't mount
        // points at all are always part of it.
        #[cfg(target_os = "linux")]
        {
            let mounts = crate::volumes::mount_sources();
            let source_of = |p: &Path| -> Option<String> {
                let mut best: Option<(usize, &String)> = None;
                for (mp, src) in mounts.iter() {
                    if p.starts_with(mp) {
                        let l = mp.as_os_str().len();
                        if best.map(|b| l > b.0).unwrap_or(true) {
                            best = Some((l, src));
                        }
                    }
                }
                best.map(|b| b.1.clone())
            };
            let is_mount = mounts.iter().any(|(mp, _)| mp == _p);
            if !is_mount {
                return true;
            }
            let child_src = source_of(_p);
            return _roots.iter().any(|r| source_of(r) == child_src && child_src.as_deref().is_some_and(|s| s.starts_with('/')));
        }
        #[allow(unreachable_code)]
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_temp_dir() {
        let dir = std::env::temp_dir().join(format!("diskmosaic_scan_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub/deeper")).unwrap();
        std::fs::write(dir.join("a.txt"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.join("sub/b.bin"), vec![0u8; 5000]).unwrap();
        std::fs::write(dir.join("sub/deeper/c.bin"), vec![0u8; 7]).unwrap();
        let res = scan(&[dir.clone()], &ScanOptions::default(), Arc::new(Progress::default()));
        let t = &res.tree;
        let r = t.roots[0];
        assert_eq!(t.nodes[r as usize].size, 6007);
        assert_eq!(t.nodes[r as usize].files, 3);
        assert_eq!(t.nodes[r as usize].folders, 2);
        assert_eq!(t.name(t.kids(r)[0]), "sub");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
