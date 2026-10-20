//! Direct NTFS Master File Table reader — the reason WizTree is fast on
//! Windows. Instead of walking directories, the whole `$MFT` is read
//! sequentially in large chunks and every FILE record is decoded in
//! parallel; the folder hierarchy is then rebuilt from parent references.
//!
//! The parser only needs `Read + Seek` over a volume, so it is platform
//! neutral (and tested against `mkntfs` images); opening `\\.\C:` is the
//! only Windows specific part and requires administrator rights.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::format::from_win_attrs;
use crate::model::{Node, Tree, flags, NONE};
use crate::scan::Progress;
#[cfg(any(windows, target_os = "linux"))]
use crate::scan::ScanResult;
use rayon::prelude::*;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::Ordering;

const ROOT_RECORD: u32 = 5;
const NO_REC: u32 = u32::MAX;

const ATTR_STANDARD_INFORMATION: u32 = 0x10;
const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_BITMAP: u32 = 0xB0;
const ATTR_END: u32 = 0xFFFF_FFFF;

#[derive(Debug, Clone)]
pub struct Boot {
    #[allow(dead_code)]
    pub bytes_per_sector: u64,
    pub cluster: u64,
    pub mft_lcn: u64,
    pub record_size: usize,
}

pub fn parse_boot(b: &[u8]) -> Option<Boot> {
    if b.len() < 512 || &b[3..11] != b"NTFS    " {
        return None;
    }
    let bps = u16::from_le_bytes([b[0x0B], b[0x0C]]) as u64;
    let spc_raw = b[0x0D];
    // Sectors per cluster > 128 are stored as a negative power of two.
    let spc = if spc_raw <= 0x80 { spc_raw as u64 } else { 1u64 << (256 - spc_raw as u32) };
    let cluster = bps * spc;
    let mft_lcn = u64::from_le_bytes(b[0x30..0x38].try_into().ok()?);
    let cpr = b[0x40] as i8;
    let record_size = if cpr > 0 { cpr as usize * cluster as usize } else { 1usize << (-(cpr as i32)) };
    if bps == 0 || cluster == 0 || !(256..=65536).contains(&record_size) {
        return None;
    }
    Some(Boot { bytes_per_sector: bps, cluster, mft_lcn, record_size })
}

/// Decode a non-resident attribute's mapping pairs into (lcn, clusters)
/// runs. Sparse runs get `lcn == u64::MAX`.
pub fn decode_runs(mut p: &[u8]) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut lcn: i64 = 0;
    while let Some(&h) = p.first() {
        if h == 0 {
            break;
        }
        let (ls, os) = ((h & 0x0F) as usize, (h >> 4) as usize);
        if p.len() < 1 + ls + os || ls == 0 || ls > 8 || os > 8 {
            break;
        }
        let mut len: u64 = 0;
        for i in 0..ls {
            len |= (p[1 + i] as u64) << (8 * i);
        }
        if os == 0 {
            out.push((u64::MAX, len));
        } else {
            let mut off: i64 = 0;
            for i in 0..os {
                off |= (p[1 + ls + i] as i64) << (8 * i);
            }
            // Sign extend.
            let shift = 64 - 8 * os as u32;
            off = (off << shift) >> shift;
            lcn += off;
            out.push((lcn as u64, len));
        }
        p = &p[1 + ls + os..];
    }
    out
}

/// Apply the update sequence array; false if the record is torn.
pub fn fixup(rec: &mut [u8]) -> bool {
    if rec.len() < 48 || &rec[0..4] != b"FILE" {
        return false;
    }
    let usa_off = u16::from_le_bytes([rec[4], rec[5]]) as usize;
    let usa_cnt = u16::from_le_bytes([rec[6], rec[7]]) as usize;
    if usa_cnt == 0 || usa_off + usa_cnt * 2 > rec.len() {
        return false;
    }
    let usn = [rec[usa_off], rec[usa_off + 1]];
    for i in 1..usa_cnt {
        let end = i * 512;
        if end > rec.len() {
            break;
        }
        if rec[end - 2..end] != usn {
            return false;
        }
        rec[end - 2] = rec[usa_off + 2 * i];
        rec[end - 1] = rec[usa_off + 2 * i + 1];
    }
    true
}

#[derive(Default, Clone, Debug)]
pub struct Rec {
    pub no: u32,
    /// Base record for extension records (NO_REC for base records).
    pub base: u32,
    pub parent: u32,
    pub name: String,
    /// Name quality: Win32 names beat DOS 8.3 names.
    pub name_rank: u8,
    pub is_dir: bool,
    pub attrs: u32,
    pub size: u64,
    pub alloc: u64,
    /// Clusters allocated to named streams (alternate data streams), counted
    /// from their runs so sparse placeholders ($BadClus:$Bad) count as 0.
    /// WizTree adds these to the file's allocated size, not its size.
    pub ads_clusters: u64,
    pub has_data: bool,
    pub mtime: i64,
    pub atime: i64,
    pub ctime: i64,
    pub has_std: bool,
}

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn rd64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

pub fn filetime_to_unix(ft: u64) -> i64 {
    // 0 is a real NTFS timestamp (1601-01-01), not "unknown": $MFT and a few
    // other system records use it, and WizTree prints it as such.
    (ft / 10_000_000) as i64 - 11_644_473_600
}

