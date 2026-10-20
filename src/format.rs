//! Number, size, date and attribute formatting.

use crate::model::flags;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SizeUnit {
    #[default]
    Dynamic,
    Bytes,
    KB,
    MB,
    GB,
    TB,
}

#[derive(Clone, Copy, Debug)]
pub struct Fmt {
    pub unit: SizeUnit,
    pub decimals: usize,
    /// 1024 (binary, Windows style) or 1000 (decimal, macOS style).
    pub base: u64,
}

impl Default for Fmt {
    fn default() -> Self {
        Fmt { unit: SizeUnit::Dynamic, decimals: 1, base: 1024 }
    }
}

pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    let len = s.len();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn fixed_thousands(v: f64, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, v);
    let (int, frac) = match s.find('.') {
        Some(i) => (&s[..i], &s[i..]),
        None => (&s[..], ""),
    };
    let n: u64 = int.parse().unwrap_or(0);
    format!("{}{}", thousands(n), frac)
}

impl Fmt {
    pub fn size(&self, bytes: u64) -> String {
        let b = self.base as f64;
        let v = bytes as f64;
        match self.unit {
            SizeUnit::Bytes => thousands(bytes),
            SizeUnit::KB => format!("{} KB", fixed_thousands(v / b, self.decimals)),
            SizeUnit::MB => format!("{} MB", fixed_thousands(v / b / b, self.decimals)),
            SizeUnit::GB => format!("{} GB", fixed_thousands(v / b / b / b, self.decimals)),
            SizeUnit::TB => format!("{} TB", fixed_thousands(v / b / b / b / b, self.decimals)),
            SizeUnit::Dynamic => {
                if bytes < self.base {
                    format!("{} Bytes", bytes)
                } else if v < b * b {
                    format!("{} KB", fixed_thousands(v / b, self.decimals))
                } else if v < b * b * b {
                    format!("{} MB", fixed_thousands(v / b / b, self.decimals))
                } else if v < b * b * b * b {
                    format!("{} GB", fixed_thousands(v / b / b / b, self.decimals))
                } else {
                    format!("{} TB", fixed_thousands(v / b / b / b / b, self.decimals))
                }
            }
        }
    }

}

/// Windows has no historical time-zone data: for dates before the modern DST
/// era it uses the zone's standard bias. chrono instead applies the pre-1900
/// local mean time (+5:53:28 in Kolkata), so those dates — in practice only
/// NTFS's 1601 "zero" FILETIME — use the standard offset. Taking the smaller
/// of the January and July offsets gives standard time in both hemispheres.
fn standard_offset() -> chrono::FixedOffset {
    use chrono::{Datelike, Local, TimeZone};
    static OFF: std::sync::OnceLock<chrono::FixedOffset> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| {
        let y = Local::now().year();
        match (
            Local.with_ymd_and_hms(y, 1, 15, 12, 0, 0).single(),
            Local.with_ymd_and_hms(y, 7, 15, 12, 0, 0).single(),
        ) {
            (Some(a), Some(b)) => {
                if a.offset().local_minus_utc() <= b.offset().local_minus_utc() { *a.offset() } else { *b.offset() }
            }
            _ => *Local::now().offset(),
        }
    })
}

pub fn date(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    if ts == 0 {
        return String::new();
    }
    const FMT: &str = "%Y/%m/%d %H:%M:%S";
    if ts < -2_208_988_800 {
        // Before 1900: see standard_offset.
        return match chrono::Utc.timestamp_opt(ts, 0).single() {
            Some(t) => t.with_timezone(&standard_offset()).format(FMT).to_string(),
            None => String::new(),
        };
    }
    match Local.timestamp_opt(ts, 0).single() {
        Some(t) => t.format(FMT).to_string(),
        None => String::new(),
    }
}

pub fn date_utc(ts: i64) -> String {
    use chrono::{TimeZone, Utc};
    if ts == 0 {
        return String::new();
    }
    match Utc.timestamp_opt(ts, 0).single() {
        Some(t) => t.format("%Y/%m/%d %H:%M:%S").to_string(),
        None => String::new(),
    }
}

/// Parse "yyyy/mm/dd hh:nn:ss" (local) back into a timestamp.
pub fn parse_date(s: &str) -> i64 {
    use chrono::{Local, NaiveDateTime, TimeZone};
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }
    for f in ["%Y/%m/%d %H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y/%m/%d %H:%M", "%d/%m/%Y %H:%M:%S", "%m/%d/%Y %H:%M:%S"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, f) {
            return Local.from_local_datetime(&dt).earliest().map(|t| t.timestamp()).unwrap_or(0);
        }
    }
    0
}

