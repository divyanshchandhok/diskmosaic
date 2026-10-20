//! Volume enumeration and capacity queries.

use crate::model::VolumeStats;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct VolumeInfo {
    pub path: PathBuf,
    pub display: String,
    pub label: String,
    pub fs_type: String,
    pub total: u64,
    pub free: u64,
    pub is_boot: bool,
    #[allow(dead_code)]
    pub is_local: bool,
}

#[cfg(unix)]
pub fn volume_stats(p: &Path) -> Option<VolumeStats> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(p.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let frs = if s.f_frsize > 0 { s.f_frsize as u64 } else { s.f_bsize as u64 };
    let total = s.f_blocks as u64 * frs;
    let free = s.f_bavail as u64 * frs;
    let reserved = (s.f_bfree as u64).saturating_sub(s.f_bavail as u64) * frs;
    let (fs_type, label) = mount_info_for(p);
    Some(VolumeStats {
        total,
        free,
        reserved,
        label,
        fs_type,
        serial: format!("{:x}", s.f_fsid as u64),
    })
}

#[cfg(target_os = "linux")]
fn mount_info_for(p: &Path) -> (String, String) {
    let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut best: Option<(usize, String, String)> = None;
    for m in linux_mounts() {
        if canon.starts_with(&m.0) {
            let l = m.0.as_os_str().len();
            if best.as_ref().map(|b| l > b.0).unwrap_or(true) {
                best = Some((l, m.1.clone(), m.2.clone()));
            }
        }
    }
    best.map(|b| (b.1, b.2)).unwrap_or_default()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn mount_info_for(p: &Path) -> (String, String) {
    let c = match std::ffi::CString::new(p.as_os_str().to_string_lossy().as_bytes()) {
        Ok(c) => c,
        Err(_) => return Default::default(),
    };
    let mut s: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut s) } != 0 {
        return Default::default();
    }
    let ty = unsafe { std::ffi::CStr::from_ptr(s.f_fstypename.as_ptr()) }.to_string_lossy().to_string();
    let mnt = unsafe { std::ffi::CStr::from_ptr(s.f_mntonname.as_ptr()) }.to_string_lossy().to_string();
    let label = if mnt == "/" {
        "Macintosh HD".to_string()
    } else {
        Path::new(&mnt).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(mnt)
    };
    (ty, label)
}

/// (mount point, fs type, source device)
#[cfg(target_os = "linux")]
fn linux_mounts() -> Vec<(PathBuf, String, String)> {
    let Ok(text) = std::fs::read_to_string("/proc/self/mountinfo") else { return Vec::new() };
    let mut v = Vec::new();
    for line in text.lines() {
        // id parent maj:min root mountpoint opts... - fstype source superopts
        let mut halves = line.splitn(2, " - ");
        let left = halves.next().unwrap_or("");
        let right = halves.next().unwrap_or("");
        let lf: Vec<&str> = left.split(' ').collect();
        let rf: Vec<&str> = right.split(' ').collect();
        if lf.len() < 5 || rf.len() < 2 {
            continue;
        }
        let mp = unescape_mount(lf[4]);
        v.push((PathBuf::from(mp), rf[0].to_string(), unescape_mount(rf[1])));
    }
    v
}

/// The mount containing `p`: (mount point, fs type, source device).
#[cfg(target_os = "linux")]
pub fn mount_for(p: &Path) -> Option<(PathBuf, String, String)> {
    let p = std::fs::canonicalize(p).ok()?;
    linux_mounts().into_iter().filter(|m| p.starts_with(&m.0)).max_by_key(|m| m.0.as_os_str().len())
}