/// Decode one (fixed-up) FILE record. Returns None for free records.
pub fn parse_record(no: u32, rec: &[u8]) -> Option<Rec> {
    let rflags = rd16(rec, 0x16);
    if rflags & 0x01 == 0 {
        return None;
    }
    let base_ref = rd64(rec, 0x20) & 0x0000_FFFF_FFFF_FFFF;
    let mut r = Rec {
        no,
        base: if base_ref == 0 { NO_REC } else { base_ref as u32 },
        parent: NO_REC,
        is_dir: rflags & 0x02 != 0,
        ..Default::default()
    };
    let used = (rd32(rec, 0x18) as usize).min(rec.len());
    let mut off = rd16(rec, 0x14) as usize;
    while off + 16 <= used {
        let ty = rd32(rec, off);
        if ty == ATTR_END {
            break;
        }
        let len = rd32(rec, off + 4) as usize;
        if len < 16 || off + len > used {
            break;
        }
        let a = &rec[off..off + len];
        let nonres = a[8] != 0;
        let name_len = a[9];
        let aflags = rd16(a, 0x0C);
        match ty {
            ATTR_STANDARD_INFORMATION if !nonres && a.len() >= 0x18 => {
                let vo = rd16(a, 0x14) as usize;
                let vl = rd32(a, 0x10) as usize;
                if vo + vl <= a.len() && vl >= 36 {
                    let v = &a[vo..vo + vl];
                    r.ctime = filetime_to_unix(rd64(v, 0));
                    r.mtime = filetime_to_unix(rd64(v, 8));
                    r.atime = filetime_to_unix(rd64(v, 24));
                    r.attrs = rd32(v, 32);
                    r.has_std = true;
                }
            }
            ATTR_FILE_NAME if !nonres && a.len() >= 0x18 => {
                let vo = rd16(a, 0x14) as usize;
                let vl = rd32(a, 0x10) as usize;
                if vo + vl <= a.len() && vl >= 66 {
                    let v = &a[vo..vo + vl];
                    let ns = v[65];
                    // 0 POSIX, 1 Win32, 2 DOS, 3 Win32+DOS.
                    let rank = match ns {
                        1 | 3 => 3,
                        0 => 2,
                        _ => 1,
                    };
                    if rank > r.name_rank {
                        let n = v[64] as usize;
                        if 66 + n * 2 <= v.len() {
                            let units: Vec<u16> = (0..n).map(|i| rd16(v, 66 + i * 2)).collect();
                            r.name = String::from_utf16_lossy(&units);
                            r.name_rank = rank;
                            r.parent = (rd64(v, 0) & 0x0000_FFFF_FFFF_FFFF) as u32;
                            if !r.has_std {
                                r.ctime = filetime_to_unix(rd64(v, 8));
                                r.mtime = filetime_to_unix(rd64(v, 16));
                                r.atime = filetime_to_unix(rd64(v, 32));
                                r.attrs = rd32(v, 56);
                            }
                        }
                    }
                }
            }
            // Only the unnamed (default) stream counts as the file's size.
            ATTR_DATA if name_len == 0 => {
                if nonres {
                    if a.len() >= 0x40 && rd64(a, 0x10) == 0 {
                        r.size = rd64(a, 0x30);
                        let compressed_or_sparse = aflags & 0x8001 != 0;
                        r.alloc = if compressed_or_sparse && a.len() >= 0x48 { rd64(a, 0x40) } else { rd64(a, 0x28) };
                        r.has_data = true;
                    }
                } else if a.len() >= 0x18 {
                    r.size = rd32(a, 0x10) as u64;
                    r.alloc = 0;
                    r.has_data = true;
                }
            }
            // Named streams: only their (non-resident) clusters count.
            ATTR_DATA if nonres && a.len() >= 0x40 => {
                let ro = (rd16(a, 0x20) as usize).min(a.len());
                r.ads_clusters += decode_runs(&a[ro..]).iter().filter(|r| r.0 != u64::MAX).map(|r| r.1).sum::<u64>();
            }
            ATTR_ATTRIBUTE_LIST => {}
            _ => {}
        }
        off += len;
    }
    Some(r)
}

/// Where the MFT lives on the volume.
#[derive(Debug, Clone)]
pub struct MftLayout {
    pub boot: Boot,
    /// Byte extents in MFT order: (volume byte offset, length). Sparse
    /// extents have offset `u64::MAX` and read as zeros.
    pub extents: Vec<(u64, u64)>,
    /// Bytes of MFT data (`$MFT`'s real size).
    pub size: u64,
}

fn read_at<R: Read + Seek>(vol: &mut R, off: u64, buf: &mut [u8]) -> io::Result<()> {
    vol.seek(SeekFrom::Start(off))?;
    vol.read_exact(buf)
}

/// Unnamed $DATA pieces (start vcn, runs) and $ATTRIBUTE_LIST pointers
/// (start vcn, record number) of a FILE record.
#[allow(clippy::type_complexity)]
fn data_pieces(rec: &[u8]) -> (Vec<(u64, Vec<(u64, u64)>)>, u64, Vec<(u64, u32)>) {
    let mut pieces = Vec::new();
    let mut size = 0;
    let mut list = Vec::new();
    let used = (rd32(rec, 0x18) as usize).min(rec.len());
    let mut off = rd16(rec, 0x14) as usize;
    while off + 16 <= used {
        let ty = rd32(rec, off);
        if ty == ATTR_END {
            break;
        }
        let len = rd32(rec, off + 4) as usize;
        if len < 16 || off + len > used {
            break;
        }
        let a = &rec[off..off + len];
        if ty == ATTR_DATA && a[8] != 0 && a[9] == 0 && a.len() >= 0x40 {
            let vcn = rd64(a, 0x10);
            if vcn == 0 {
                size = rd64(a, 0x30);
            }
            let ro = (rd16(a, 0x20) as usize).min(a.len());
            pieces.push((vcn, decode_runs(&a[ro..])));
        } else if ty == ATTR_ATTRIBUTE_LIST && a[8] == 0 && a.len() >= 0x18 {
            let vo = rd16(a, 0x14) as usize;
            let vl = rd32(a, 0x10) as usize;
            if vo + vl <= a.len() {
                let v = &a[vo..vo + vl];
                let mut p = 0;
                while p + 0x1A <= v.len() {
                    let et = rd32(v, p);
                    let el = rd16(v, p + 4) as usize;
                    if el < 0x1A {
                        break;
                    }
                    if et == ATTR_DATA && v[p + 6] == 0 {
                        let no = (rd64(v, p + 0x10) & 0x0000_FFFF_FFFF_FFFF) as u32;
                        if no != 0 {
                            list.push((rd64(v, p + 8), no));
                        }
                    }
                    p += el;
                }
            }
        }
        off += len;
    }
    (pieces, size, list)
}