/// Attribute letters as shown in WizTree's "Attributes" column.
pub fn attrs(fl: u32) -> String {
    let mut s = String::new();
    for (bit, ch) in [
        (flags::READONLY, 'R'),
        (flags::HIDDEN, 'H'),
        (flags::SYSTEM, 'S'),
        (flags::ARCHIVE, 'A'),
        (flags::COMPRESSED, 'C'),
        (flags::ENCRYPTED, 'E'),
        (flags::SPARSE, 'P'),
        (flags::REPARSE, 'L'),
        (flags::SYMLINK, 'L'),
        (flags::OFFLINE, 'O'),
        (flags::TEMPORARY, 'T'),
        (flags::NOT_INDEXED, 'N'),
    ] {
        if fl & bit != 0 && !s.contains(ch) {
            s.push(ch);
        }
    }
    s
}

/// Windows FILE_ATTRIBUTE_* bits (used by the CSV format).
pub fn win_attrs(fl: u32) -> u32 {
    let mut a = 0;
    for (bit, w) in [
        (flags::READONLY, 0x1),
        (flags::HIDDEN, 0x2),
        (flags::SYSTEM, 0x4),
        (flags::DIR, 0x10),
        (flags::ARCHIVE, 0x20),
        (flags::TEMPORARY, 0x100),
        (flags::SPARSE, 0x200),
        (flags::REPARSE, 0x400),
        (flags::SYMLINK, 0x400),
        (flags::COMPRESSED, 0x800),
        (flags::OFFLINE, 0x1000),
        (flags::NOT_INDEXED, 0x2000),
        (flags::ENCRYPTED, 0x4000),
        (flags::RECALL_ON_OPEN, 0x40000),
        (flags::PINNED, 0x80000),
        (flags::INDEX_VIEW, 0x1000_0000),
        (flags::ATTR_29, 0x2000_0000),
        (flags::ATTR_30, 0x4000_0000),
        (flags::ATTR_31, 0x8000_0000),
    ] {
        if fl & bit != 0 {
            a |= w;
        }
    }
    a
}

pub fn from_win_attrs(a: u32) -> u32 {
    let mut fl = 0;
    for (w, bit) in [
        (0x1, flags::READONLY),
        (0x2, flags::HIDDEN),
        (0x4, flags::SYSTEM),
        (0x10, flags::DIR),
        (0x20, flags::ARCHIVE),
        (0x100, flags::TEMPORARY),
        (0x200, flags::SPARSE),
        (0x400, flags::REPARSE),
        (0x800, flags::COMPRESSED),
        (0x1000, flags::OFFLINE),
        (0x2000, flags::NOT_INDEXED),
        (0x4000, flags::ENCRYPTED),
        (0x40000, flags::RECALL_ON_OPEN),
        (0x80000, flags::PINNED),
        (0x1000_0000, flags::INDEX_VIEW),
        (0x2000_0000, flags::ATTR_29),
        (0x4000_0000, flags::ATTR_30),
        (0x8000_0000, flags::ATTR_31),
    ] {
        if a & w != 0 {
            fl |= bit;
        }
    }
    fl
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes() {
        let f = Fmt::default();
        assert_eq!(f.size(512), "512 Bytes");
        assert_eq!(f.size(1536), "1.5 KB");
        assert_eq!(f.size(5 * 1024 * 1024 * 1024), "5.0 GB");
        assert_eq!(thousands(1234567), "1,234,567");
        let f = Fmt { unit: SizeUnit::MB, decimals: 2, base: 1024 };
        assert_eq!(f.size(1024 * 1024 * 1500), "1,500.00 MB");
    }

    /// Every NTFS attribute WizTree prints must survive a scan/export/
    /// import round trip bit for bit.
    #[test]
    fn win_attributes_roundtrip() {
        for a in [
            0x26u32, 0x40020, 0x80020, 0x2000_0026, 0x8000_0006, 0x1000_0026, 0x4000_0020,
        ] {
            assert_eq!(win_attrs(from_win_attrs(a)), a, "{a:#x}");
        }
    }

    /// NTFS zero FILETIME is 1601, not "unknown": it must render a date.
    #[test]
    fn zero_filetime_has_a_date() {
        assert!(!date(-11_644_473_600).is_empty());
        assert_eq!(date(0), "");
    }
}
