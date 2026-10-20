//! Minimal gettext `.po` translation support. Translations are looked up by
//! the English source string; missing entries fall back to the source.
//!
//! Layout: `<locale_dir>/<lang>/LC_MESSAGES/default.po` where `locale_dir`
//! is `locale/` next to the executable, or `locale/` in the config dir.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn table() -> &'static RwLock<HashMap<String, String>> {
    static T: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
    T.get_or_init(Default::default)
}

/// Translate `s` using the loaded catalogue.
pub fn tr(s: &str) -> String {
    let t = table().read();
    if t.is_empty() {
        return s.to_string();
    }
    if let Some(v) = t.get(s).filter(|v| !v.is_empty()) {
        return v.clone();
    }
    // Our labels use "…" where WizTree's catalogue has "..." or nothing.
    if let Some(base) = s.strip_suffix('…') {
        for k in [format!("{base}..."), base.to_string()] {
            if let Some(v) = t.get(&k).filter(|v| !v.is_empty()) {
                return format!("{}…", v.trim_end_matches("...").trim_end_matches('…'));
            }
        }
    }
    s.to_string()
}

/// Directory searched for translations (the first that exists, else the
/// one next to the executable).
pub fn locale_dir() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    let mut cands = Vec::new();
    if let Some(d) = &exe_dir {
        cands.push(d.join("locale"));
    }
    if let Some(p) = directories::ProjectDirs::from("", "", "wiztree") {
        cands.push(p.config_dir().join("locale"));
    }
    cands.iter().find(|p| p.is_dir()).cloned().or_else(|| cands.into_iter().next()).unwrap_or_else(|| PathBuf::from("locale"))
}

fn po_path(code: &str) -> PathBuf {
    locale_dir().join(code).join("LC_MESSAGES").join("default.po")
}

/// Installed languages as (code, display name).
pub fn available() -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(locale_dir()) {
        for e in rd.flatten() {
            let code = e.file_name().to_string_lossy().into_owned();
            if po_path(&code).is_file() {
                let name = language_name(&code).to_string();
                out.push((code, name));
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Load a language; an empty code (or a missing file) resets to English.
pub fn load(code: &str) -> bool {
    let mut map = HashMap::new();
    let ok = !code.is_empty()
        && std::fs::read_to_string(po_path(code)).map(|s| parse_po(&s, &mut map)).is_ok();
    *table().write() = map;
    ok
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s);
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => {}
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_po(src: &str, map: &mut HashMap<String, String>) {
    #[derive(PartialEq)]
    enum F {
        None,
        Id,
        Str,
    }
    let (mut id, mut st, mut field) = (String::new(), String::new(), F::None);
    let mut flush = |id: &mut String, st: &mut String| {
        if !id.is_empty() && !st.is_empty() {
            // Delphi-style accelerators ("&File") are stripped on both sides.
            map.insert(id.replace('&', ""), st.replace('&', ""));
            map.insert(std::mem::take(id), std::mem::take(st));
        }
        id.clear();
        st.clear();
    };
    for line in src.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some(r) = l.strip_prefix("msgid ") {
            flush(&mut id, &mut st);
            id = unquote(r);
            field = F::Id;
        } else if let Some(r) = l.strip_prefix("msgstr ") {
            st = unquote(r);
            field = F::Str;
        } else if l.starts_with('"') {
            match field {
                F::Id => id.push_str(&unquote(l)),
                F::Str => st.push_str(&unquote(l)),
                F::None => {}
            }
        } else {
            field = F::None;
        }
    }
    flush(&mut id, &mut st);
}

fn language_name(code: &str) -> &str {
    let base = code.split(['_', '-']).next().unwrap_or(code);
    match code {
        "pt_BR" => return "Português (Brasil)",
        "zh_CN" | "zh_Hans" => return "简体中文",
        "zh_TW" | "zh_Hant" => return "繁體中文",
        _ => {}
    }
    match base {
        "ar" => "العربية",
        "bg" => "Български",
        "ca" => "Català",
        "cs" => "Čeština",
        "da" => "Dansk",
        "de" => "Deutsch",
        "el" => "Ελληνικά",
        "en" => "English",
        "es" => "Español",
        "fa" => "فارسی",
        "fi" => "Suomi",
        "fr" => "Français",
        "he" => "עברית",
        "hr" => "Hrvatski",
        "hu" => "Magyar",
        "id" => "Bahasa Indonesia",
        "it" => "Italiano",
        "ja" => "日本語",
        "ko" => "한국어",
        "lt" => "Lietuvių",
        "nl" => "Nederlands",
        "no" | "nb" => "Norsk",
        "pl" => "Polski",
        "pt" => "Português",
        "ro" => "Română",
        "ru" => "Русский",
        "sk" => "Slovenčina",
        "sl" => "Slovenščina",
        "sr" => "Српски",
        "sv" => "Svenska",
        "th" => "ไทย",
        "tr" => "Türkçe",
        "uk" => "Українська",
        "vi" => "Tiếng Việt",
        "zh" => "中文",
        _ => code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_po() {
        let mut m = HashMap::new();
        parse_po(
            "# c\nmsgid \"\"\nmsgstr \"Header\\n\"\n\nmsgid \"&File\"\nmsgstr \"&Datei\"\n\nmsgid \"a\"\n\"b\"\nmsgstr \"x\\ty\"\n",
            &mut m,
        );
        assert_eq!(m.get("File").map(String::as_str), Some("Datei"));
        assert_eq!(m.get("&File").map(String::as_str), Some("&Datei"));
        assert_eq!(m.get("ab").map(String::as_str), Some("x\ty"));
        assert!(!m.contains_key(""));
    }
}