/// Locate the MFT: boot sector, record 0 and, for a fragmented MFT, the
/// extension records its `$ATTRIBUTE_LIST` points to.
pub fn mft_layout<R: Read + Seek>(vol: &mut R) -> io::Result<MftLayout> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    let mut boot_buf = vec![0u8; 512];
    read_at(vol, 0, &mut boot_buf)?;
    let boot = parse_boot(&boot_buf).ok_or_else(|| bad("not an NTFS volume"))?;
    let rs = boot.record_size;
    let mut rec0 = vec![0u8; rs];
    read_at(vol, boot.mft_lcn * boot.cluster, &mut rec0)?;
    if !fixup(&mut rec0) {
        return Err(bad("could not decode the first MFT record"));
    }
    let (mut pieces, size, list) = data_pieces(&rec0);
    // Runs known so far, used to find extension records.
    let to_extents = |pieces: &[(u64, Vec<(u64, u64)>)]| -> Vec<(u64, u64)> {
        let mut sorted: Vec<&(u64, Vec<(u64, u64)>)> = pieces.iter().collect();
        sorted.sort_by_key(|p| p.0);
        sorted
            .iter()
            .flat_map(|p| p.1.iter())
            .map(|&(lcn, n)| (if lcn == u64::MAX { u64::MAX } else { lcn * boot.cluster }, n * boot.cluster))
            .collect()
    };
    for (vcn, no) in list {
        if pieces.iter().any(|p| p.0 == vcn) {
            continue;
        }
        let ext = to_extents(&pieces);
        let Some(off) = record_offset(&ext, no as u64 * rs as u64) else { continue };
        let mut r = vec![0u8; rs];
        read_at(vol, off, &mut r)?;
        if fixup(&mut r) {
            let (more, _, _) = data_pieces(&r);
            pieces.extend(more.into_iter().filter(|p| p.0 == vcn));
        }
    }
    if pieces.is_empty() || size == 0 {
        return Err(bad("no $MFT data runs"));
    }
    Ok(MftLayout { extents: to_extents(&pieces), boot, size })
}

/// The MFT's own `$BITMAP` (one bit per record, 1 = in use).
pub fn mft_bitmap<R: Read + Seek>(vol: &mut R, l: &MftLayout) -> io::Result<Vec<u8>> {
    let rs = l.boot.record_size;
    let mut rec0 = vec![0u8; rs];
    read_at(vol, l.extents[0].0, &mut rec0)?;
    if !fixup(&mut rec0) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad $MFT record"));
    }
    let used = (rd32(&rec0, 0x18) as usize).min(rs);
    let mut off = rd16(&rec0, 0x14) as usize;
    while off + 16 <= used {
        let ty = rd32(&rec0, off);
        let len = rd32(&rec0, off + 4) as usize;
        if ty == ATTR_END || len < 16 || off + len > used {
            break;
        }
        let a = &rec0[off..off + len];
        if ty == ATTR_BITMAP && a[9] == 0 {
            if a[8] == 0 {
                let vo = rd16(a, 0x14) as usize;
                let vl = rd32(a, 0x10) as usize;
                return Ok(a[vo.min(a.len())..(vo + vl).min(a.len())].to_vec());
            }
            let size = rd64(a, 0x30) as usize;
            let ro = (rd16(a, 0x20) as usize).min(a.len());
            let mut out = Vec::with_capacity(size);
            for (lcn, n) in decode_runs(&a[ro..]) {
                let bytes = (n * l.boot.cluster) as usize;
                let start = out.len();
                out.resize(start + bytes, 0);
                if lcn != u64::MAX {
                    read_at(vol, lcn * l.boot.cluster, &mut out[start..])?;
                }
            }
            out.truncate(size);
            return Ok(out);
        }
        off += len;
    }
    Err(io::Error::new(io::ErrorKind::InvalidData, "no $MFT bitmap"))
}

fn record_offset(extents: &[(u64, u64)], mut pos: u64) -> Option<u64> {
    for &(off, len) in extents {
        if pos < len {
            return if off == u64::MAX { None } else { Some(off + pos) };
        }
        pos -= len;
    }
    None
}

/// Sequential reader over the MFT's extents (sparse ranges read as zeros).
/// Ranges known to hold only free records are skipped (returned as zeros)
/// and, on Unix, the kernel is asked to prefetch ahead of the read position
/// so the disk never idles between requests.
pub struct ExtentReader<R> {
    vol: R,
    extents: Vec<(u64, u64)>,
    idx: usize,
    pos: u64,
    left: u64,
    /// Position in the MFT byte stream.
    mpos: u64,
    /// Free ranges in MFT byte positions, sorted: (start, end).
    skip: Vec<(u64, u64)>,
    skip_idx: usize,
    #[cfg(target_os = "linux")]
    fd: Option<i32>,
    /// Absolute volume offset prefetched up to (current extent).
    ahead: u64,
}

/// Skip only free runs at least this long; shorter gaps cost more as a
/// seek than reading through them.
const MIN_SKIP: u64 = 4 << 20;
#[cfg(target_os = "linux")]
const PREFETCH: u64 = 64 << 20;

impl<R: Read + Seek> ExtentReader<R> {
    pub fn new(vol: R, layout: &MftLayout) -> Self {
        ExtentReader {
            vol,
            extents: layout.extents.clone(),
            idx: 0,
            pos: 0,
            left: layout.size,
            mpos: 0,
            skip: Vec::new(),
            skip_idx: 0,
            #[cfg(target_os = "linux")]
            fd: None,
            ahead: 0,
        }
    }

