//! Flat, cache-friendly representation of a scanned file system.
//!
//! Every file and folder is one [`Node`] in a single `Vec`, addressed by a
//! `u32` id. Names live in one shared byte arena. Children are stored in a
//! CSR-style index array, pre-sorted by size (descending) so the tree view,
//! the treemap and "top N" queries never need to sort at display time.

use rayon::prelude::*;
use std::collections::HashMap;

pub type NodeId = u32;
pub const NONE: NodeId = u32::MAX;

pub mod flags {
    pub const DIR: u32 = 1 << 0;
    pub const HIDDEN: u32 = 1 << 1;
    pub const READONLY: u32 = 1 << 2;
    pub const SYSTEM: u32 = 1 << 3;
    pub const ARCHIVE: u32 = 1 << 4;
    pub const COMPRESSED: u32 = 1 << 5;
    pub const SPARSE: u32 = 1 << 6;
    pub const SYMLINK: u32 = 1 << 7;
    pub const ENCRYPTED: u32 = 1 << 8;
    pub const OFFLINE: u32 = 1 << 9;
    pub const TEMPORARY: u32 = 1 << 10;
    pub const REPARSE: u32 = 1 << 11;
    pub const NOT_INDEXED: u32 = 1 << 12;
    /// Folder could not be read (permission denied etc.)
    pub const ACCESS_DENIED: u32 = 1 << 16;
    /// Hard link whose data was already counted elsewhere.
    pub const HARDLINK_DUP: u32 = 1 << 17;
    /// Removed from disk after the scan (deleted via UI or monitor).
    pub const DELETED: u32 = 1 << 18;
    /// A root entry of the scan (drive, folder or multi-select item).
    pub const ROOT: u32 = 1 << 19;
    /// Mount point / different device that was not descended into.
    pub const MOUNT: u32 = 1 << 20;
    /// Synthetic "free space" or other non-file entry.
    pub const SYNTHETIC: u32 = 1 << 21;
    /// NTFS high attribute bits 0x10000000..0x80000000, kept verbatim so
    /// exports carry the same attribute values as WizTree.
    pub const INDEX_VIEW: u32 = 1 << 22;
    pub const ATTR_29: u32 = 1 << 23;
    pub const ATTR_30: u32 = 1 << 24;
    pub const ATTR_31: u32 = 1 << 25;
    /// FILE_ATTRIBUTE_RECALL_ON_OPEN (0x40000) and FILE_ATTRIBUTE_PINNED
    /// (0x80000): rare flags WizTree prints as part of the attribute value.
    pub const RECALL_ON_OPEN: u32 = 1 << 13;
    pub const PINNED: u32 = 1 << 14;
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Node {
    pub size: u64,
    pub alloc: u64,
    pub mtime: i64,
    pub atime: i64,
    pub ctime: i64,
    /// File id: inode on Unix, MFT record number on NTFS.
    pub file_id: u64,
    pub name_off: u32,
    pub parent: NodeId,
    pub first_child: u32,
    pub child_count: u32,
    pub files: u32,
    pub folders: u32,
    pub flags: u32,
    pub name_len: u16,
    pub ext: u16,
}

impl Node {
    #[inline]
    pub fn is_dir(&self) -> bool {
        self.flags & flags::DIR != 0
    }
    #[inline]
    pub fn is_deleted(&self) -> bool {
        self.flags & flags::DELETED != 0
    }
    #[inline]
    pub fn metric(&self, alloc: bool) -> u64 {
        if alloc { self.alloc } else { self.size }
    }
}

/// Extension table: id 0 is "no extension" (and folders).
#[derive(Clone, Default)]
pub struct ExtTable {
    pub names: Vec<String>,
    pub map: HashMap<String, u16>,
}

impl ExtTable {
    pub fn new() -> Self {
        let mut t = ExtTable::default();
        t.names.push(String::new());
        t.map.insert(String::new(), 0);
        t
    }
    pub fn intern(&mut self, ext_lower: &str) -> u16 {
        if let Some(&id) = self.map.get(ext_lower) {
            return id;
        }
        if self.names.len() >= u16::MAX as usize {
            return 0;
        }
        let id = self.names.len() as u16;
        self.names.push(ext_lower.to_string());
        self.map.insert(ext_lower.to_string(), id);
        id
    }
    pub fn get(&self, ext_lower: &str) -> Option<u16> {
        self.map.get(ext_lower).copied()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ExtStat {
    pub ext: u16,
    pub size: u64,
    pub alloc: u64,
    pub files: u64,
}

/// Information about the volume(s) a scan covers.
#[derive(Clone, Debug, Default)]
pub struct VolumeStats {
    pub total: u64,
    pub free: u64,
    pub reserved: u64,
    pub label: String,
    pub fs_type: String,
    pub serial: String,
}

impl VolumeStats {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free + self.reserved)
    }
}

#[derive(Clone, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub names: Vec<u8>,
    /// Lower-cased copy of `names`, same offsets, used for searching.
    pub names_lower: Vec<u8>,
    pub children: Vec<NodeId>,
    pub roots: Vec<NodeId>,
    pub exts: ExtTable,
    pub ext_stats: Vec<ExtStat>,
    pub volume: VolumeStats,
    /// All file (non-dir) ids sorted by size descending.
    pub files_by_size: Vec<NodeId>,
    /// All folder ids sorted by size descending.
    pub dirs_by_size: Vec<NodeId>,
    pub denied: Vec<NodeId>,
    pub path_sep: char,
    /// Monotonic revision, bumped on every structural change.
    pub revision: u64,
}

