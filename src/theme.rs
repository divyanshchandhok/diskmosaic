//! Visual design system: UI themes, accent colours, treemap colour schemes,
//! typography and egui style setup.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub name: &'static str,
    pub dark: bool,
    /// Window background.
    pub bg: Color32,
    /// Toolbar / sidebar surface.
    pub chrome: Color32,
    /// Card (content) surface.
    pub card: Color32,
    /// Raised element on a card (inputs, segmented controls).
    pub raised: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub separator: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub accent: Color32,
    pub row_alt: Color32,
    pub treemap_bg: Color32,
    pub danger: Color32,
    pub success: Color32,
    pub warning: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const THEMES: &[Palette] = &[
    Palette {
        name: "Porcelain",
        dark: false,
        bg: rgb(0xF5, 0xF5, 0xF7),
        chrome: rgb(0xEC, 0xEC, 0xEF),
        card: rgb(0xFF, 0xFF, 0xFF),
        raised: rgb(0xF2, 0xF2, 0xF5),
        hover: rgb(0xE9, 0xE9, 0xEE),
        border: rgb(0xDE, 0xDE, 0xE3),
        separator: rgb(0xEA, 0xEA, 0xEE),
        text: rgb(0x1D, 0x1D, 0x1F),
        text2: rgb(0x6E, 0x6E, 0x73),
        text3: rgb(0xA1, 0xA1, 0xA6),
        accent: rgb(0x00, 0x71, 0xE3),
        row_alt: rgb(0xF9, 0xF9, 0xFB),
        treemap_bg: rgb(0xFF, 0xFF, 0xFF),
        danger: rgb(0xFF, 0x3B, 0x30),
        success: rgb(0x34, 0xC7, 0x59),
        warning: rgb(0xFF, 0x95, 0x00),
    },
    Palette {
        name: "Graphite",
        dark: true,
        bg: rgb(0x1C, 0x1C, 0x1E),
        chrome: rgb(0x23, 0x23, 0x26),
        card: rgb(0x2A, 0x2A, 0x2D),
        raised: rgb(0x36, 0x36, 0x3A),
        hover: rgb(0x3A, 0x3A, 0x3E),
        border: rgb(0x3D, 0x3D, 0x41),
        separator: rgb(0x34, 0x34, 0x38),
        text: rgb(0xF5, 0xF5, 0xF7),
        text2: rgb(0xA1, 0xA1, 0xA6),
        text3: rgb(0x6E, 0x6E, 0x73),
        accent: rgb(0x0A, 0x84, 0xFF),
        row_alt: rgb(0x2F, 0x2F, 0x32),
        treemap_bg: rgb(0x1C, 0x1C, 0x1E),
        danger: rgb(0xFF, 0x45, 0x3A),
        success: rgb(0x30, 0xD1, 0x58),
        warning: rgb(0xFF, 0x9F, 0x0A),
    },
    Palette {
        name: "Midnight",
        dark: true,
        bg: rgb(0x0D, 0x11, 0x17),
        chrome: rgb(0x11, 0x16, 0x1D),
        card: rgb(0x16, 0x1B, 0x22),
        raised: rgb(0x21, 0x28, 0x31),
        hover: rgb(0x23, 0x2B, 0x35),
        border: rgb(0x2A, 0x31, 0x3C),
        separator: rgb(0x21, 0x27, 0x30),
        text: rgb(0xE6, 0xED, 0xF3),
        text2: rgb(0x8D, 0x96, 0xA0),
        text3: rgb(0x5A, 0x63, 0x6D),
        accent: rgb(0x58, 0xA6, 0xFF),
        row_alt: rgb(0x19, 0x1F, 0x27),
        treemap_bg: rgb(0x0D, 0x11, 0x17),
        danger: rgb(0xF8, 0x51, 0x49),
        success: rgb(0x3F, 0xB9, 0x50),
        warning: rgb(0xD2, 0x99, 0x22),
    },
    Palette {
        name: "Obsidian",
        dark: true,
        bg: rgb(0x00, 0x00, 0x00),
        chrome: rgb(0x0A, 0x0A, 0x0A),
        card: rgb(0x12, 0x12, 0x13),
        raised: rgb(0x1E, 0x1E, 0x20),
        hover: rgb(0x22, 0x22, 0x25),
        border: rgb(0x26, 0x26, 0x29),
        separator: rgb(0x1C, 0x1C, 0x1E),
        text: rgb(0xFA, 0xFA, 0xFA),
        text2: rgb(0x98, 0x98, 0x9D),
        text3: rgb(0x5C, 0x5C, 0x61),
        accent: rgb(0xBF, 0x5A, 0xF2),
        row_alt: rgb(0x16, 0x16, 0x18),
        treemap_bg: rgb(0x00, 0x00, 0x00),
        danger: rgb(0xFF, 0x45, 0x3A),
        success: rgb(0x30, 0xD1, 0x58),
        warning: rgb(0xFF, 0x9F, 0x0A),
    },
    Palette {
        name: "Nord",
        dark: true,
        bg: rgb(0x2E, 0x34, 0x40),
        chrome: rgb(0x33, 0x39, 0x46),
        card: rgb(0x3B, 0x42, 0x52),
        raised: rgb(0x43, 0x4C, 0x5E),
        hover: rgb(0x46, 0x4F, 0x62),
        border: rgb(0x4C, 0x56, 0x6A),
        separator: rgb(0x43, 0x4C, 0x5E),
        text: rgb(0xEC, 0xEF, 0xF4),
        text2: rgb(0xB8, 0xC0, 0xCE),
        text3: rgb(0x81, 0x8C, 0xA0),
        accent: rgb(0x88, 0xC0, 0xD0),
        row_alt: rgb(0x3F, 0x46, 0x57),
        treemap_bg: rgb(0x2E, 0x34, 0x40),
        danger: rgb(0xBF, 0x61, 0x6A),
        success: rgb(0xA3, 0xBE, 0x8C),
        warning: rgb(0xEB, 0xCB, 0x8B),
    },
    Palette {
        name: "Rosé Pine",
        dark: true,
        bg: rgb(0x19, 0x17, 0x24),
        chrome: rgb(0x1C, 0x1A, 0x29),
        card: rgb(0x1F, 0x1D, 0x2E),
        raised: rgb(0x26, 0x23, 0x3A),
        hover: rgb(0x2A, 0x27, 0x3F),
        border: rgb(0x31, 0x2D, 0x48),
        separator: rgb(0x26, 0x23, 0x3A),
        text: rgb(0xE0, 0xDE, 0xF4),
        text2: rgb(0x90, 0x8C, 0xAA),
        text3: rgb(0x6E, 0x6A, 0x86),
        accent: rgb(0xEB, 0xBC, 0xBA),
        row_alt: rgb(0x22, 0x20, 0x33),
        treemap_bg: rgb(0x19, 0x17, 0x24),
        danger: rgb(0xEB, 0x6F, 0x92),
        success: rgb(0x9C, 0xCF, 0xD8),
        warning: rgb(0xF6, 0xC1, 0x77),
    },
    Palette {
        name: "Sand",
        dark: false,
        bg: rgb(0xF4, 0xF1, 0xEA),
        chrome: rgb(0xEC, 0xE7, 0xDC),
        card: rgb(0xFF, 0xFD, 0xF8),
        raised: rgb(0xF3, 0xEF, 0xE5),
        hover: rgb(0xEC, 0xE6, 0xD9),
        border: rgb(0xE0, 0xD8, 0xC8),
        separator: rgb(0xEC, 0xE6, 0xDA),
        text: rgb(0x2B, 0x26, 0x1E),
        text2: rgb(0x7A, 0x70, 0x60),
        text3: rgb(0xAA, 0xA0, 0x8E),
        accent: rgb(0xC2, 0x6A, 0x2F),
        row_alt: rgb(0xFA, 0xF7, 0xF0),
        treemap_bg: rgb(0xFF, 0xFD, 0xF8),
        danger: rgb(0xC0, 0x39, 0x2B),
        success: rgb(0x4E, 0x8F, 0x3A),
        warning: rgb(0xD9, 0x8E, 0x04),
    },
    Palette {
        name: "Solarized",
        dark: false,
        bg: rgb(0xFD, 0xF6, 0xE3),
        chrome: rgb(0xF5, 0xEE, 0xD8),
        card: rgb(0xFF, 0xFB, 0xEF),
        raised: rgb(0xEE, 0xE8, 0xD5),
        hover: rgb(0xEA, 0xE3, 0xCC),
        border: rgb(0xE0, 0xD9, 0xC1),
        separator: rgb(0xEE, 0xE8, 0xD5),
        text: rgb(0x07, 0x36, 0x42),
        text2: rgb(0x58, 0x6E, 0x75),
        text3: rgb(0x93, 0xA1, 0xA1),
        accent: rgb(0x26, 0x8B, 0xD2),
        row_alt: rgb(0xF8, 0xF2, 0xDF),
        treemap_bg: rgb(0xFF, 0xFB, 0xEF),
        danger: rgb(0xDC, 0x32, 0x2F),
        success: rgb(0x85, 0x99, 0x00),
        warning: rgb(0xB5, 0x89, 0x00),
    },
    Palette {
        name: "Mint",
        dark: false,
        bg: rgb(0xF1, 0xF6, 0xF4),
        chrome: rgb(0xE6, 0xEF, 0xEB),
        card: rgb(0xFF, 0xFF, 0xFF),
        raised: rgb(0xEE, 0xF4, 0xF1),
        hover: rgb(0xE2, 0xEC, 0xE8),
        border: rgb(0xD5, 0xE2, 0xDC),
        separator: rgb(0xE5, 0xEE, 0xEA),
        text: rgb(0x14, 0x2B, 0x24),
        text2: rgb(0x5B, 0x73, 0x6B),
        text3: rgb(0x98, 0xAC, 0xA5),
        accent: rgb(0x00, 0x9B, 0x77),
        row_alt: rgb(0xF6, 0xFA, 0xF8),
        treemap_bg: rgb(0xFF, 0xFF, 0xFF),
        danger: rgb(0xE0, 0x45, 0x3A),
        success: rgb(0x00, 0x9B, 0x77),
        warning: rgb(0xE8, 0x9B, 0x0C),
    },
];