    /// Use the MFT bitmap to skip long runs of free records.
    pub fn with_bitmap(mut self, bitmap: &[u8], rs: usize) -> Self {
        let total = (self.left / rs as u64) as usize;
        let used = |i: usize| i / 8 < bitmap.len() && bitmap[i / 8] >> (i % 8) & 1 == 1;
        let mut i = 0;
        while i < total {
            if used(i) {
                i += 1;
                continue;
            }
            let start = i;
            while i < total && !used(i) {
                i += 1;
            }
            let (a, b) = (start as u64 * rs as u64, i as u64 * rs as u64);
            if b - a >= MIN_SKIP {
                self.skip.push((a, b));
            }
        }
        self
    }

    #[cfg(target_os = "linux")]
    pub fn with_prefetch(mut self, fd: i32) -> Self {
        self.fd = Some(fd);
        self
    }

    #[cfg(target_os = "linux")]
    fn prefetch(&mut self, abs: u64, extent_end: u64) {
        let Some(fd) = self.fd else { return };
        if self.ahead < abs {
            self.ahead = abs;
        }
        // Keep PREFETCH bytes requested beyond the read position, in 16 MiB
        // steps, never past the extent or into the next skipped range.
        let mut limit = (abs + PREFETCH).min(extent_end);
        if let Some(&(s, _)) = self.skip.get(self.skip_idx) {
            let s_abs = abs + (s.saturating_sub(self.mpos));
            limit = limit.min(s_abs.max(abs));
        }
        if limit > self.ahead + (16 << 20) || (limit == extent_end && limit > self.ahead) {
            // SAFETY: advisory call on a valid descriptor.
            unsafe { libc::posix_fadvise(fd, self.ahead as i64, (limit - self.ahead) as i64, libc::POSIX_FADV_WILLNEED) };
            self.ahead = limit;
        }
    }
}

impl<R: Read + Seek> Read for ExtentReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.idx < self.extents.len() && self.pos >= self.extents[self.idx].1 {
            self.idx += 1;
            self.pos = 0;
            self.ahead = 0;
        }
        if self.idx >= self.extents.len() || self.left == 0 {
            return Ok(0);
        }
        while self.skip_idx < self.skip.len() && self.skip[self.skip_idx].1 <= self.mpos {
            self.skip_idx += 1;
        }
        let (off, len) = self.extents[self.idx];
        let mut n = (buf.len() as u64).min(len - self.pos).min(self.left);
        let in_skip = match self.skip.get(self.skip_idx) {
            Some(&(s, e)) if s <= self.mpos => {
                n = n.min(e - self.mpos);
                true
            }
            Some(&(s, _)) => {
                n = n.min(s - self.mpos);
                false
            }
            None => false,
        };
        let n = n as usize;
        if off == u64::MAX || in_skip {
            buf[..n].fill(0);
        } else {
            let abs = off + self.pos;
            #[cfg(target_os = "linux")]
            self.prefetch(abs, off + len);
            if self.vol.stream_position()? != abs {
                self.vol.seek(SeekFrom::Start(abs))?;
            }
            self.vol.read_exact(&mut buf[..n])?;
        }
        self.pos += n as u64;
        self.left -= n as u64;
        self.mpos += n as u64;
        Ok(n)
    }
}

/// Reader over the MFT that skips free regions and prefetches.
fn mft_reader(vol: std::fs::File, layout: &MftLayout) -> ExtentReader<std::fs::File> {
    let mut vol = vol;
    let bitmap = mft_bitmap(&mut vol, layout).ok();
    #[cfg(target_os = "linux")]
    let fd = std::os::fd::AsRawFd::as_raw_fd(&vol);
    let r = ExtentReader::new(vol, layout);
    let r = match bitmap {
        Some(b) => r.with_bitmap(&b, layout.boot.record_size),
        None => r,
    };
    #[cfg(target_os = "linux")]
    let r = r.with_prefetch(fd);
    r
}

fn read_full<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match r.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(got)
}