pub fn ext_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(0) | None => "",
        Some(i) => &name[i + 1..],
    }
}

impl Tree {
    pub fn new() -> Self {
        Tree {
            exts: ExtTable::new(),
            path_sep: std::path::MAIN_SEPARATOR,
            ..Default::default()
        }
    }

    #[inline]
    pub fn name(&self, id: NodeId) -> &str {
        let n = &self.nodes[id as usize];
        let s = &self.names[n.name_off as usize..n.name_off as usize + n.name_len as usize];
        // Names are always stored as valid UTF-8 (lossily converted on insert).
        unsafe { std::str::from_utf8_unchecked(s) }
    }

    #[inline]
    pub fn name_lower(&self, id: NodeId) -> &[u8] {
        let n = &self.nodes[id as usize];
        &self.names_lower[n.name_off as usize..n.name_off as usize + n.name_len as usize]
    }

    pub fn push_name(&mut self, name: &str) -> (u32, u16) {
        let mut name = name;
        if name.len() > u16::MAX as usize {
            let mut end = u16::MAX as usize;
            while !name.is_char_boundary(end) {
                end -= 1;
            }
            name = &name[..end];
        }
        let off = self.names.len() as u32;
        self.names.extend_from_slice(name.as_bytes());
        push_lower(&mut self.names_lower, name);
        (off, name.len() as u16)
    }

    /// Add a node (children must be added after their parent).
    pub fn add_node(&mut self, parent: NodeId, name: &str, mut node: Node) -> NodeId {
        let (off, len) = self.push_name(name);
        node.name_off = off;
        node.name_len = len;
        node.parent = parent;
        node.first_child = 0;
        node.child_count = 0;
        if !node.is_dir() {
            let e = ext_of(name);
            node.ext = if e.is_empty() { 0 } else { self.exts.intern(&e.to_lowercase()) };
        }
        let id = self.nodes.len() as NodeId;
        self.nodes.push(node);
        id
    }

    #[inline]
    pub fn kids(&self, id: NodeId) -> &[NodeId] {
        let n = &self.nodes[id as usize];
        &self.children[n.first_child as usize..(n.first_child + n.child_count) as usize]
    }

    pub fn parent(&self, id: NodeId) -> NodeId {
        self.nodes[id as usize].parent
    }

    pub fn is_root(&self, id: NodeId) -> bool {
        self.nodes[id as usize].flags & flags::ROOT != 0
    }

    /// Full path of a node, e.g. `/home/user/file.txt` or `C:\Windows\x.dll`.
    pub fn path(&self, id: NodeId) -> String {
        let mut parts: Vec<NodeId> = Vec::with_capacity(16);
        let mut cur = id;
        while cur != NONE {
            parts.push(cur);
            if self.is_root(cur) {
                break;
            }
            cur = self.nodes[cur as usize].parent;
        }
        let mut s = String::with_capacity(128);
        for (i, &p) in parts.iter().rev().enumerate() {
            let name = self.name(p);
            if i > 0 && !s.ends_with(self.path_sep) && !s.ends_with('/') {
                s.push(self.path_sep);
            }
            s.push_str(name);
        }
        s
    }

