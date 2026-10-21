//! Live file system monitoring: changed folders are re-read and patched into
//! the in-memory tree, so the views stay current without a rescan.

use crate::model::{Node, NodeId, Tree, flags, NONE};
use crate::scan;
use eframe::egui;
use notify::{RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Monitor {
    /// Filled in by the setup thread once every root is watched.
    _watcher: Arc<Mutex<Option<notify::RecommendedWatcher>>>,
    rx: crossbeam_channel::Receiver<PathBuf>,
    pending: HashSet<PathBuf>,
    last_event: Instant,
    ignore: Arc<Mutex<Vec<(PathBuf, Instant)>>>,
}

impl Monitor {
    pub fn start(roots: &[PathBuf], ctx: egui::Context) -> Option<Monitor> {
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if matches!(ev.kind, notify::EventKind::Access(_)) {
                    return;
                }
                for p in ev.paths {
                    let _ = tx.send(p);
                }
                ctx.request_repaint_after(Duration::from_millis(800));
            }
        })
        .ok()?;
        // A recursive inotify watch walks every folder under the roots. On a
        // whole drive just read through the MFT the directories are not in
        // the kernel cache, so this takes seconds to minutes on a spinning
        // disk; doing it on the UI thread froze the app at the end of a scan.
        let slot = Arc::new(Mutex::new(None));
        let (slot2, roots) = (slot.clone(), roots.to_vec());
        std::thread::Builder::new()
            .name("monitor-setup".into())
            .spawn(move || {
                let mut any = false;
                for r in &roots {
                    any |= w.watch(r, RecursiveMode::Recursive).is_ok();
                }
                if any {
                    *slot2.lock() = Some(w);
                }
            })
            .ok()?;
        Some(Monitor {
            _watcher: slot,
            rx,
            pending: HashSet::new(),
            last_event: Instant::now(),
            ignore: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Paths changed by the app itself (delete / rename) are already applied.
    pub fn ignore_paths(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut g = self.ignore.lock();
        let now = Instant::now();
        g.retain(|(_, t)| now.duration_since(*t) < Duration::from_secs(5));
        for p in paths {
            g.push((p, now));
        }
    }

    /// Apply pending changes. Returns true if the tree changed.
    pub fn apply(&mut self, tree: &mut Tree) -> bool {
        let mut got = false;
        while let Ok(p) = self.rx.try_recv() {
            let ignored = self.ignore.lock().iter().any(|(ip, _)| p.starts_with(ip) || ip == &p);
            if ignored {
                continue;
            }
            let dir = p.parent().map(Path::to_path_buf).unwrap_or(p);
            self.pending.insert(dir);
            got = true;
        }
        if got {
            self.last_event = Instant::now();
        }
        if self.pending.is_empty() || self.last_event.elapsed() < Duration::from_millis(700) {
            return false;
        }
        let mut dirs: Vec<PathBuf> = self.pending.drain().collect();
        dirs.sort();
        // Drop dirs contained in another pending dir? No: each dir refresh is
        // non-recursive for existing sub folders, so all are needed.
        let mut changed = false;
        for d in dirs {
            if let Some(id) = tree.find_path(&d.to_string_lossy()) {
                if tree.nodes[id as usize].is_dir() {
                    changed |= refresh_dir(tree, id, &d);
                }
            } else if let Some(parent) = d.parent() {
                // A new folder whose parent we know: refresh the parent.
                if let Some(pid) = tree.find_path(&parent.to_string_lossy()) {
                    changed |= refresh_dir(tree, pid, parent);
                }
            }
        }
        if changed {
            tree.rebuild_indexes();
            tree.revision += 1;
        }
        changed
    }
}

fn refresh_dir(tree: &mut Tree, dir: NodeId, path: &Path) -> bool {
    let entries = match scan::platform::read_dir(path) {
        Ok(e) => e,
        Err(_) => {
            if !path.exists() && !tree.is_root(dir) {
                tree.mark_deleted(dir);
                return true;
            }
            return false;
        }
    };
    let mut changed = false;
    let kids: Vec<NodeId> = tree.kids(dir).to_vec();
    let mut by_name: std::collections::HashMap<String, NodeId> = kids.iter().map(|&k| (tree.name(k).to_string(), k)).collect();
    for e in entries {
        match by_name.remove(&e.name) {
            Some(id) => {
                let n = tree.nodes[id as usize];
                if n.is_dir() != e.is_dir {
                    tree.mark_deleted(id);
                    add_entry(tree, dir, path, &e);
                    changed = true;
                } else if !e.is_dir && (n.size != e.size || n.alloc != e.alloc || n.mtime != e.mtime) {
                    let (ds, da) = (e.size as i64 - n.size as i64, e.alloc as i64 - n.alloc as i64);
                    let nn = &mut tree.nodes[id as usize];
                    nn.size = e.size;
                    nn.alloc = e.alloc;
                    nn.mtime = e.mtime;
                    nn.atime = e.atime;
                    tree.propagate(id, ds, da, 0, 0);
                    changed = true;
                }
            }
            None => {
                add_entry(tree, dir, path, &e);
                changed = true;
            }
        }
    }
    for (_, id) in by_name {
        tree.mark_deleted(id);
        changed = true;
    }
    if changed {
        tree.resort_chain(dir);
    }
    changed
}

fn add_entry(tree: &mut Tree, dir: NodeId, path: &Path, e: &scan::RawEntry) {
    if e.is_dir && e.flags & flags::SYMLINK == 0 {
        if let Some(sub) = scan::scan_subtree(&path.join(&e.name)) {
            let id = tree.graft(dir, &e.name, &sub);
            if id != NONE {
                let n = tree.nodes[id as usize];
                tree.propagate(id, n.size as i64, n.alloc as i64, n.files as i64, n.folders as i64 + 1);
            }
        }
        return;
    }
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
    }
    let id = tree.add_node(dir, &e.name, node);
    tree.append_child(dir, id);
    if e.is_dir {
        tree.propagate(id, 0, 0, 0, 1);
    } else {
        tree.propagate(id, e.size as i64, e.alloc as i64, 1, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_picks_up_changes() {
        let dir = std::env::temp_dir().join(format!("diskmosaic_mon_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::write(dir.join("a/one.bin"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("gone.bin"), vec![0u8; 50]).unwrap();
        let mut t = scan::scan(&[dir.clone()], &Default::default(), Default::default()).tree;
        let r = t.roots[0];
        assert_eq!(t.nodes[r as usize].size, 150);
        std::fs::remove_file(dir.join("gone.bin")).unwrap();
        std::fs::create_dir_all(dir.join("b/c")).unwrap();
        std::fs::write(dir.join("b/c/two.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.join("a/one.bin"), vec![0u8; 300]).unwrap();
        refresh_dir(&mut t, r, &dir);
        let a = t.find_path(&dir.join("a").to_string_lossy()).unwrap();
        refresh_dir(&mut t, a, &dir.join("a"));
        assert_eq!(t.nodes[r as usize].size, 1300);
        assert_eq!(t.nodes[r as usize].files, 2);
        assert!(t.find_path(&dir.join("b/c/two.bin").to_string_lossy()).is_some());
        assert_eq!(t.kids(r).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