/// Decode a stream of raw MFT records. Reading and parsing overlap: one
/// thread keeps the disk busy with large sequential reads while the rayon
/// pool decodes the previous chunk.
pub fn parse_stream<R: Read + Send>(mut src: R, rs: usize, progress: Option<&Progress>) -> io::Result<Vec<Rec>> {
    const CHUNK: usize = 16 << 20;
    let chunk = CHUNK / rs * rs;
    let (full_tx, full_rx) = crossbeam_channel::bounded::<(u64, Vec<u8>, usize)>(2);
    let (free_tx, free_rx) = crossbeam_channel::unbounded::<Vec<u8>>();
    for _ in 0..3 {
        let _ = free_tx.send(vec![0u8; chunk]);
    }
    let cancelled = || progress.is_some_and(|p| p.cancelled());
    std::thread::scope(|s| {
        let reader = s.spawn(move || -> io::Result<()> {
            let mut next: u64 = 0;
            while let Ok(mut buf) = free_rx.recv() {
                let n = read_full(&mut src, &mut buf)?;
                if n == 0 {
                    break;
                }
                let first = next;
                next += (n / rs) as u64;
                if full_tx.send((first, buf, n)).is_err() || n < chunk {
                    break;
                }
            }
            Ok(())
        });
        let mut recs: Vec<Rec> = Vec::new();
        for (first, mut buf, n) in full_rx.iter() {
            if cancelled() {
                break;
            }
            let count = n / rs;
            let mut parsed: Vec<Rec> = buf[..count * rs]
                .par_chunks_mut(rs)
                .enumerate()
                .filter_map(|(i, r)| if fixup(r) { parse_record((first + i as u64) as u32, r) } else { None })
                .collect();
            if let Some(p) = progress {
                let (mut files, mut dirs, mut bytes) = (0u64, 0u64, 0u64);
                for r in parsed.iter().filter(|r| r.base == NO_REC) {
                    if r.is_dir {
                        dirs += 1;
                    } else {
                        files += 1;
                        bytes += r.alloc.max(r.size);
                    }
                }
                p.files.fetch_add(files, Ordering::Relaxed);
                p.folders.fetch_add(dirs, Ordering::Relaxed);
                p.bytes.fetch_add(bytes, Ordering::Relaxed);
            }
            recs.append(&mut parsed);
            let _ = free_tx.send(buf);
        }
        drop(free_tx);
        drop(full_rx);
        if cancelled() {
            let _ = reader.join();
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        reader.join().unwrap_or_else(|_| Err(io::Error::other("reader panicked")))?;
        Ok(recs)
    })
}

/// Read the whole MFT of an NTFS volume and decode every record.
#[cfg_attr(not(test), allow(dead_code))]
pub fn read_mft<R: Read + Seek + Send>(mut vol: R, progress: Option<&Progress>) -> io::Result<(Boot, Vec<Rec>)> {
    let layout = mft_layout(&mut vol)?;
    let rs = layout.boot.record_size;
    let recs = parse_stream(ExtentReader::new(vol, &layout), rs, progress)?;
    Ok((layout.boot, recs))
}

/// `read_mft` for a real device: skips free regions and prefetches.
pub fn read_mft_file(mut vol: std::fs::File, progress: Option<&Progress>) -> io::Result<(Boot, Vec<Rec>)> {
    let layout = mft_layout(&mut vol)?;
    let rs = layout.boot.record_size;
    let recs = parse_stream(mft_reader(vol, &layout), rs, progress)?;
    Ok((layout.boot, recs))
}

const STREAM_MAGIC: &[u8; 8] = b"WTMFT01\0";

/// Privileged helper side: write the raw MFT of `dev` to `out`, prefixed by
/// a small header. Only NTFS volumes are accepted.
pub fn dump_mft<W: io::Write>(dev: &std::path::Path, out: &mut W) -> io::Result<()> {
    let mut vol = std::fs::File::open(dev)?;
    let layout = mft_layout(&mut vol)?;
    let mut hdr = Vec::with_capacity(32);
    hdr.extend_from_slice(STREAM_MAGIC);
    hdr.extend_from_slice(&(layout.boot.record_size as u64).to_le_bytes());
    hdr.extend_from_slice(&layout.boot.cluster.to_le_bytes());
    hdr.extend_from_slice(&layout.size.to_le_bytes());
    out.write_all(&hdr)?;
    let mut r = mft_reader(vol, &layout);
    let mut buf = vec![0u8; 8 << 20];
    loop {
        let n = read_full(&mut r, &mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
    }
    out.flush()
}

/// Unprivileged side: decode a stream produced by `dump_mft`.
pub fn read_mft_stream<R: Read + Send>(mut src: R, progress: Option<&Progress>) -> io::Result<(u64, Vec<Rec>)> {
    let mut hdr = [0u8; 32];
    src.read_exact(&mut hdr)?;
    if &hdr[0..8] != STREAM_MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad MFT stream"));
    }
    let rs = u64::from_le_bytes(hdr[8..16].try_into().unwrap()) as usize;
    if !(256..=65536).contains(&rs) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad record size"));
    }
    let cluster = u64::from_le_bytes(hdr[16..24].try_into().unwrap());
    Ok((cluster, parse_stream(src, rs, progress)?))
}

/// Build a `Tree` from decoded records. `root_name` labels record 5.
/// Build a tree rooted at MFT record `root_rec` (5 = volume root). Paths
/// use `sep`; the root is labelled `root_name`.
pub fn build_tree(mut recs: Vec<Rec>, root_rec: u32, root_name: &str, sep: char, cluster: u64) -> Tree {
    recs.sort_unstable_by_key(|r| r.no);
    let max_no = recs.last().map(|r| r.no as usize + 1).unwrap_or(0);
    // Index base records by number.
    let mut idx = vec![NO_REC; max_no];
    for (i, r) in recs.iter().enumerate() {
        if r.base == NO_REC {
            idx[r.no as usize] = i as u32;
        }
    }
    // Merge extension records (attribute lists split across records).
    let ext: Vec<usize> = (0..recs.len()).filter(|&i| recs[i].base != NO_REC).collect();
    for i in ext {
        let (base, e) = (recs[i].base as usize, recs[i].clone());
        let Some(&bi) = idx.get(base) else { continue };
        if bi == NO_REC {
            continue;
        }
        let b = &mut recs[bi as usize];
        if e.name_rank > b.name_rank {
            b.name = e.name;
            b.name_rank = e.name_rank;
            b.parent = e.parent;
        }
        if e.has_data && !b.has_data {
            b.size = e.size;
            b.alloc = e.alloc;
            b.has_data = true;
        }
        b.ads_clusters += e.ads_clusters;
    }

    // Children lists in CSR form, keyed by parent record number.
    let mut counts = vec![0u32; max_no + 1];
    for r in recs.iter().filter(|r| r.base == NO_REC && r.no != ROOT_RECORD && !r.name.is_empty()) {
        if (r.parent as usize) < max_no {
            counts[r.parent as usize] += 1;
        }
    }
    let mut start = vec![0u32; max_no + 1];
    let mut acc = 0u32;
    for i in 0..max_no {
        start[i] = acc;
        acc += counts[i];
    }
    start[max_no] = acc;
    let mut fill = start.clone();
    let mut kids = vec![0u32; acc as usize];
    for (i, r) in recs.iter().enumerate() {
        if r.base == NO_REC && r.no != ROOT_RECORD && !r.name.is_empty() && (r.parent as usize) < max_no {
            kids[fill[r.parent as usize] as usize] = i as u32;
            fill[r.parent as usize] += 1;
        }
    }

    let mut tree = Tree::new();
    tree.path_sep = sep;
    let root_ri = idx.get(root_rec as usize).copied().unwrap_or(NO_REC);
    let mut root = Node { flags: flags::DIR | flags::ROOT, file_id: root_rec as u64, ..Default::default() };
    if root_ri != NO_REC {
        let r = &recs[root_ri as usize];
        root.mtime = r.mtime;
        root.atime = r.atime;
        root.ctime = r.ctime;
        root.flags |= from_win_attrs(r.attrs);
    }
    let root_id = tree.add_node(NONE, root_name, root);
    tree.roots.push(root_id);

    // Breadth-first so parents always precede children.
    let mut queue: std::collections::VecDeque<(u32, u32)> = std::collections::VecDeque::new();
    queue.push_back((root_rec, root_id));
    let mut visited = vec![false; max_no];
    if (root_rec as usize) < max_no {
        visited[root_rec as usize] = true;
    }
    while let Some((rno, nid)) = queue.pop_front() {
        let (a, b) = (start[rno as usize] as usize, start[rno as usize + 1] as usize);
        for &ri in &kids[a..b] {
            let r = &recs[ri as usize];
            if visited[r.no as usize] {
                continue;
            }
            visited[r.no as usize] = true;
            let mut fl = from_win_attrs(r.attrs);
            if r.attrs & 0x400 != 0 {
                fl |= flags::REPARSE;
            }
            let is_dir = r.is_dir || r.attrs & 0x10 != 0;
            let mut node = Node {
                mtime: r.mtime,
                atime: r.atime,
                ctime: r.ctime,
                file_id: r.no as u64,
                flags: fl,
                ..Default::default()
            };
            if is_dir {
                node.flags |= flags::DIR;
            } else {
                node.size = r.size;
                // Resident data lives inside the MFT record: no clusters.
                node.alloc = r.alloc + r.ads_clusters * cluster;
            }
            let id = tree.add_node(nid, &r.name, node);
            if is_dir {
                queue.push_back((r.no, id));
            }
        }
    }
    tree.finalize();
    tree
}