/// Whether a block device (e.g. `/dev/sda1`) sits on a spinning disk.
#[cfg(target_os = "linux")]
pub fn is_rotational(dev: &Path) -> bool {
    let Ok(dev) = std::fs::canonicalize(dev) else { return false };
    let Some(name) = dev.file_name() else { return false };
    let Ok(sys) = std::fs::canonicalize(Path::new("/sys/class/block").join(name)) else { return false };
    // Partitions have no queue/ of their own; use the parent disk's.
    for d in [sys.clone(), sys.parent().map(Path::to_path_buf).unwrap_or(sys)] {
        if let Ok(v) = std::fs::read_to_string(d.join("queue/rotational")) {
            return v.trim() == "1";
        }
    }
    false
}

/// (mount point, source device) pairs, cached for the process lifetime
/// (refreshed every few seconds).
#[cfg(target_os = "linux")]
pub fn mount_sources() -> std::sync::Arc<Vec<(PathBuf, String)>> {
    use parking_lot::Mutex;
    use std::sync::{Arc, OnceLock};
    use std::time::Instant;
    static CACHE: OnceLock<Mutex<(Instant, Arc<Vec<(PathBuf, String)>>)>> = OnceLock::new();
    let c = CACHE.get_or_init(|| Mutex::new((Instant::now(), Arc::new(linux_mounts().into_iter().map(|m| (m.0, m.2)).collect()))));
    let mut g = c.lock();
    if g.0.elapsed().as_secs() > 5 {
        *g = (Instant::now(), Arc::new(linux_mounts().into_iter().map(|m| (m.0, m.2)).collect()));
    }
    g.1.clone()
}

#[cfg(target_os = "linux")]
fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 4], 8) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(target_os = "linux")]
pub fn list_volumes() -> Vec<VolumeInfo> {
    const SKIP_FS: &[&str] = &[
        "proc", "sysfs", "devtmpfs", "devpts", "tmpfs", "cgroup", "cgroup2", "securityfs", "pstore",
        "efivarfs", "bpf", "debugfs", "tracefs", "configfs", "fusectl", "mqueue", "hugetlbfs",
        "autofs", "binfmt_misc", "rpc_pipefs", "nsfs", "overlay", "squashfs", "ramfs", "selinuxfs",
        "fuse.portal", "fuse.gvfsd-fuse", "fuse.xdg-document-portal", "fuse.lxcfs",
    ];
    let mut out: Vec<VolumeInfo> = Vec::new();
    let mut seen_src = std::collections::HashSet::new();
    for (mp, fs, src) in linux_mounts() {
        if SKIP_FS.contains(&fs.as_str()) {
            continue;
        }
        let network = matches!(fs.as_str(), "nfs" | "nfs4" | "cifs" | "smb3" | "fuse.sshfs" | "9p");
        if !src.starts_with('/') && !network && !fs.starts_with("fuse") && fs != "zfs" {
            continue;
        }
        let mps = mp.to_string_lossy();
        if mps.starts_with("/snap/") || mps.starts_with("/var/lib/docker") {
            continue;
        }
        // btrfs subvolumes mount the same device repeatedly: keep the first (shortest) mount.
        if !seen_src.insert(src.clone()) && fs == "btrfs" {
            continue;
        }
        let Some(st) = volume_stats(&mp) else { continue };
        if st.total == 0 {
            continue;
        }
        let label = Path::new(&src).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        out.push(VolumeInfo {
            display: mps.to_string(),
            is_boot: mps == "/",
            is_local: !network,
            path: mp,
            label,
            fs_type: fs,
            total: st.total,
            free: st.free,
        });
    }
    out.sort_by(|a, b| b.is_boot.cmp(&a.is_boot).then(a.path.cmp(&b.path)));
    out
}