    /// Lower-cased full path written into `buf` (used by path searches).
    pub fn path_lower_into(&self, id: NodeId, buf: &mut Vec<u8>, stack: &mut Vec<NodeId>) {
        buf.clear();
        stack.clear();
        let mut cur = id;
        while cur != NONE {
            stack.push(cur);
            if self.is_root(cur) {
                break;
            }
            cur = self.nodes[cur as usize].parent;
        }
        let sep = self.path_sep as u8;
        for (i, &p) in stack.iter().rev().enumerate() {
            if i > 0 && buf.last() != Some(&sep) && buf.last() != Some(&b'/') {
                buf.push(sep);
            }
            buf.extend_from_slice(self.name_lower(p));
        }
    }

    /// Ancestors from root down to (and including) `id`.
    pub fn chain(&self, id: NodeId) -> Vec<NodeId> {
        let mut v = vec![id];
        let mut cur = id;
        while !self.is_root(cur) {
            cur = self.nodes[cur as usize].parent;
            if cur == NONE {
                break;
            }
            v.push(cur);
        }
        v.reverse();
        v
    }

    pub fn total_size(&self) -> u64 {
        self.roots.iter().map(|&r| self.nodes[r as usize].size).sum()
    }
    pub fn total_alloc(&self) -> u64 {
        self.roots.iter().map(|&r| self.nodes[r as usize].alloc).sum()
    }

    /// Aggregate sizes/counts bottom-up, build sorted child index and
    /// derived indexes. Requires `parent < child` for every node.
    pub fn finalize(&mut self) {
        let n = self.nodes.len();
        // Reset aggregates on dirs.
        for node in self.nodes.iter_mut() {
            if node.is_dir() {
                node.size = 0;
                node.alloc = 0;
                node.files = 0;
                node.folders = 0;
            }
        }
        // WizTree's "max modified date": a folder shows the newest date of
        // itself and every *file* below it (sub folders' own dates don't
        // count). Verified row-for-row against WizTree 4.33's export.
        let max_mtime = MAX_MTIME.load(std::sync::atomic::Ordering::Relaxed);
        let mut fmax: Vec<i64> = if max_mtime { vec![i64::MIN; n] } else { Vec::new() };
        for i in (0..n).rev() {
            let node = self.nodes[i];
            if node.is_deleted() {
                continue;
            }
            let p = node.parent;
            if p == NONE || node.flags & flags::ROOT != 0 {
                continue;
            }
            if max_mtime {
                let c = if node.is_dir() { fmax[i] } else { node.mtime };
                if c > fmax[p as usize] {
                    fmax[p as usize] = c;
                }
            }
            let pn = &mut self.nodes[p as usize];
            pn.size += node.size;
            pn.alloc += node.alloc;
            if node.is_dir() {
                pn.files += node.files;
                pn.folders += node.folders + 1;
            } else if node.flags & flags::SYNTHETIC == 0 {
                pn.files += 1;
            }
        }
        if max_mtime {
            for (node, &m) in self.nodes.iter_mut().zip(&fmax) {
                if node.is_dir() && m > node.mtime {
                    node.mtime = m;
                }
            }
        }
        self.rebuild_children();
        self.rebuild_indexes();
        self.revision += 1;
    }