/// Try the MFT fast path for a whole NTFS drive (`C:\`). Returns None when
/// not applicable (not a drive root, not NTFS, not elevated) so the caller
/// falls back to the directory walker.
#[cfg(windows)]
pub fn try_scan_windows(roots: &[std::path::PathBuf], _opts: &crate::scan::ScanOptions, progress: &std::sync::Arc<Progress>) -> Option<ScanResult> {
    if roots.len() != 1 {
        return None;
    }
    let s = roots[0].to_string_lossy().replace('/', "\\");
    let b = s.as_bytes();
    let is_drive = (b.len() == 2 || (b.len() == 3 && b[2] == b'\\')) && b[0].is_ascii_alphabetic() && b[1] == b':';
    if !is_drive {
        return None;
    }
    let letter = (b[0] as char).to_ascii_uppercase();
    let vinfo = crate::volumes::volume_stats(&roots[0])?;
    if !vinfo.fs_type.eq_ignore_ascii_case("NTFS") {
        return None;
    }
    let f = std::fs::File::open(format!("\\\\.\\{letter}:")).ok()?;
    let started = std::time::Instant::now();
    progress.expected.store(vinfo.used(), Ordering::Relaxed);
    let (boot, recs) = match read_mft_file(f, Some(progress)) {
        Ok(x) => x,
        Err(e) if e.kind() == io::ErrorKind::Interrupted => {
            return Some(ScanResult { tree: Tree::new(), elapsed: 0.0, cancelled: true, used_mft: true, note: None });
        }
        Err(_) => return None,
    };
    let mut tree = build_tree(recs, ROOT_RECORD, &format!("{letter}:\\"), '\\', boot.cluster);
    tree.volume = vinfo;
    Some(ScanResult { tree, elapsed: started.elapsed().as_secs_f64(), cancelled: false, used_mft: true, note: None })
}

/// An NTFS volume (or folder on one) that can be scanned through its MFT.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
pub struct NtfsTarget {
    pub dev: std::path::PathBuf,
    pub mount: std::path::PathBuf,
    /// MFT record of the scanned folder (NTFS drivers expose it as inode).
    pub root_rec: u32,
}

#[cfg(target_os = "linux")]
pub fn linux_target(root: &std::path::Path) -> Option<NtfsTarget> {
    use std::os::unix::fs::MetadataExt;
    let (mount, fs, src) = crate::volumes::mount_for(root)?;
    if !matches!(fs.as_str(), "ntfs3" | "ntfs" | "fuseblk" | "fuse.ntfs-3g") || !src.starts_with("/dev/") {
        return None;
    }
    // Both ntfs3 and ntfs-3g use the MFT record number as inode number;
    // the volume root is always record 5.
    if std::fs::metadata(&mount).ok()?.ino() & 0x0000_FFFF_FFFF_FFFF != ROOT_RECORD as u64 {
        return None;
    }
    let ino = std::fs::metadata(root).ok()?.ino() & 0x0000_FFFF_FFFF_FFFF;
    Some(NtfsTarget { dev: src.into(), mount, root_rec: ino as u32 })
}

/// Whether the current user can read the raw device without elevation.
#[cfg(target_os = "linux")]
pub fn can_read_directly(dev: &std::path::Path) -> bool {
    std::fs::File::open(dev).is_ok()
}

#[cfg(target_os = "linux")]
fn helper_command(how: &crate::scan::Elevation, dev: &std::path::Path) -> Option<std::process::Command> {
    use crate::scan::Elevation;
    use std::process::{Command, Stdio};
    // Tests run inside the test harness; point them at the real binary.
    let exe = match std::env::var_os("WT_HELPER_EXE") {
        Some(p) if cfg!(test) => p.into(),
        _ => std::env::current_exe().ok()?,
    };
    let mut c = match how {
        Elevation::None => return None,
        Elevation::Sudo(_) => {
            let mut c = Command::new("sudo");
            c.args(["-S", "-p", "", "--"]);
            c
        }
        Elevation::SudoTty => {
            let mut c = Command::new("sudo");
            c.arg("--");
            c
        }
    };
    c.arg(exe).arg("--mft-dump").arg(dev);
    c.stdout(Stdio::piped());
    c.stdin(if matches!(how, Elevation::Sudo(_)) { Stdio::piped() } else { Stdio::inherit() });
    c.stderr(if matches!(how, Elevation::SudoTty) { Stdio::inherit() } else { Stdio::piped() });
    Some(c)
}