#[cfg(target_os = "macos")]
pub fn list_volumes() -> Vec<VolumeInfo> {
    let mut paths = vec![PathBuf::from("/")];
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        for e in rd.flatten() {
            let p = e.path();
            // The boot volume shows up in /Volumes as a symlink to "/".
            if std::fs::canonicalize(&p).map(|c| c == Path::new("/")).unwrap_or(false) {
                continue;
            }
            paths.push(p);
        }
    }
    paths
        .into_iter()
        .filter_map(|p| {
            let st = volume_stats(&p)?;
            Some(VolumeInfo {
                display: if p == Path::new("/") { format!("{} (/)", st.label) } else { p.to_string_lossy().to_string() },
                is_boot: p == Path::new("/"),
                is_local: !matches!(st.fs_type.as_str(), "smbfs" | "nfs" | "afpfs" | "webdav"),
                label: st.label,
                fs_type: st.fs_type,
                total: st.total,
                free: st.free,
                path: p,
            })
        })
        .collect()
}

#[cfg(all(unix, not(target_os = "linux"), not(target_os = "macos")))]
pub fn list_volumes() -> Vec<VolumeInfo> {
    let p = PathBuf::from("/");
    volume_stats(&p)
        .map(|st| {
            vec![VolumeInfo {
                display: "/".into(),
                is_boot: true,
                is_local: true,
                label: st.label,
                fs_type: st.fs_type,
                total: st.total,
                free: st.free,
                path: p,
            }]
        })
        .unwrap_or_default()
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn volume_root(p: &Path) -> String {
    let s = p.to_string_lossy();
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        format!("{}\\", &s[..2])
    } else {
        s.to_string()
    }
}

#[cfg(windows)]
pub fn volume_stats(p: &Path) -> Option<VolumeStats> {
    use windows_sys::Win32::Storage::FileSystem::*;
    let root = volume_root(p);
    let w = wide(&root);
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    if unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) } == 0 {
        return None;
    }
    let mut label = [0u16; 261];
    let mut fsname = [0u16; 261];
    let mut serial = 0u32;
    let mut maxlen = 0u32;
    let mut fsflags = 0u32;
    unsafe {
        GetVolumeInformationW(
            w.as_ptr(),
            label.as_mut_ptr(),
            261,
            &mut serial,
            &mut maxlen,
            &mut fsflags,
            fsname.as_mut_ptr(),
            261,
        )
    };
    let z = |b: &[u16]| String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]);
    Some(VolumeStats {
        total,
        free: avail,
        reserved: free.saturating_sub(avail),
        label: z(&label),
        fs_type: z(&fsname),
        serial: format!("{:04X}-{:04X}", serial >> 16, serial & 0xffff),
    })
}

#[cfg(windows)]
pub fn cluster_size(p: &Path) -> u64 {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static CACHE: OnceLock<parking_lot::Mutex<HashMap<String, u64>>> = OnceLock::new();
    let root = volume_root(p);
    let cache = CACHE.get_or_init(|| parking_lot::Mutex::new(HashMap::new()));
    if let Some(&c) = cache.lock().get(&root) {
        return c;
    }
    let w = wide(&root);
    let (mut spc, mut bps, mut fc, mut tc) = (0u32, 0u32, 0u32, 0u32);
    let c = unsafe {
        if windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceW(w.as_ptr(), &mut spc, &mut bps, &mut fc, &mut tc) != 0 {
            spc as u64 * bps as u64
        } else {
            4096
        }
    };
    cache.lock().insert(root, c);
    c
}

#[cfg(windows)]
pub fn list_volumes() -> Vec<VolumeInfo> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    let mask = unsafe { GetLogicalDrives() };
    let sysdrive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let mut out = Vec::new();
    for i in 0..26 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let ty = unsafe { GetDriveTypeW(wide(&root).as_ptr()) };
        let Some(st) = volume_stats(Path::new(&root)) else { continue };
        let disp = if st.label.is_empty() {
            format!("{letter}: ({})", st.fs_type)
        } else {
            format!("{letter}: [{}] ({})", st.label, st.fs_type)
        };
        out.push(VolumeInfo {
            path: PathBuf::from(&root),
            display: disp,
            is_boot: sysdrive.eq_ignore_ascii_case(&format!("{letter}:")),
            is_local: ty != 4,
            label: st.label,
            fs_type: st.fs_type,
            total: st.total,
            free: st.free,
        });
    }
    out
}