    pub fn rebuild_children(&mut self) {
        let n = self.nodes.len();
        let mut pairs: Vec<(NodeId, NodeId)> = (0..n as u32)
            .into_par_iter()
            .filter_map(|i| {
                let node = &self.nodes[i as usize];
                if node.parent == NONE || node.is_deleted() || node.flags & flags::ROOT != 0 {
                    None
                } else {
                    Some((node.parent, i))
                }
            })
            .collect();
        let nodes = &self.nodes;
        pairs.par_sort_unstable_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| nodes[b.1 as usize].size.cmp(&nodes[a.1 as usize].size))
                .then_with(|| a.1.cmp(&b.1))
        });
        for node in self.nodes.iter_mut() {
            node.first_child = 0;
            node.child_count = 0;
        }
        self.children.clear();
        self.children.reserve(pairs.len());
        let mut i = 0;
        while i < pairs.len() {
            let p = pairs[i].0;
            let start = self.children.len() as u32;
            let mut j = i;
            while j < pairs.len() && pairs[j].0 == p {
                self.children.push(pairs[j].1);
                j += 1;
            }
            let pn = &mut self.nodes[p as usize];
            pn.first_child = start;
            pn.child_count = (j - i) as u32;
            i = j;
        }
    }

    pub fn rebuild_indexes(&mut self) {
        let nodes = &self.nodes;
        let mut files: Vec<NodeId> = Vec::new();
        let mut dirs: Vec<NodeId> = Vec::new();
        rayon::join(
            || {
                files = (0..nodes.len() as u32)
                    .into_par_iter()
                    .filter(|&i| {
                        let n = &nodes[i as usize];
                        !n.is_dir() && !n.is_deleted() && n.flags & flags::SYNTHETIC == 0
                    })
                    .collect();
                files.par_sort_unstable_by(|a, b| {
                    nodes[*b as usize].size.cmp(&nodes[*a as usize].size).then(a.cmp(b))
                });
            },
            || {
                dirs = (0..nodes.len() as u32)
                    .into_par_iter()
                    .filter(|&i| {
                        let n = &nodes[i as usize];
                        n.is_dir() && !n.is_deleted()
                    })
                    .collect();
                dirs.par_sort_unstable_by(|a, b| {
                    nodes[*b as usize].size.cmp(&nodes[*a as usize].size).then(a.cmp(b))
                });
            },
        );
        self.files_by_size = files;
        self.dirs_by_size = dirs;
        self.denied = (0..nodes.len() as u32)
            .filter(|&i| nodes[i as usize].flags & flags::ACCESS_DENIED != 0)
            .collect();
        self.recompute_ext_stats();
    }

    pub fn recompute_ext_stats(&mut self) {
        let next = self.exts.names.len();
        let nodes = &self.nodes;
        let stats = self
            .files_by_size
            .par_chunks(1 << 16)
            .fold(
                || vec![(0u64, 0u64, 0u64); next],
                |mut acc, chunk| {
                    for &id in chunk {
                        let n = &nodes[id as usize];
                        let e = &mut acc[n.ext as usize];
                        e.0 += n.size;
                        e.1 += n.alloc;
                        e.2 += 1;
                    }
                    acc
                },
            )
            .reduce(
                || vec![(0u64, 0u64, 0u64); next],
                |mut a, b| {
                    for (x, y) in a.iter_mut().zip(b) {
                        x.0 += y.0;
                        x.1 += y.1;
                        x.2 += y.2;
                    }
                    a
                },
            );
        let mut v: Vec<ExtStat> = stats
            .into_iter()
            .enumerate()
            .filter(|(_, s)| s.2 > 0)
            .map(|(i, s)| ExtStat { ext: i as u16, size: s.0, alloc: s.1, files: s.2 })
            .collect();
        v.sort_unstable_by(|a, b| b.size.cmp(&a.size).then(a.ext.cmp(&b.ext)));
        self.ext_stats = v;
    }

    /// Subtract a removed subtree's totals from all its ancestors and mark it deleted.
    pub fn mark_deleted(&mut self, id: NodeId) {
        let node = self.nodes[id as usize];
        if node.is_deleted() {
            return;
        }
        let (size, alloc) = (node.size, node.alloc);
        let (files, folders) = if node.is_dir() { (node.files, node.folders + 1) } else { (1, 0) };
        let mut cur = node.parent;
        let mut child = id;
        while cur != NONE && !self.is_root(child) {
            let p = &mut self.nodes[cur as usize];
            p.size = p.size.saturating_sub(size);
            p.alloc = p.alloc.saturating_sub(alloc);
            p.files = p.files.saturating_sub(files);
            p.folders = p.folders.saturating_sub(folders);
            child = cur;
            cur = p.parent;
        }
        // Mark the whole subtree deleted.
        let mut stack = vec![id];
        while let Some(x) = stack.pop() {
            self.nodes[x as usize].flags |= flags::DELETED;
            let n = self.nodes[x as usize];
            for k in 0..n.child_count {
                stack.push(self.children[(n.first_child + k) as usize]);
            }
        }
        // Remove from parent's child slice.
        let p = node.parent;
        if p != NONE {
            let pn = self.nodes[p as usize];
            let start = pn.first_child as usize;
            let end = start + pn.child_count as usize;
            if let Some(pos) = self.children[start..end].iter().position(|&c| c == id) {
                self.children.copy_within(start + pos + 1..end, start + pos);
                self.nodes[p as usize].child_count -= 1;
            }
        }
        if let Some(pos) = self.roots.iter().position(|&r| r == id) {
            self.roots.remove(pos);
        }
        self.revision += 1;
    }

    /// Re-sort the children of every ancestor of `id` after size changes.
    pub fn resort_chain(&mut self, id: NodeId) {
        let mut cur = id;
        while cur != NONE {
            let n = self.nodes[cur as usize];
            let start = n.first_child as usize;
            let end = start + n.child_count as usize;
            let nodes = &self.nodes;
            self.children[start..end]
                .sort_unstable_by(|a, b| nodes[*b as usize].size.cmp(&nodes[*a as usize].size));
            if self.is_root(cur) {
                break;
            }
            cur = n.parent;
        }
    }

    pub fn rename(&mut self, id: NodeId, new_name: &str) {
        let (off, len) = self.push_name(new_name);
        let is_dir = self.nodes[id as usize].is_dir();
        let ext = if is_dir {
            0
        } else {
            let e = ext_of(new_name);
            if e.is_empty() { 0 } else { self.exts.intern(&e.to_lowercase()) }
        };
        let n = &mut self.nodes[id as usize];
        n.name_off = off;
        n.name_len = len;
        n.ext = ext;
        self.revision += 1;
    }

    /// Append `child` to `parent`'s child list (relocating the slice to the
    /// end of the index array; the old slice is simply abandoned).
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        let pn = self.nodes[parent as usize];
        let start = pn.first_child as usize;
        let end = start + pn.child_count as usize;
        if end == self.children.len() && pn.child_count > 0 {
            self.children.push(child);
        } else {
            let new_start = self.children.len();
            self.children.extend_from_within(start..end);
            self.children.push(child);
            self.nodes[parent as usize].first_child = new_start as u32;
        }
        self.nodes[parent as usize].child_count += 1;
    }

    /// Add size / count deltas to every ancestor of `id` (exclusive).
    pub fn propagate(&mut self, id: NodeId, dsize: i64, dalloc: i64, dfiles: i64, dfolders: i64) {
        let mut child = id;
        let mut cur = self.nodes[id as usize].parent;
        while cur != NONE && !self.is_root(child) {
            let p = &mut self.nodes[cur as usize];
            p.size = (p.size as i64 + dsize).max(0) as u64;
            p.alloc = (p.alloc as i64 + dalloc).max(0) as u64;
            p.files = (p.files as i64 + dfiles).max(0) as u32;
            p.folders = (p.folders as i64 + dfolders).max(0) as u32;
            child = cur;
            cur = p.parent;
        }
    }

    /// Copy the finalized tree `sub` (single root) under `parent` with the
    /// given name. Returns the id of the new subtree root.
    pub fn graft(&mut self, parent: NodeId, name: &str, sub: &Tree) -> NodeId {
        let Some(&sroot) = sub.roots.first() else { return NONE };
        let mut map = vec![NONE; sub.nodes.len()];
        for (i, n) in sub.nodes.iter().enumerate() {
            let mut node = *n;
            node.flags &= !flags::ROOT;
            let (p, nm) = if i as u32 == sroot { (parent, name) } else { (map[n.parent as usize], sub.name(i as u32)) };
            if p == NONE {
                continue;
            }
            let id = self.add_node(p, nm, node);
            // add_node recomputes ext from the name; keep aggregates from `sub`.
            let saved = (n.size, n.alloc, n.files, n.folders);
            let nn = &mut self.nodes[id as usize];
            nn.size = saved.0;
            nn.alloc = saved.1;
            nn.files = saved.2;
            nn.folders = saved.3;
            map[i] = id;
        }
        // Children index for the grafted nodes, preserving sub's order.
        for (i, _) in sub.nodes.iter().enumerate() {
            let nid = map[i];
            if nid == NONE {
                continue;
            }
            let kids = sub.kids(i as u32);
            let start = self.children.len() as u32;
            for &k in kids {
                if map[k as usize] != NONE {
                    self.children.push(map[k as usize]);
                }
            }
            let n = &mut self.nodes[nid as usize];
            n.first_child = start;
            n.child_count = self.children.len() as u32 - start;
        }
        let root = map[sroot as usize];
        self.append_child(parent, root);
        root
    }

    /// Find a node by full path (exact, case-sensitive component match).
    pub fn find_path(&self, path: &str) -> Option<NodeId> {
        for &r in &self.roots {
            let rp = self.path(r);
            let rp_trim = rp.trim_end_matches(['/', '\\']);
            let p_trim = path.trim_end_matches(['/', '\\']);
            if p_trim == rp_trim || (rp_trim.is_empty() && p_trim.is_empty()) {
                return Some(r);
            }
            let rest = if rp_trim.is_empty() {
                p_trim.strip_prefix(['/', '\\'])
            } else {
                p_trim.strip_prefix(rp_trim).and_then(|s| s.strip_prefix(['/', '\\']))
            };
            if let Some(rest) = rest {
                let mut cur = r;
                let mut ok = true;
                for comp in rest.split(['/', '\\']).filter(|c| !c.is_empty()) {
                    match self.kids(cur).iter().find(|&&k| self.name(k) == comp) {
                        Some(&k) => cur = k,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    return Some(cur);
                }
            }
        }
        None
    }
}