/// Read the MFT records of `dev`, directly or through the privileged helper.
#[cfg(target_os = "linux")]
fn linux_records(dev: &std::path::Path, how: &crate::scan::Elevation, progress: &Progress) -> Result<(u64, Vec<Rec>), String> {
    match std::fs::File::open(dev) {
        Ok(f) => {
            // Large sequential reads; tell the kernel to read ahead hard.
            use std::os::fd::AsRawFd;
            // SAFETY: plain advisory call on a valid descriptor.
            unsafe { libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_SEQUENTIAL) };
            return read_mft_file(f, Some(progress)).map(|(b, r)| (b.cluster, r)).map_err(|e| e.to_string());
        }
        Err(e) if e.kind() != io::ErrorKind::PermissionDenied => return Err(e.to_string()),
        Err(_) => {}
    }
    let mut cmd = helper_command(how, dev).ok_or_else(|| "no permission to read the device".to_string())?;
    let mut child = cmd.spawn().map_err(|e| format!("could not start the helper: {e}"))?;
    if let (crate::scan::Elevation::Sudo(pw), Some(mut stdin)) = (how, child.stdin.take()) {
        use std::io::Write;
        let _ = writeln!(stdin, "{pw}");
    }
    let stdout = child.stdout.take().expect("piped stdout");
    let res = read_mft_stream(io::BufReader::with_capacity(4 << 20, stdout), Some(progress));
    let status = child.wait().map_err(|e| e.to_string())?;
    match res {
        Ok(x) if status.success() => Ok(x),
        Err(e) if e.kind() == io::ErrorKind::Interrupted => Err("cancelled".into()),
        _ => {
            let mut msg = String::new();
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut msg);
            }
            let msg = msg.trim();
            Err(if msg.is_empty() || msg.contains("incorrect password") || msg.contains("Sorry") {
                "authentication failed".into()
            } else {
                msg.to_string()
            })
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MftMode {
    /// Not NTFS / no access: walk folders.
    Skip,
    /// Read the MFT straight away.
    Now,
    /// Walk briefly first; read the MFT if the walk takes long.
    Race,
}

/// Decide how to scan `roots` on Linux. Whole NTFS volumes always use the
/// MFT (as WizTree does); folders do when the option is on, and on spinning
/// disks only when a short walk shows they are big.
#[cfg(target_os = "linux")]
pub fn linux_policy(roots: &[std::path::PathBuf], opts: &crate::scan::ScanOptions) -> MftMode {
    if roots.len() != 1 {
        return MftMode::Skip;
    }
    let Some(t) = linux_target(&roots[0]) else { return MftMode::Skip };
    if matches!(opts.elevation, crate::scan::Elevation::None) && !can_read_directly(&t.dev) {
        return MftMode::Skip;
    }
    let whole = std::fs::canonicalize(&roots[0]).ok().as_deref() == Some(t.mount.as_path());
    if whole || opts.mft_for_folders {
        MftMode::Now
    } else if crate::volumes::is_rotational(&t.dev) {
        MftMode::Race
    } else {
        MftMode::Skip
    }
}

