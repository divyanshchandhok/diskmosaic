//! Search / filter expressions, following WizTree's `decodeFilterText` and
//! `checkSpecialOp` (recovered from WizTreeMac's ScanEngine):
//!
//! * Space-separated terms are ANDed; `|` separates alternatives (OR).
//!   `" | "`, `" |"` and `"| "` are normalised to `|`. Quotes keep spaces.
//! * `!` negates an alternative. `*.*` means "everything".
//! * `*` and `?` are wildcards. Without wildcards a term matches any name
//!   that contains it. `*.ext` is an extension match.
//! * A term containing a path separator is matched against the full path.
//! * `\regex\` is a case-insensitive regular expression.
//! * Special operators: `[S|A|NAMELEN|PATHLEN|DUPSIZE|DUPCOUNT]<op><value>`
//!   with op one of `= == <> != >= <= > <`. With no prefix the operand is the
//!   size (or modified date if the value is a date). Sizes take B/K/M/G/T
//!   suffixes (binary). `=folder` / `=file` restrict the item type.

use crate::model::{NodeId, Tree};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl Op {
    fn test<T: PartialOrd>(self, a: T, b: T) -> bool {
        match self {
            Op::Lt => a < b,
            Op::Le => a <= b,
            Op::Gt => a > b,
            Op::Ge => a >= b,
            Op::Eq => a == b,
            Op::Ne => a != b,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Field {
    Size,
    Alloc,
    NameLen,
    PathLen,
    DupSize,
    DupCount,
}

#[derive(Clone, Debug)]
enum Kind {
    Any,
    Contains(Vec<u8>),
    Glob(Vec<u8>),
    Ext(Vec<u8>),
    Regex(regex::bytes::Regex),
    Num(Field, Op, u64),
    /// Modified date comparison: (op, start, end) of the given day/time.
    Date(Op, i64, i64),
    FoldersOnly,
    FilesOnly,
}

#[derive(Clone, Debug)]
struct Item {
    not: bool,
    kind: Kind,
    /// Match against the full path instead of the name.
    path: bool,
}

/// Duplicate information for DUPSIZE / DUPCOUNT terms.
pub trait DupInfo {
    fn dup(&self, id: NodeId) -> (u32, u64);
}

impl DupInfo for () {
    fn dup(&self, _: NodeId) -> (u32, u64) {
        (0, 0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Matcher {
    groups: Vec<Vec<Item>>,
    pub needs_path: bool,
    pub needs_dups: bool,
}

/// Split on `sep`, ignoring separators inside double quotes.
fn split_quote_aware(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut inq = false;
    for c in s.chars() {
        if c == '"' {
            inq = !inq;
            cur.push(c);
        } else if c == sep && !inq {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out.into_iter().filter(|x| !x.is_empty()).collect()
}

fn strip_quotes(s: &str) -> (&str, bool) {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        (&s[1..s.len() - 1], true)
    } else if let Some(r) = s.strip_prefix('"') {
        (r, true)
    } else {
        (s, false)
    }
}

pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().replace(',', "");
    let idx = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, unit) = s.split_at(idx);
    if num.is_empty() {
        return None;
    }
    let n: f64 = num.parse().ok()?;
    let u = unit.trim().to_ascii_uppercase();
    let mul: f64 = match u.trim_end_matches(['B', 'b']).trim_end_matches('I') {
        "" => 1.0,
        "K" => 1024.0,
        "M" => 1048576.0,
        "G" => 1073741824.0,
        "T" => 1099511627776.0,
        _ => return None,
    };
    Some((n * mul + 0.5) as u64)
}

/// Parse `yyyy/mm/dd[-hh:mm[:ss]]` (also `-` separators, `T`, ±hh:mm offset).
/// Returns the [start, end) range of the specified day or second.
pub fn parse_date(s: &str) -> Option<(i64, i64)> {
    use chrono::{FixedOffset, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    let local_day = |d: NaiveDate| -> Option<(i64, i64)> {
        let t = Local.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).earliest()?.timestamp();
        Some((t, t + 86400))
    };
    let today = Local::now().date_naive();
    match lower.as_str() {
        "today" => return local_day(today),
        "yesterday" => return local_day(today.pred_opt()?),
        _ => {}
    }
    // Date part: first 10 chars with / or - separators.
    let norm: String = s.chars().map(|c| if c == '/' { '-' } else { c }).collect();
    if norm.len() < 8 {
        return None;
    }
    let (date_part, rest) = match norm.char_indices().filter(|(_, c)| *c == '-').nth(2) {
        Some((i, _)) => (&norm[..i], &norm[i + 1..]),
        None => (norm.as_str(), ""),
    };
    let (date_part, rest) = if let Some(i) = norm.find(['T', ' ']) { (&norm[..i], &norm[i + 1..]) } else { (date_part, rest) };
    let d = NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;
    if rest.is_empty() {
        return local_day(d);
    }
    // Optional timezone suffix.
    let (time_str, tz) = match rest.rfind(['+', '-']) {
        Some(i) if i > 0 => (&rest[..i], Some(&rest[i..])),
        _ => (rest, None),
    };
    let (t, span) = if let Ok(t) = NaiveTime::parse_from_str(time_str, "%H:%M:%S") {
        (t, 1)
    } else if let Ok(t) = NaiveTime::parse_from_str(time_str, "%H:%M") {
        (t, 60)
    } else {
        return local_day(d);
    };
    let dt = NaiveDateTime::new(d, t);
    let ts = match tz {
        Some(z) => {
            let sign = if z.starts_with('-') { -1 } else { 1 };
            let z = &z[1..];
            let (h, m) = z.split_once(':').unwrap_or((z, "0"));
            let off = sign * (h.parse::<i32>().ok()? * 3600 + m.parse::<i32>().unwrap_or(0) * 60);
            FixedOffset::east_opt(off)?.from_local_datetime(&dt).single()?.timestamp()
        }
        None => Local.from_local_datetime(&dt).earliest()?.timestamp(),
    };
    Some((ts, ts + span))
}

fn split_op(s: &str) -> Option<(Op, &str)> {
    for (tok, op) in [("==", Op::Eq), ("<>", Op::Ne), ("!=", Op::Ne), (">=", Op::Ge), ("<=", Op::Le), ("=", Op::Eq), (">", Op::Gt), ("<", Op::Lt)] {
        if let Some(r) = s.strip_prefix(tok) {
            return Some((op, r));
        }
    }
    None
}

fn special_op(s: &str) -> Option<Kind> {
    let up = s.to_ascii_uppercase();
    let (field, rest) = if up.starts_with("DUPCOUNT") {
        (Some(Field::DupCount), &s[8..])
    } else if up.starts_with("DUPSIZE") {
        (Some(Field::DupSize), &s[7..])
    } else if up.starts_with("NAMELEN") {
        (Some(Field::NameLen), &s[7..])
    } else if up.starts_with("PATHLEN") {
        (Some(Field::PathLen), &s[7..])
    } else if up.starts_with('A') && split_op(&s[1..]).is_some() {
        (Some(Field::Alloc), &s[1..])
    } else if up.starts_with('S') && split_op(&s[1..]).is_some() {
        (Some(Field::Size), &s[1..])
    } else {
        (None, s)
    };
    let (op, val) = split_op(rest)?;
    if val.is_empty() {
        return None;
    }
    if field.is_none() && op == Op::Eq {
        if val.eq_ignore_ascii_case("folder") {
            return Some(Kind::FoldersOnly);
        }
        if val.eq_ignore_ascii_case("file") {
            return Some(Kind::FilesOnly);
        }
    }
    match field {
        Some(f @ (Field::NameLen | Field::PathLen | Field::DupCount)) => Some(Kind::Num(f, op, val.parse().ok()?)),
        Some(f) => Some(Kind::Num(f, op, parse_size(val)?)),
        None => {
            if let Some(n) = parse_size(val) {
                if !(val.contains('/') || val.matches('-').count() >= 2) {
                    return Some(Kind::Num(Field::Size, op, n));
                }
            }
            let (a, b) = parse_date(val)?;
            Some(Kind::Date(op, a, b))
        }
    }
}

fn lower(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len());
    crate::model::push_lower(&mut v, s);
    v
}

fn parse_item(raw: &str, full_path: bool) -> Option<Item> {
    let raw = raw.trim_start();
    if raw.is_empty() {
        return None;
    }
    let sep = std::path::MAIN_SEPARATOR;
    let (mut s, quoted) = strip_quotes(raw);
    let mut not = false;
    if !quoted {
        if let Some(r) = s.strip_prefix('!') {
            not = true;
            s = strip_quotes(r).0;
        }
        if let Some(k) = special_op(s) {
            return Some(Item { not, kind: k, path: false });
        }
    }
    if s.is_empty() {
        return None;
    }
    // \regex\
    if s.len() > 1 && s.starts_with('\\') && (cfg!(not(windows)) || s.ends_with('\\')) {
        let body = s[1..].strip_suffix('\\').unwrap_or(&s[1..]);
        let body = strip_quotes(body).0;
        if let Ok(re) = regex::bytes::RegexBuilder::new(body).case_insensitive(true).build() {
            return Some(Item { not, kind: Kind::Regex(re), path: full_path || body.contains(sep) });
        }
    }
    let wild = s.contains(['*', '?']);
    let has_sep = s.contains('/') || s.contains('\\');
    let path = full_path || has_sep;
    let mut pat: String = if has_sep { s.chars().map(|c| if c == '/' || c == '\\' { sep } else { c }).collect() } else { s.to_string() };
    if pat == "*" || pat == "*.*" {
        return Some(Item { not, kind: Kind::Any, path: false });
    }
    if !path {
        if let Some(e) = pat.strip_prefix("*.") {
            if !e.is_empty() && !e.contains(['*', '?']) {
                return Some(Item { not, kind: Kind::Ext(lower(e)), path: false });
            }
        }
    }
    if wild {
        if path && !pat.starts_with('*') {
            pat.insert(0, '*');
        }
        if path && !pat.ends_with('*') && !pat.contains('.') {
            pat.push('*');
        }
        return Some(Item { not, kind: Kind::Glob(lower(&pat)), path });
    }
    Some(Item { not, kind: Kind::Contains(lower(&pat)), path })
}

/// Glob match supporting `*` and `?` (byte-wise, iterative with backtracking).
pub fn glob(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while si < s.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = pi;
            pi += 1;
            mark = si;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

impl Matcher {
    /// `full_path`: "Match entire path" mode.
    pub fn new(text: &str, full_path: bool) -> Matcher {
        let t = text.replace(" | ", "|").replace(" |", "|").replace("| ", "|");
        let mut groups = Vec::new();
        for term in split_quote_aware(&t, ' ') {
            let term = term.trim();
            if term.is_empty() {
                continue;
            }
            let term = if term == "*.*" { "*" } else { term };
            let items: Vec<Item> = split_quote_aware(term, '|').iter().filter_map(|i| parse_item(i, full_path)).collect();
            if !items.is_empty() {
                groups.push(items);
            }
        }
        let needs_path = groups.iter().flatten().any(|i| i.path || matches!(i.kind, Kind::Num(Field::PathLen, ..)));
        let needs_dups = groups.iter().flatten().any(|i| matches!(i.kind, Kind::Num(Field::DupSize | Field::DupCount, ..)));
        Matcher { groups, needs_path, needs_dups }
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub fn matches(&self, tree: &Tree, id: NodeId, path_lower: Option<&[u8]>) -> bool {
        self.matches_with(tree, id, path_lower, &())
    }

    pub fn matches_with(&self, tree: &Tree, id: NodeId, path_lower: Option<&[u8]>, dups: &dyn DupInfo) -> bool {
        for g in &self.groups {
            let mut any = false;
            for it in g {
                if item_hit(it, tree, id, path_lower, dups) != it.not {
                    any = true;
                    break;
                }
            }
            if !any {
                return false;
            }
        }
        true
    }
}

#[inline]
fn item_hit(it: &Item, tree: &Tree, id: NodeId, path_lower: Option<&[u8]>, dups: &dyn DupInfo) -> bool {
    let node = &tree.nodes[id as usize];
    let hay = || if it.path { path_lower.unwrap_or_else(|| tree.name_lower(id)) } else { tree.name_lower(id) };
    match &it.kind {
        Kind::Any => true,
        Kind::Contains(n) => {
            let h = hay();
            n.len() <= h.len() && memchr::memmem::find(h, n).is_some()
        }
        Kind::Glob(g) => glob(g, hay()),
        Kind::Ext(e) => !node.is_dir() && tree.exts.names[node.ext as usize].as_bytes() == &e[..],
        Kind::Regex(re) => re.is_match(hay()),
        Kind::Num(f, op, v) => {
            let x = match f {
                Field::Size => node.size,
                Field::Alloc => node.alloc,
                Field::NameLen => tree.name(id).chars().count() as u64,
                Field::PathLen => path_lower.map(|p| String::from_utf8_lossy(p).chars().count() as u64).unwrap_or(0),
                Field::DupSize => dups.dup(id).1,
                Field::DupCount => dups.dup(id).0 as u64,
            };
            op.test(x, *v)
        }
        Kind::Date(op, a, b) => {
            let t = node.mtime;
            match op {
                Op::Eq => t >= *a && t < *b,
                Op::Ne => !(t >= *a && t < *b),
                Op::Gt => t >= *b,
                Op::Ge => t >= *a,
                Op::Lt => t < *a,
                Op::Le => t < *b,
            }
        }
        Kind::FoldersOnly => node.is_dir(),
        Kind::FilesOnly => !node.is_dir(),
    }
}

/// Byte ranges of special-operator tokens (for syntax highlighting), using
/// WizTree's highlighter regex.
pub fn highlight_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::RegexBuilder::new(r"(?:^|\s)((?:(?:NAMELEN|PATHLEN|DUPSIZE|DUPCOUNT|[as])(?:==|<>|!=|>=|<=|=|>|<)|(?:==|<>|!=|>=|<=|=|>|<|/))\S+)")
            .case_insensitive(true)
            .build()
            .unwrap()
    });
    re.captures_iter(text).filter_map(|c| c.get(1).map(|m| m.range())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Node, NONE, flags};

    fn tree() -> (Tree, NodeId, NodeId, NodeId) {
        let mut t = Tree::new();
        t.path_sep = std::path::MAIN_SEPARATOR;
        let r = t.add_node(NONE, "/r", Node { flags: flags::DIR | flags::ROOT, ..Default::default() });
        t.roots.push(r);
        let d = t.add_node(r, "Videos", Node { flags: flags::DIR, ..Default::default() });
        let a = t.add_node(d, "Movie.MP4", Node { size: 2 << 30, mtime: 1_700_000_000, ..Default::default() });
        let b = t.add_node(r, "notes old.txt", Node { size: 10, ..Default::default() });
        t.finalize();
        (t, d, a, b)
    }

    fn m(t: &Tree, q: &str, fp: bool, id: NodeId) -> bool {
        let p = t.path(id).to_lowercase();
        Matcher::new(q, fp).matches(t, id, Some(p.as_bytes()))
    }

    #[test]
    fn globs() {
        assert!(glob(b"*.mp4", b"movie.mp4"));
        assert!(!glob(b"*.mp4", b"movie.mp3"));
        assert!(glob(b"m?vie*", b"movie.mp4"));
        assert!(glob(b"*a*b*c", b"xxaxxbxxc"));
    }

    #[test]
    fn wiztree_syntax() {
        let (t, d, a, b) = tree();
        assert!(m(&t, "*.mp4", false, a) && !m(&t, "*.mp4", false, b));
        assert!(m(&t, "note|movie", false, a) && m(&t, "note|movie", false, b));
        assert!(m(&t, "movie mp4", false, a) && !m(&t, "movie txt", false, a));
        assert!(m(&t, ">1gb", false, a) && !m(&t, ">1gb", false, b));
        assert!(m(&t, "s>=2G", false, a));
        assert!(m(&t, "!*.txt", false, a) && !m(&t, "!*.txt", false, b));
        assert!(m(&t, "\"notes old\"", false, b) && !m(&t, "\"notes old\"", false, a));
        assert!(m(&t, "=folder", false, d) && !m(&t, "=folder", false, a));
        assert!(m(&t, "namelen>8", false, a) && !m(&t, "namelen>20", false, a));
        assert!(m(&t, "videos/", false, a));
        assert!(m(&t, ">2020/01/01", false, a) && !m(&t, "<2020/01/01", false, a));
        assert!(m(&t, "\\^movie\\.mp4$\\", false, a));
        assert!(m(&t, "*.* ", false, b));
        assert_eq!(highlight_ranges("foo >1gb NAMELEN<5").len(), 2);
    }
}