/// Whether folders report the newest modified time of their contents.
pub static MAX_MTIME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn push_lower(out: &mut Vec<u8>, s: &str) {
    if s.is_ascii() {
        out.extend(s.bytes().map(|b| b.to_ascii_lowercase()));
    } else {
        // Keep byte offsets identical to the original string: lower-case
        // per char only when the lowered form has the same UTF-8 length.
        for c in s.chars() {
            let mut buf = [0u8; 4];
            let orig = c.encode_utf8(&mut buf).len();
            let mut lc = c.to_lowercase();
            let l = lc.next().unwrap_or(c);
            if lc.next().is_none() && l.len_utf8() == orig {
                let mut b2 = [0u8; 4];
                out.extend_from_slice(l.encode_utf8(&mut b2).as_bytes());
            } else {
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample() -> Tree {
        let mut t = Tree::new();
        t.path_sep = '/';
        let root = t.add_node(NONE, "/r", Node { flags: flags::DIR | flags::ROOT, ..Default::default() });
        t.roots.push(root);
        let a = t.add_node(root, "a", Node { flags: flags::DIR, ..Default::default() });
        t.add_node(a, "x.txt", Node { size: 100, alloc: 4096, ..Default::default() });
        t.add_node(a, "y.MP4", Node { size: 1000, alloc: 4096, ..Default::default() });
        t.add_node(root, "z.txt", Node { size: 10, alloc: 4096, ..Default::default() });
        t.finalize();
        t
    }

    #[test]
    fn aggregate_and_sort() {
        let t = sample();
        let r = t.roots[0];
        assert_eq!(t.nodes[r as usize].size, 1110);
        assert_eq!(t.nodes[r as usize].files, 3);
        assert_eq!(t.nodes[r as usize].folders, 1);
        let kids = t.kids(r);
        assert_eq!(t.name(kids[0]), "a");
        assert_eq!(t.path(t.kids(kids[0])[0]), "/r/a/y.MP4");
        assert_eq!(t.exts.names[t.nodes[t.kids(kids[0])[0] as usize].ext as usize], "mp4");
        assert_eq!(t.files_by_size.len(), 3);
        assert_eq!(t.ext_stats[0].size, 1000);
    }

    #[test]
    fn delete_updates_ancestors() {
        let mut t = sample();
        let r = t.roots[0];
        let a = t.kids(r)[0];
        let y = t.kids(a)[0];
        t.mark_deleted(y);
        assert_eq!(t.nodes[r as usize].size, 110);
        assert_eq!(t.nodes[a as usize].files, 1);
        assert_eq!(t.kids(a).len(), 1);
        assert_eq!(t.find_path("/r/a/x.txt"), Some(t.kids(a)[0]));
    }
}