/// MFT fast path on Linux. `Ok(None)`: not an NTFS target (walk normally);
/// `Err`: NTFS but the MFT could not be read (the caller walks instead).
#[cfg(target_os = "linux")]
pub fn try_scan_linux(
    roots: &[std::path::PathBuf],
    opts: &crate::scan::ScanOptions,
    progress: &std::sync::Arc<Progress>,
) -> Result<Option<ScanResult>, String> {
    if roots.len() != 1 {
        return Ok(None);
    }
    let Some(t) = linux_target(&roots[0]) else { return Ok(None) };
    let whole = std::fs::canonicalize(&roots[0]).ok().as_deref() == Some(t.mount.as_path());
    let started = std::time::Instant::now();
    let vol = crate::volumes::volume_stats(&roots[0]).unwrap_or_default();
    progress.expected.store(vol.used(), Ordering::Relaxed);
    if let Some(mut c) = progress.current.try_lock() {
        *c = format!("Reading the MFT of {}", t.dev.display());
    }
    let (cluster, recs) = match linux_records(&t.dev, &opts.elevation, progress) {
        Ok(r) => r,
        Err(e) if e == "cancelled" || e == "authentication failed" => {
            return Ok(Some(ScanResult { tree: Tree::new(), elapsed: 0.0, cancelled: true, used_mft: true, note: Some(e) }));
        }
        Err(e) => return Err(format!("Fast NTFS scan unavailable: {e}")),
    };
    let t_read = started.elapsed();
    let mut tree = build_tree(recs, t.root_rec, &crate::scan::display_root(&roots[0]), '/', cluster);
    if std::env::var_os("WT_TIMING").is_some() {
        eprintln!("mft read+parse {:?}, build_tree {:?}", t_read, started.elapsed() - t_read);
    }
    if !whole {
        tree.nodes[tree.roots[0] as usize].flags &= !flags::MOUNT;
    }
    tree.volume = vol;
    Ok(Some(ScanResult { tree, elapsed: started.elapsed().as_secs_f64(), cancelled: false, used_mft: true, note: None }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_decode() {
        // 0x21: 1 length byte, 2 offset bytes. Second run is relative, negative.
        let r = decode_runs(&[0x21, 0x10, 0x00, 0x01, 0x11, 0x05, 0xF0, 0x01, 0x08, 0x00]);
        assert_eq!(r, vec![(0x100, 0x10), (0x100 - 0x10, 5), (u64::MAX, 8)]);
    }

    #[test]
    fn zero_filetime_is_1601() {
        // $MFT's own record has zero timestamps; WizTree shows 1601-01-01.
        assert_eq!(filetime_to_unix(0), -11_644_473_600);
        assert_eq!(filetime_to_unix(116_444_736_000_000_000), 0);
    }

    /// Build a real NTFS image with `mkntfs` (if installed) and parse it.
    #[test]
    fn parses_mkntfs_image() {
        let Ok(mk) = which("mkntfs") else { return };
        let dir = std::env::temp_dir().join(format!("wt_ntfs_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("vol.img");
        std::fs::File::create(&img).unwrap().set_len(16 << 20).unwrap();
        let ok = std::process::Command::new(mk)
            .args(["-F", "-f", "-q", "-L", "WT"])
            .arg(&img)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return;
        }
        // Optionally add a file with ntfscp.
        let payload = dir.join("payload.bin");
        std::fs::write(&payload, vec![7u8; 100_000]).unwrap();
        let copied = which("ntfscp")
            .ok()
            .and_then(|cp| {
                std::process::Command::new(cp)
                    .arg("-f")
                    .arg(&img)
                    .arg(&payload)
                    .arg("/payload.bin")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .ok()
            })
            .is_some_and(|s| s.success());
        let mut f = std::fs::File::open(&img).unwrap();
        let (boot, recs) = read_mft(&mut f, None).unwrap();
        // The skipping / prefetching reader must decode the same records.
        let (_, recs2) = read_mft_file(std::fs::File::open(&img).unwrap(), None).unwrap();
        assert_eq!(recs.len(), recs2.len());
        assert!(recs.iter().zip(&recs2).all(|(a, b)| a.no == b.no && a.name == b.name && a.size == b.size));
        // And so must the helper stream.
        let mut stream = Vec::new();
        dump_mft(&img, &mut stream).unwrap();
        let (_, recs3) = read_mft_stream(&stream[..], None).unwrap();
        assert_eq!(recs.len(), recs3.len());
        let _ = boot.cluster;
        assert_eq!(boot.record_size, 1024);
        let tree = build_tree(recs, ROOT_RECORD, "X:\\", '\\', boot.cluster);
        let root = tree.roots[0];
        let names: Vec<&str> = tree.kids(root).iter().map(|&k| tree.name(k)).collect();
        assert!(names.contains(&"$MFT"), "{names:?}");
        assert!(names.contains(&"$Extend"), "{names:?}");
        if copied {
            let p = tree.kids(root).iter().copied().find(|&k| tree.name(k) == "payload.bin").expect("payload");
            assert_eq!(tree.nodes[p as usize].size, 100_000);
            assert!(tree.nodes[p as usize].alloc >= 100_000);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn which(bin: &str) -> Result<std::path::PathBuf, ()> {
        for d in std::env::var("PATH").unwrap_or_default().split(':').chain(["/usr/sbin", "/sbin"]) {
            let p = std::path::Path::new(d).join(bin);
            if p.is_file() {
                return Ok(p);
            }
        }
        Err(())
    }
}

#[cfg(test)]
mod layout_probe {
    /// `WT_NTFS_DEV=/dev/sda1 cargo test --release layout_probe -- --ignored --nocapture` (as root)
    #[test]
    #[ignore]
    fn print_layout() {
        let dev = std::env::var("WT_NTFS_DEV").unwrap();
        let mut f = std::fs::File::open(dev).unwrap();
        let l = super::mft_layout(&mut f).unwrap();
        println!("record {} cluster {} size {} extents {}", l.boot.record_size, l.boot.cluster, l.size, l.extents.len());
        for e in l.extents.iter().take(40) {
            println!("  off {:>14} len {:>12}", e.0, e.1);
        }
        let bm = super::mft_bitmap(&mut f, &l).unwrap();
        let total = (l.size / l.boot.record_size as u64) as usize;
        let used = (0..total).filter(|&i| bm[i / 8] >> (i % 8) & 1 == 1).count();
        println!("records {total} used {used}");
        // Free runs per 1 MiB block (1024 records).
        let per = (1 << 20) / l.boot.record_size;
        let blocks = total.div_ceil(per);
        let free_blocks = (0..blocks).filter(|b| (b * per..((b + 1) * per).min(total)).all(|i| bm[i / 8] >> (i % 8) & 1 == 0)).count();
        println!("1MiB blocks {blocks}, entirely free {free_blocks}");
    }
}

#[cfg(all(test, target_os = "linux"))]
mod helper_e2e {
    /// End-to-end check of the elevated helper path used by the GUI:
    /// `WT_NTFS_MOUNT=/mnt/x WT_SUDO_PW=... cargo test --release helper_e2e -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn scan_through_sudo_helper() {
        let mount = std::path::PathBuf::from(std::env::var("WT_NTFS_MOUNT").unwrap());
        let pw = std::env::var("WT_SUDO_PW").unwrap();
        let opts = crate::scan::ScanOptions { elevation: crate::scan::Elevation::Sudo(pw), ..Default::default() };
        let t = std::time::Instant::now();
        let res = crate::scan::scan(&[mount.clone()], &opts, Default::default());
        println!("used_mft {} cancelled {} note {:?} nodes {} in {:?}", res.used_mft, res.cancelled, res.note, res.tree.nodes.len(), t.elapsed());
        assert!(res.used_mft && !res.cancelled);
        // Wrong password must not fall back to a slow walk.
        let _ = std::process::Command::new("sudo").arg("-k").status();
        let bad = crate::scan::ScanOptions { elevation: crate::scan::Elevation::Sudo("definitely-wrong".into()), ..Default::default() };
        let r2 = crate::scan::scan(&[mount], &bad, Default::default());
        println!("bad pw: cancelled {} note {:?}", r2.cancelled, r2.note);
        assert!(r2.cancelled && r2.note.as_deref() == Some("authentication failed"));
    }
}