pub const ACCENTS: &[(&str, Color32)] = &[
    ("Blue", rgb(0x0A, 0x84, 0xFF)),
    ("Purple", rgb(0xBF, 0x5A, 0xF2)),
    ("Pink", rgb(0xFF, 0x37, 0x5F)),
    ("Red", rgb(0xFF, 0x45, 0x3A)),
    ("Orange", rgb(0xFF, 0x9F, 0x0A)),
    ("Yellow", rgb(0xE6, 0xB8, 0x00)),
    ("Green", rgb(0x30, 0xD1, 0x58)),
    ("Mint", rgb(0x00, 0xC7, 0xBE)),
    ("Teal", rgb(0x40, 0xC8, 0xE0)),
    ("Indigo", rgb(0x5E, 0x5C, 0xE6)),
    ("Graphite", rgb(0x8E, 0x8E, 0x93)),
];

/// 12 colours for the twelve largest file types + 1 for all other types.
/// `gain` scales colours before cushion shading (which brightens up to ~1.47x);
/// WizTree's own palette is pre-darkened for that, the others are not.
pub struct Scheme {
    pub name: &'static str,
    pub colors: [[u8; 3]; 13],
    pub gain: f32,
}

pub const SCHEMES: &[Scheme] = &[
    Scheme {
        // WizTree's palette, recovered from WizTreeMac (darkenColor(percent, r, g, b) table).
        name: "Classic",
        colors: [
            [0x9A, 0x50, 0x0D], [0x8B, 0x8E, 0x04], [0x16, 0x76, 0x1F], [0x6D, 0x28, 0xA5], [0x88, 0x27, 0x4D],
            [0x10, 0x70, 0xC6], [0x59, 0x54, 0xFA], [0xA2, 0x1E, 0x94], [0xB0, 0x0C, 0x0C], [0x4C, 0x69, 0x1C],
            [0x0F, 0x9E, 0x7D], [0x60, 0x3E, 0x15], [0x48, 0x48, 0x48],
        ],
        gain: 1.0,
    },
    Scheme {
        name: "Vivid",
        colors: [
            [0x0A, 0x84, 0xFF], [0x30, 0xD1, 0x58], [0xFF, 0x9F, 0x0A], [0xFF, 0x37, 0x5F], [0xBF, 0x5A, 0xF2],
            [0x40, 0xC8, 0xE0], [0xFF, 0xD6, 0x0A], [0x5E, 0x5C, 0xE6], [0xFF, 0x45, 0x3A], [0x00, 0xC7, 0xBE],
            [0xAC, 0x8E, 0x68], [0xDA, 0x8F, 0xFF], [0x8E, 0x8E, 0x93],
        ],
        gain: 0.70,
    },
    Scheme {
        name: "Pastel",
        colors: [
            [0x8E, 0xC5, 0xFC], [0xA8, 0xE6, 0xCF], [0xFF, 0xD3, 0xB6], [0xFF, 0xAA, 0xA5], [0xD5, 0xB8, 0xFF],
            [0xA0, 0xE7, 0xE5], [0xFF, 0xF1, 0xA8], [0xB4, 0xC5, 0xFF], [0xFF, 0xC4, 0xE1], [0xC7, 0xF2, 0xA4],
            [0xE2, 0xD1, 0xC3], [0xB8, 0xDE, 0xF0], [0xC7, 0xC7, 0xCC],
        ],
        gain: 0.68,
    },
    Scheme {
        name: "Ocean",
        colors: [
            [0x03, 0x45, 0x8C], [0x02, 0x77, 0xBD], [0x00, 0xA6, 0xD6], [0x4D, 0xD0, 0xE1], [0x00, 0x89, 0x7B],
            [0x26, 0xA6, 0x9A], [0x80, 0xCB, 0xC4], [0x3F, 0x51, 0xB5], [0x79, 0x86, 0xCB], [0x01, 0x57, 0x9B],
            [0x00, 0x60, 0x64], [0x1D, 0xE9, 0xB6], [0x78, 0x90, 0x9C],
        ],
        gain: 0.72,
    },
    Scheme {
        name: "Sunset",
        colors: [
            [0xF9, 0x41, 0x44], [0xF3, 0x72, 0x2C], [0xF8, 0x96, 0x1E], [0xF9, 0xC7, 0x4F], [0xE0, 0x5A, 0x8A],
            [0xB5, 0x17, 0x9E], [0x72, 0x09, 0xB7], [0xFF, 0x8F, 0xAB], [0xD0, 0x00, 0x00], [0xFF, 0xB7, 0x03],
            [0x9D, 0x02, 0x08], [0xE8, 0x5D, 0x04], [0x8D, 0x80, 0x7A],
        ],
        gain: 0.70,
    },
    Scheme {
        name: "Forest",
        colors: [
            [0x2D, 0x6A, 0x4F], [0x40, 0x91, 0x6C], [0x52, 0xB7, 0x88], [0x74, 0xC6, 0x9D], [0x95, 0xD5, 0xB2],
            [0x8C, 0x6A, 0x3F], [0xB0, 0x89, 0x68], [0x6B, 0x8F, 0x23], [0xA7, 0xC9, 0x57], [0x38, 0x66, 0x41],
            [0xBC, 0x6C, 0x25], [0xDD, 0xA1, 0x5E], [0x8A, 0x8F, 0x84],
        ],
        gain: 0.74,
    },
    Scheme {
        name: "Neon",
        colors: [
            [0x00, 0xF5, 0xD4], [0xF1, 0x5B, 0xB5], [0xFE, 0xE4, 0x40], [0x00, 0xBB, 0xF9], [0x9B, 0x5D, 0xE5],
            [0x39, 0xFF, 0x14], [0xFF, 0x6B, 0x35], [0xFF, 0x00, 0x6E], [0x83, 0x38, 0xEC], [0x3A, 0x86, 0xFF],
            [0xFB, 0x56, 0x07], [0xFF, 0xBE, 0x0B], [0x6E, 0x6E, 0x80],
        ],
        gain: 0.70,
    },
    Scheme {
        name: "Graphite",
        colors: [
            [0x5A, 0x5A, 0x60], [0x78, 0x78, 0x7E], [0x96, 0x96, 0x9C], [0xB4, 0xB4, 0xBA], [0x6A, 0x6A, 0x70],
            [0x88, 0x88, 0x8E], [0xA6, 0xA6, 0xAC], [0xC4, 0xC4, 0xCA], [0x50, 0x50, 0x56], [0x70, 0x70, 0x76],
            [0x8E, 0x8E, 0x94], [0xAC, 0xAC, 0xB2], [0x48, 0x48, 0x4C],
        ],
        gain: 0.72,
    },
    Scheme {
        name: "Color Blind",
        colors: [
            [0x00, 0x72, 0xB2], [0xE6, 0x9F, 0x00], [0x56, 0xB4, 0xE9], [0x00, 0x9E, 0x73], [0xF0, 0xE4, 0x42],
            [0xD5, 0x5E, 0x00], [0xCC, 0x79, 0xA7], [0x33, 0x22, 0x88], [0x88, 0xCC, 0xEE], [0x44, 0xAA, 0x99],
            [0x99, 0x99, 0x33], [0xAA, 0x44, 0x99], [0x77, 0x77, 0x77],
        ],
        gain: 0.72,
    },
];

pub fn scheme(name: &str) -> &'static Scheme {
    SCHEMES.iter().find(|s| s.name == name).unwrap_or(&SCHEMES[0])
}

/// Resolve the active palette. `theme` may be "Auto", which follows the OS.
pub fn resolve(theme: &str, accent: &str, system_dark: bool) -> Palette {
    let mut p = if theme == "Auto" || theme.is_empty() {
        if system_dark { THEMES[1] } else { THEMES[0] }
    } else {
        *THEMES.iter().find(|t| t.name == theme).unwrap_or(&THEMES[0])
    };
    if let Some((_, c)) = ACCENTS.iter().find(|(n, _)| *n == accent) {
        p.accent = *c;
    }
    p
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

pub fn with_alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

pub const FONT_MEDIUM: &str = "medium";
pub const FONT_SEMIBOLD: &str = "semibold";

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FONT_MEDIUM.into()))
}
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FONT_SEMIBOLD.into()))
}
pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("inter".into(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf"))));
    fonts.font_data.insert("inter-medium".into(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-Medium.ttf"))));
    fonts.font_data.insert("inter-semibold".into(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf"))));

    // Optional system fallbacks for CJK / other scripts in file names.
    let mut fallbacks: Vec<String> = Vec::new();
    for (i, p) in [
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\YuGothR.ttc",
        "C:\\Windows\\Fonts\\malgun.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
    ]
    .iter()
    .enumerate()
    {
        if let Ok(bytes) = std::fs::read(p) {
            let key = format!("fallback{i}");
            fonts.font_data.insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
            fallbacks.push(key);
        }
    }

    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    prop.insert(0, "inter".into());
    prop.extend(fallbacks.iter().cloned());
    let base: Vec<String> = fonts.families[&FontFamily::Proportional].clone();
    let mut med = vec!["inter-medium".to_string()];
    med.extend(base.iter().skip(1).cloned());
    let mut semi = vec!["inter-semibold".to_string()];
    semi.extend(base.iter().skip(1).cloned());
    fonts.families.insert(FontFamily::Name(FONT_MEDIUM.into()), med);
    fonts.families.insert(FontFamily::Name(FONT_SEMIBOLD.into()), semi);
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (TextStyle::Small, regular(11.0)),
        (TextStyle::Body, regular(13.0)),
        (TextStyle::Button, regular(13.0)),
        (TextStyle::Heading, semibold(19.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 6.0);
    s.button_padding = egui::vec2(10.0, 4.0);
    s.interact_size = egui::vec2(24.0, 26.0);
    s.menu_margin = Margin::same(6);
    s.window_margin = Margin::same(18);
    s.indent = 16.0;
    s.combo_height = 320.0;
    s.scroll = egui::style::ScrollStyle::floating();
    s.scroll.bar_width = 7.0;
    style.animation_time = 0.12;

    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.dark_mode = p.dark;
    v.override_text_color = Some(p.text);
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = Shadow { offset: [0, 10], blur: 34, spread: 0, color: Color32::from_black_alpha(if p.dark { 120 } else { 40 }) };
    v.popup_shadow = Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(if p.dark { 100 } else { 30 }) };
    v.faint_bg_color = p.row_alt;
    v.extreme_bg_color = p.raised;
    v.text_edit_bg_color = Some(p.raised);
    v.code_bg_color = p.raised;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = with_alpha(p.accent, if p.dark { 0.45 } else { 0.28 });
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.striped = false;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;

    let r = CornerRadius::same(7);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.card;
    w.noninteractive.weak_bg_fill = p.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.separator);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r;

    w.inactive.bg_fill = p.raised;
    w.inactive.weak_bg_fill = p.raised;
    w.inactive.bg_stroke = Stroke::new(1.0, lerp(p.raised, p.border, 0.6));
    w.inactive.fg_stroke = Stroke::new(1.2, p.text);
    w.inactive.corner_radius = r;
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.border);
    w.hovered.fg_stroke = Stroke::new(1.4, p.text);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = lerp(p.hover, p.accent, 0.25);
    w.active.weak_bg_fill = lerp(p.hover, p.accent, 0.25);
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.6, p.text);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;

    w.open.bg_fill = p.hover;
    w.open.weak_bg_fill = p.hover;
    w.open.bg_stroke = Stroke::new(1.0, p.border);
    w.open.fg_stroke = Stroke::new(1.2, p.text);
    w.open.corner_radius = r;

    style.visuals = v;
    let style = Arc::new(style);
    ctx.set_theme(if p.dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style);
}
