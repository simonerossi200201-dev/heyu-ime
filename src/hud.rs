//! HEYU (鹤羽输入法) - Candidate HUD & Status Pill Renderer
//! ======================================================
//! Pure-Rust per-pixel-alpha renderer for `UpdateLayeredWindow` popups.
//!
//! Design notes
//! * Geometry (rounded cards, hairlines, soft shadows) is rasterised with signed
//!   distance fields into a premultiplied BGRA buffer.
//! * Text is rendered by GDI with ClearType (natural) **directly onto the real,
//!   already-painted background colour** of every pixel and then re-premultiplied.
//!   This gives exactly the same gamma/contrast Windows uses for native text, so
//!   dark-on-light and light-on-dark are both crisp (no thin "white mask" text).
//! * Every metric is multiplied by the DPI scale of the monitor the HUD is shown on
//!   (`MonitorFromPoint` + `GetDpiForMonitor`, falling back to 96 DPI), times the
//!   user's `font_scale`.
//! * Glass themes can use the DWM blur-behind accent (Windows 10/11) clipped to the
//!   card shape; if that API is unavailable we fall back to a drawn translucent card
//!   with a soft shadow.
//! * Layout (horizontal / vertical) and glass are read from `data/hud_style.json`
//!   (`{"layout": "horizontal"|"vertical", "glass": true|false}`).

use crate::engine::{HeyuEngine, LangMode};
use crate::win32::*;
use std::cell::RefCell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub type Rgba = (u8, u8, u8, u8);
pub type Rgb = (u8, u8, u8);

// ---------------------------------------------------------------------------
// Private FFI not present in win32.rs (kept local so win32.rs stays untouched)
// ---------------------------------------------------------------------------
#[allow(non_snake_case, non_camel_case_types, dead_code)]
mod ffi {
    use crate::win32::{BOOL, HDC, HWND, POINT, RECT};
    use std::ffi::c_void;

    pub type HRGN = isize;
    pub type HMONITOR = isize;
    pub type HMODULE = isize;

    pub const MONITOR_DEFAULTTONEAREST: u32 = 2;
    pub const LOGPIXELSX: i32 = 88;
    pub const CLEARTYPE_NATURAL_QUALITY: u32 = 6;
    pub const WCA_ACCENT_POLICY: u32 = 19;
    pub const ACCENT_DISABLED: u32 = 0;
    pub const ACCENT_ENABLE_BLURBEHIND: u32 = 3;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct MONITORINFO {
        pub cbSize: u32,
        pub rcMonitor: RECT,
        pub rcWork: RECT,
        pub dwFlags: u32,
    }

    #[repr(C)]
    pub struct TEXTMETRICW {
        pub tmHeight: i32,
        pub tmAscent: i32,
        pub tmDescent: i32,
        pub tmInternalLeading: i32,
        pub tmExternalLeading: i32,
        pub tmAveCharWidth: i32,
        pub tmMaxCharWidth: i32,
        pub tmWeight: i32,
        pub tmOverhang: i32,
        pub tmDigitizedAspectX: i32,
        pub tmDigitizedAspectY: i32,
        pub tmFirstChar: u16,
        pub tmLastChar: u16,
        pub tmDefaultChar: u16,
        pub tmBreakChar: u16,
        pub tmItalic: u8,
        pub tmUnderlined: u8,
        pub tmStruckOut: u8,
        pub tmPitchAndFamily: u8,
        pub tmCharSet: u8,
    }

    #[repr(C)]
    pub struct ACCENT_POLICY {
        pub AccentState: u32,
        pub AccentFlags: u32,
        pub GradientColor: u32,
        pub AnimationId: u32,
    }

    #[repr(C)]
    pub struct WINCOMPATTRDATA {
        pub Attrib: u32,
        pub pvData: *mut c_void,
        pub cbData: usize,
    }

    pub type SetWindowCompositionAttributeFn =
        unsafe extern "system" fn(HWND, *mut WINCOMPATTRDATA) -> BOOL;
    pub type GetDpiForMonitorFn = unsafe extern "system" fn(HMONITOR, i32, *mut u32, *mut u32) -> i32;

    #[link(name = "user32")]
    extern "system" {
        pub fn MonitorFromPoint(pt: POINT, dwFlags: u32) -> HMONITOR;
        pub fn GetMonitorInfoW(hMonitor: HMONITOR, lpmi: *mut MONITORINFO) -> BOOL;
        pub fn SetWindowRgn(hWnd: HWND, hRgn: HRGN, bRedraw: BOOL) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        pub fn CreateRoundRectRgn(x1: i32, y1: i32, x2: i32, y2: i32, w: i32, h: i32) -> HRGN;
        pub fn GetTextFaceW(hdc: HDC, c: i32, lpName: *mut u16) -> i32;
        pub fn GetTextMetricsW(hdc: HDC, lptm: *mut TEXTMETRICW) -> BOOL;
        pub fn GetDeviceCaps(hdc: HDC, index: i32) -> i32;
        pub fn GdiFlush() -> BOOL;
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn LoadLibraryW(lpLibFileName: *const u16) -> HMODULE;
        pub fn GetProcAddress(hModule: HMODULE, lpProcName: *const u8) -> *const c_void;
    }
}

// ---------------------------------------------------------------------------
// Themes
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ThemePalette {
    /// "中文名 · English Name"
    pub name: &'static str,
    /// Stable ASCII identifier (used for preview file names).
    pub slug: &'static str,
    pub is_dark: bool,
    /// Theme is designed as frosted glass (uses DWM blur when enabled/available).
    pub glass: bool,
    /// Card alpha used when real blur-behind is active.
    pub glass_alpha: u8,
    /// Subtle grain amplitude (paper fibres / acrylic noise), 0 = none.
    pub texture: u8,
    pub bg_top: Rgba,
    pub bg_bottom: Rgba,
    /// Outer 1px hairline.
    pub border: Rgba,
    /// Inner 1px highlight (stronger along the top edge).
    pub highlight: Rgba,
    /// Shadow strength (0..255).
    pub shadow: u8,
    pub accent: Rgb,
    pub accent_soft: Rgba,
    pub selected_bg: Rgba,
    pub selected_text: Rgb,
    pub selected_index: Rgb,
    pub ghost_accent: Rgb,
    pub text_primary: Rgb,
    pub text_secondary: Rgb,
    pub text_muted: Rgb,
    pub badge_text: Rgb,
    pub divider: Rgba,
}

pub const THEMES: [ThemePalette; 15] = [
    // 0 ── 曜石流金 · Obsidian Gold (dark)
    ThemePalette {
        name: "曜石流金 · Obsidian Gold",
        slug: "obsidian_gold",
        is_dark: true,
        glass: false,
        glass_alpha: 200,
        texture: 0,
        bg_top: (33, 34, 41, 248),
        bg_bottom: (24, 25, 31, 250),
        border: (0, 0, 0, 160),
        highlight: (255, 255, 255, 26),
        shadow: 150,
        accent: (238, 178, 76),
        accent_soft: (238, 178, 76, 36),
        selected_bg: (238, 178, 76, 44),
        selected_text: (255, 224, 166),
        selected_index: (238, 178, 76),
        ghost_accent: (125, 200, 250),
        text_primary: (238, 238, 242),
        text_secondary: (190, 192, 200),
        text_muted: (124, 127, 138),
        badge_text: (34, 26, 12),
        divider: (255, 255, 255, 20),
    },
    // 1 ── 翡翠极光 · Cyber Jade (dark)
    ThemePalette {
        name: "翡翠极光 · Cyber Jade",
        slug: "cyber_jade",
        is_dark: true,
        glass: false,
        glass_alpha: 200,
        texture: 0,
        bg_top: (22, 35, 35, 248),
        bg_bottom: (15, 26, 27, 250),
        border: (0, 0, 0, 160),
        highlight: (255, 255, 255, 24),
        shadow: 150,
        accent: (52, 211, 153),
        accent_soft: (52, 211, 153, 34),
        selected_bg: (52, 211, 153, 40),
        selected_text: (170, 244, 210),
        selected_index: (52, 211, 153),
        ghost_accent: (196, 160, 255),
        text_primary: (234, 244, 241),
        text_secondary: (182, 205, 199),
        text_muted: (110, 138, 132),
        badge_text: (6, 32, 23),
        divider: (255, 255, 255, 20),
    },
    // 2 ── 深海星云 · Nebula Cyan (dark)
    ThemePalette {
        name: "深海星云 · Nebula Cyan",
        slug: "nebula_cyan",
        is_dark: true,
        glass: false,
        glass_alpha: 200,
        texture: 0,
        bg_top: (24, 32, 52, 248),
        bg_bottom: (16, 22, 38, 250),
        border: (0, 0, 0, 160),
        highlight: (255, 255, 255, 26),
        shadow: 150,
        accent: (94, 202, 250),
        accent_soft: (94, 202, 250, 34),
        selected_bg: (94, 202, 250, 40),
        selected_text: (190, 232, 253),
        selected_index: (94, 202, 250),
        ghost_accent: (251, 128, 150),
        text_primary: (238, 243, 251),
        text_secondary: (184, 198, 220),
        text_muted: (110, 126, 154),
        badge_text: (8, 22, 38),
        divider: (255, 255, 255, 20),
    },
    // 3 ── 和紙 · Washi (light, Japanese beige paper)
    ThemePalette {
        name: "和紙 · Washi",
        slug: "washi",
        is_dark: false,
        glass: false,
        glass_alpha: 210,
        texture: 3,
        bg_top: (249, 244, 232, 252),
        bg_bottom: (242, 235, 218, 253),
        border: (96, 72, 40, 58),
        highlight: (255, 253, 246, 210),
        shadow: 70,
        accent: (184, 66, 48),
        accent_soft: (184, 66, 48, 26),
        selected_bg: (184, 66, 48, 30),
        selected_text: (128, 38, 24),
        selected_index: (184, 66, 48),
        ghost_accent: (44, 82, 132),
        text_primary: (44, 38, 32),
        text_secondary: (96, 84, 70),
        text_muted: (150, 135, 114),
        badge_text: (255, 250, 240),
        divider: (96, 72, 40, 38),
    },
    // 4 ── 霜白 · macOS Frost (light, frosted glass)
    ThemePalette {
        name: "霜白 · macOS Frost",
        slug: "macos_frost",
        is_dark: false,
        glass: true,
        glass_alpha: 186,
        texture: 2,
        bg_top: (250, 250, 252, 238),
        bg_bottom: (242, 242, 246, 242),
        border: (0, 0, 0, 40),
        highlight: (255, 255, 255, 220),
        shadow: 85,
        accent: (0, 122, 255),
        accent_soft: (0, 122, 255, 26),
        selected_bg: (0, 122, 255, 255),
        selected_text: (255, 255, 255),
        selected_index: (214, 232, 255),
        ghost_accent: (175, 82, 222),
        text_primary: (29, 29, 31),
        text_secondary: (64, 64, 70),
        text_muted: (134, 134, 140),
        badge_text: (255, 255, 255),
        divider: (0, 0, 0, 22),
    },
    // 5 ── 纸白 · Notion Paper (light, opaque paper white)
    ThemePalette {
        name: "纸白 · Notion Paper",
        slug: "notion_paper",
        is_dark: false,
        glass: false,
        glass_alpha: 220,
        texture: 0,
        bg_top: (255, 255, 255, 254),
        bg_bottom: (255, 255, 255, 254),
        border: (15, 15, 15, 34),
        highlight: (255, 255, 255, 0),
        shadow: 80,
        accent: (35, 131, 226),
        accent_soft: (35, 131, 226, 24),
        selected_bg: (55, 53, 47, 20),
        selected_text: (37, 35, 30),
        selected_index: (35, 131, 226),
        ghost_accent: (217, 115, 13),
        text_primary: (55, 53, 47),
        text_secondary: (95, 93, 87),
        text_muted: (155, 154, 151),
        badge_text: (255, 255, 255),
        divider: (55, 53, 47, 24),
    },
    // 6 ── 樱 · Sakura (light, soft pink)
    ThemePalette {
        name: "樱 · Sakura",
        slug: "sakura",
        is_dark: false,
        glass: false,
        glass_alpha: 210,
        texture: 0,
        bg_top: (255, 249, 251, 252),
        bg_bottom: (253, 241, 245, 253),
        border: (190, 90, 120, 52),
        highlight: (255, 255, 255, 210),
        shadow: 70,
        accent: (214, 86, 128),
        accent_soft: (214, 86, 128, 26),
        selected_bg: (214, 86, 128, 34),
        selected_text: (156, 38, 80),
        selected_index: (214, 86, 128),
        ghost_accent: (112, 104, 196),
        text_primary: (64, 40, 50),
        text_secondary: (112, 82, 94),
        text_muted: (170, 138, 151),
        badge_text: (255, 255, 255),
        divider: (190, 90, 120, 34),
    },
    // 7 ── 薄荷 · Mint (light, fresh green)
    ThemePalette {
        name: "薄荷 · Mint",
        slug: "mint",
        is_dark: false,
        glass: false,
        glass_alpha: 210,
        texture: 0,
        bg_top: (246, 252, 249, 252),
        bg_bottom: (236, 247, 242, 253),
        border: (30, 120, 90, 50),
        highlight: (255, 255, 255, 210),
        shadow: 70,
        accent: (14, 148, 106),
        accent_soft: (14, 148, 106, 26),
        selected_bg: (14, 148, 106, 32),
        selected_text: (6, 96, 68),
        selected_index: (14, 148, 106),
        ghost_accent: (58, 108, 200),
        text_primary: (22, 46, 38),
        text_secondary: (60, 88, 78),
        text_muted: (120, 148, 138),
        badge_text: (255, 255, 255),
        divider: (30, 120, 90, 32),
    },
    // 8 ── 石墨 · Graphite (dark, macOS dark frosted glass)
    ThemePalette {
        name: "石墨 · Graphite",
        slug: "graphite",
        is_dark: true,
        glass: true,
        glass_alpha: 178,
        texture: 2,
        bg_top: (46, 46, 50, 240),
        bg_bottom: (37, 37, 41, 244),
        border: (0, 0, 0, 175),
        highlight: (255, 255, 255, 34),
        shadow: 160,
        accent: (10, 132, 255),
        accent_soft: (10, 132, 255, 44),
        selected_bg: (10, 132, 255, 255),
        selected_text: (255, 255, 255),
        selected_index: (210, 228, 255),
        ghost_accent: (191, 90, 242),
        text_primary: (245, 245, 247),
        text_secondary: (200, 200, 206),
        text_muted: (140, 140, 148),
        badge_text: (255, 255, 255),
        divider: (255, 255, 255, 22),
    },
    // 9 ── 浮世白鹤 · Crane & Cloud (light illustrated: vermilion sun, auspicious clouds & soaring crane)
    ThemePalette {
        name: "浮世白鹤 · Crane & Cloud",
        slug: "crane_cloud",
        is_dark: false,
        glass: false,
        glass_alpha: 230,
        texture: 2,
        bg_top: (252, 249, 242, 253),
        bg_bottom: (245, 238, 226, 254),
        border: (186, 58, 42, 68),
        highlight: (255, 253, 248, 225),
        shadow: 78,
        accent: (194, 56, 38),
        accent_soft: (194, 56, 38, 26),
        selected_bg: (194, 56, 38, 32),
        selected_text: (136, 30, 18),
        selected_index: (194, 56, 38),
        ghost_accent: (36, 98, 118),
        text_primary: (38, 32, 28),
        text_secondary: (92, 78, 68),
        text_muted: (148, 132, 116),
        badge_text: (255, 250, 244),
        divider: (186, 58, 42, 34),
    },
    // 10 ── 幽谷翠竹 · Bamboo & Panda (light illustrated: celadon mist, bamboo leaves & ink panda)
    ThemePalette {
        name: "幽谷翠竹 · Bamboo & Panda",
        slug: "bamboo_panda",
        is_dark: false,
        glass: false,
        glass_alpha: 228,
        texture: 2,
        bg_top: (245, 250, 244, 253),
        bg_bottom: (234, 244, 232, 254),
        border: (46, 112, 74, 62),
        highlight: (255, 255, 252, 220),
        shadow: 76,
        accent: (42, 124, 78),
        accent_soft: (42, 124, 78, 28),
        selected_bg: (42, 124, 78, 34),
        selected_text: (20, 78, 44),
        selected_index: (42, 124, 78),
        ghost_accent: (158, 98, 42),
        text_primary: (28, 42, 32),
        text_secondary: (68, 92, 74),
        text_muted: (122, 146, 128),
        badge_text: (252, 255, 252),
        divider: (46, 112, 74, 34),
    },
    // 11 ── 橘猫午后 · Amber Cat (light illustrated: warm oat latte, sleeping cat & paw prints)
    ThemePalette {
        name: "橘猫午后 · Amber Cat",
        slug: "amber_cat",
        is_dark: false,
        glass: false,
        glass_alpha: 228,
        texture: 2,
        bg_top: (254, 249, 240, 253),
        bg_bottom: (248, 238, 222, 254),
        border: (198, 108, 44, 64),
        highlight: (255, 253, 248, 225),
        shadow: 76,
        accent: (208, 102, 34),
        accent_soft: (208, 102, 34, 28),
        selected_bg: (208, 102, 34, 34),
        selected_text: (142, 58, 12),
        selected_index: (208, 102, 34),
        ghost_accent: (74, 118, 156),
        text_primary: (52, 36, 24),
        text_secondary: (106, 80, 58),
        text_muted: (158, 130, 104),
        badge_text: (255, 251, 245),
        divider: (198, 108, 44, 34),
    },
    // 12 ── 山茶花庭 · Camellia Garden (light illustrated: botanical ivory, camellia blossom & leaves)
    ThemePalette {
        name: "山茶花庭 · Camellia Garden",
        slug: "camellia_flora",
        is_dark: false,
        glass: false,
        glass_alpha: 228,
        texture: 2,
        bg_top: (254, 248, 248, 253),
        bg_bottom: (248, 238, 239, 254),
        border: (176, 68, 88, 62),
        highlight: (255, 253, 253, 225),
        shadow: 76,
        accent: (184, 58, 82),
        accent_soft: (184, 58, 82, 26),
        selected_bg: (184, 58, 82, 32),
        selected_text: (128, 28, 48),
        selected_index: (184, 58, 82),
        ghost_accent: (56, 118, 92),
        text_primary: (54, 34, 38),
        text_secondary: (104, 74, 80),
        text_muted: (156, 124, 130),
        badge_text: (255, 250, 251),
        divider: (176, 68, 88, 34),
    },
    // 13 ── 星夜灵狐 · Starry Fox (dark illustrated: midnight indigo, crescent moon, stars & fox silhouette)
    ThemePalette {
        name: "星夜灵狐 · Starry Fox",
        slug: "starry_fox",
        is_dark: true,
        glass: true,
        glass_alpha: 210,
        texture: 2,
        bg_top: (24, 27, 48, 248),
        bg_bottom: (16, 18, 34, 251),
        border: (236, 188, 96, 82),
        highlight: (255, 238, 196, 34),
        shadow: 158,
        accent: (242, 188, 88),
        accent_soft: (242, 188, 88, 38),
        selected_bg: (242, 188, 88, 46),
        selected_text: (255, 232, 178),
        selected_index: (242, 188, 88),
        ghost_accent: (138, 180, 255),
        text_primary: (240, 242, 252),
        text_secondary: (188, 194, 222),
        text_muted: (118, 126, 162),
        badge_text: (24, 20, 12),
        divider: (242, 188, 88, 28),
    },
    // 14 ── 深林鹿影 · Forest Stag (dark illustrated: Nordic pine, stag antlers, fern & fireflies)
    ThemePalette {
        name: "深林鹿影 · Forest Stag",
        slug: "forest_stag",
        is_dark: true,
        glass: true,
        glass_alpha: 210,
        texture: 2,
        bg_top: (20, 34, 30, 248),
        bg_bottom: (13, 24, 21, 251),
        border: (116, 214, 168, 78),
        highlight: (210, 255, 235, 30),
        shadow: 158,
        accent: (112, 218, 166),
        accent_soft: (112, 218, 166, 36),
        selected_bg: (112, 218, 166, 44),
        selected_text: (198, 252, 224),
        selected_index: (112, 218, 166),
        ghost_accent: (238, 196, 108),
        text_primary: (236, 246, 242),
        text_secondary: (180, 206, 196),
        text_muted: (112, 142, 130),
        badge_text: (8, 26, 18),
        divider: (112, 218, 166, 26),
    },
];

/// Resolve a theme by (possibly out-of-range) index.
pub fn theme_at(index: usize) -> &'static ThemePalette {
    &THEMES[index % THEMES.len()]
}

// ---------------------------------------------------------------------------
// HUD style (layout / glass) – persisted in data/hud_style.json
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HudLayout {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HudStyle {
    pub layout: HudLayout,
    /// Use real DWM blur-behind for glass themes when available.
    pub glass: bool,
}

impl Default for HudStyle {
    fn default() -> Self {
        Self {
            layout: HudLayout::Horizontal,
            glass: true,
        }
    }
}

/// Minimal tolerant parser for `{"layout": "vertical", "glass": false}`.
pub fn parse_hud_style(txt: &str) -> HudStyle {
    let mut st = HudStyle::default();
    let lower = txt.to_ascii_lowercase();
    if let Some(v) = json_value_after(&lower, "\"layout\"") {
        if v.starts_with("\"vertical") || v.starts_with("\"v") {
            st.layout = HudLayout::Vertical;
        }
    }
    if let Some(v) = json_value_after(&lower, "\"glass\"") {
        if v.starts_with("false") || v.starts_with('0') {
            st.glass = false;
        }
    }
    st
}

fn json_value_after<'a>(txt: &'a str, key: &str) -> Option<&'a str> {
    let i = txt.find(key)?;
    let rest = &txt[i + key.len()..];
    let j = rest.find(':')?;
    Some(rest[j + 1..].trim_start())
}

thread_local! {
    static STYLE_CACHE: RefCell<Option<(PathBuf, Option<SystemTime>, HudStyle)>> = RefCell::new(None);
    static STYLE_OVERRIDE: RefCell<Option<HudStyle>> = RefCell::new(None);
}

/// Force a style (e.g. from a settings UI in-process). `None` = use hud_style.json.
pub fn set_hud_style_override(style: Option<HudStyle>) {
    STYLE_OVERRIDE.with(|o| *o.borrow_mut() = style);
}

/// Load `<data_dir>/hud_style.json` (cached, re-read when its mtime changes).
pub fn load_hud_style(data_dir: &Path) -> HudStyle {
    if let Some(s) = STYLE_OVERRIDE.with(|o| *o.borrow()) {
        return s;
    }
    let path = data_dir.join("hud_style.json");
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    STYLE_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((p, t, s)) = c.as_ref() {
            if *p == path && *t == mtime {
                return *s;
            }
        }
        let style = std::fs::read_to_string(&path)
            .map(|t| parse_hud_style(&t))
            .unwrap_or_default();
        *c = Some((path, mtime, style));
        style
    })
}

// ---------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------

const CJK_FACES: &[&str] = &["Microsoft YaHei UI", "Microsoft YaHei", "DengXian", "SimHei"];
const LATIN_FACES: &[&str] = &["Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI"];
const MONO_FACES: &[&str] = &["Cascadia Mono", "Cascadia Code", "Consolas"];
const ICON_FACES: &[&str] = &["Segoe Fluent Icons", "Segoe MDL2 Assets"];

#[derive(Clone, Copy)]
struct ScaledFonts {
    key: u32,
    code: HFONT,
    index: HFONT,
    cand: HFONT,
    comment: HFONT,
    chip: HFONT,
    label: HFONT,
    hint: HFONT,
    icon: HFONT,
    has_icon_font: bool,
}

pub struct FontSet {
    /// Legacy 100%-scale handles (kept for source compatibility).
    pub font_code: HFONT,
    pub font_cand_main: HFONT,
    pub font_cand_sub: HFONT,
    pub font_badge: HFONT,
    pub font_hint: HFONT,
    cjk: Vec<u16>,
    latin: Vec<u16>,
    mono: Vec<u16>,
    icon: Option<Vec<u16>>,
    cache: RefCell<Vec<ScaledFonts>>,
}

fn make_font(face: &[u16], px: f32, weight: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -(px.round().max(6.0) as i32),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ffi::CLEARTYPE_NATURAL_QUALITY,
            DEFAULT_PITCH,
            face.as_ptr(),
        )
    }
}

/// Return the first face in `list` that GDI actually resolves to (not a substitute).
fn resolve_face(list: &[&str]) -> Option<Vec<u16>> {
    with_measure_dc(|dc| unsafe {
        for name in list {
            let w = to_wide(name);
            let f = make_font(&w, 16.0, FW_NORMAL);
            if f == 0 {
                continue;
            }
            let old = SelectObject(dc, f);
            let mut buf = [0u16; 64];
            let n = ffi::GetTextFaceW(dc, buf.len() as i32, buf.as_mut_ptr());
            SelectObject(dc, old);
            DeleteObject(f);
            if n > 0 {
                let got = String::from_utf16_lossy(&buf[..(n as usize).min(64)])
                    .trim_end_matches('\0')
                    .to_lowercase();
                if got == name.to_lowercase() {
                    return Some(w);
                }
            }
        }
        None
    })
}

impl FontSet {
    pub fn new() -> Self {
        let cjk = resolve_face(CJK_FACES).unwrap_or_else(|| to_wide("Microsoft YaHei UI"));
        let latin = resolve_face(LATIN_FACES).unwrap_or_else(|| cjk.clone());
        let mono = resolve_face(MONO_FACES).unwrap_or_else(|| to_wide("Consolas"));
        let icon = resolve_face(ICON_FACES);
        let mut fs = Self {
            font_code: 0,
            font_cand_main: 0,
            font_cand_sub: 0,
            font_badge: 0,
            font_hint: 0,
            cjk,
            latin,
            mono,
            icon,
            cache: RefCell::new(Vec::new()),
        };
        let base = fs.scaled(1.0);
        fs.font_code = base.code;
        fs.font_cand_main = base.cand;
        fs.font_cand_sub = base.comment;
        fs.font_badge = base.chip;
        fs.font_hint = base.hint;
        fs
    }

    fn scaled(&self, scale: f32) -> ScaledFonts {
        let key = (scale * 100.0).round() as u32;
        if let Some(f) = self.cache.borrow().iter().find(|f| f.key == key) {
            return *f;
        }
        let s = key as f32 / 100.0;
        let has_icon_font = self.icon.is_some();
        let icon_face = self.icon.clone().unwrap_or_else(|| self.latin.clone());
        let f = ScaledFonts {
            key,
            code: make_font(&self.mono, 16.5 * s, FW_SEMIBOLD),
            index: make_font(&self.latin, 15.0 * s, FW_SEMIBOLD),
            cand: make_font(&self.cjk, 22.0 * s, FW_NORMAL),
            comment: make_font(&self.cjk, 15.0 * s, FW_NORMAL),
            chip: make_font(&self.cjk, 14.5 * s, FW_BOLD),
            label: make_font(&self.cjk, 15.0 * s, FW_SEMIBOLD),
            hint: make_font(&self.cjk, 18.0 * s, FW_SEMIBOLD),
            icon: make_font(&icon_face, 15.0 * s, FW_NORMAL),
            has_icon_font,
        };
        self.cache.borrow_mut().push(f);
        f
    }
}

impl Default for FontSet {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FontSet {
    fn drop(&mut self) {
        for f in self.cache.borrow().iter() {
            for h in [f.code, f.index, f.cand, f.comment, f.chip, f.label, f.hint, f.icon] {
                if h != 0 {
                    unsafe {
                        DeleteObject(h);
                    }
                }
            }
        }
    }
}

thread_local! {
    static MEASURE_DC: HDC = unsafe { CreateCompatibleDC(0) };
    static MEASURE_FONTS: FontSet = FontSet::new();
}

fn with_measure_dc<R>(f: impl FnOnce(HDC) -> R) -> R {
    MEASURE_DC.with(|dc| f(*dc))
}

fn measure(font: HFONT, text: &str) -> (i32, i32) {
    if text.is_empty() {
        return (0, 0);
    }
    with_measure_dc(|dc| unsafe {
        let old = SelectObject(dc, font);
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut sz = SIZE::default();
        GetTextExtentPoint32W(dc, wide.as_ptr(), wide.len() as i32, &mut sz);
        SelectObject(dc, old);
        (sz.cx, sz.cy)
    })
}

fn line_height(font: HFONT) -> i32 {
    measure(font, "国Ag").1
}

/// Truncate `text` with an ellipsis so it fits in `max_w` pixels.
fn ellipsize(font: HFONT, text: &str, max_w: i32) -> String {
    if measure(font, text).0 <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let s: String = chars[..mid].iter().collect::<String>() + "…";
        if measure(font, &s).0 <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

// ---------------------------------------------------------------------------
// Pixel helpers
// ---------------------------------------------------------------------------

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    (
        lerp(a.0 as f32, b.0 as f32, t).round() as u8,
        lerp(a.1 as f32, b.1 as f32, t).round() as u8,
        lerp(a.2 as f32, b.2 as f32, t).round() as u8,
    )
}

#[inline]
fn rgb(c: Rgba) -> Rgb {
    (c.0, c.1, c.2)
}

/// Blend straight-alpha colour (`a` in 0..1) over a premultiplied 0xAARRGGBB pixel.
#[inline]
fn blend(dst: u32, c: Rgb, a: f32) -> u32 {
    if a <= 0.0 {
        return dst;
    }
    let a = a.min(1.0);
    let inv = 1.0 - a;
    let da = ((dst >> 24) & 0xFF) as f32;
    let dr = ((dst >> 16) & 0xFF) as f32;
    let dg = ((dst >> 8) & 0xFF) as f32;
    let db = (dst & 0xFF) as f32;
    let oa = (a * 255.0 + da * inv).round().min(255.0) as u32;
    let or = (c.0 as f32 * a + dr * inv).round().min(255.0) as u32;
    let og = (c.1 as f32 * a + dg * inv).round().min(255.0) as u32;
    let ob = (c.2 as f32 * a + db * inv).round().min(255.0) as u32;
    (oa << 24) | (or.min(oa) << 16) | (og.min(oa) << 8) | ob.min(oa)
}

#[inline]
fn sdf_rrect(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let hx = w * 0.5;
    let hy = h * 0.5;
    let r = r.min(hx).min(hy);
    let qx = (px - (x + hx)).abs() - (hx - r);
    let qy = (py - (y + hy)).abs() - (hy - r);
    let out = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    out + qx.max(qy).min(0.0) - r
}

/// Complementary error function (Abramowitz–Stegun 7.1.26).
fn erfc(x: f32) -> f32 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = t
        * (0.254_829_6
            + t * (-0.284_496_74 + t * (1.421_413_7 + t * (-1.453_152 + t * 1.061_405_4))));
    let r = poly * (-z * z).exp();
    if x >= 0.0 {
        r
    } else {
        2.0 - r
    }
}

/// Gaussian-blurred edge coverage for signed distance `d` and std-dev `sigma`.
#[inline]
fn soft_edge(d: f32, sigma: f32) -> f32 {
    0.5 * erfc(d / (sigma * std::f32::consts::SQRT_2))
}

/// C1-continuous Hermite smoothstep coverage for Signed Distance Fields (`d` in pixels).
/// Produces silky-smooth, alias-free rounded corners without staircase artifacts.
#[inline]
fn smooth_cov(d: f32) -> f32 {
    let t = ((0.62 - d) / 1.24).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn hash_noise(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    (h & 0xFFFF) as f32 / 65535.0 - 0.5
}

// ---------------------------------------------------------------------------
// Offscreen canvas
// ---------------------------------------------------------------------------

/// A finished offscreen image: premultiplied 0xAARRGGBB, top-down rows.
pub struct RenderedImage {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u32>,
}

pub struct OffscreenCanvas {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u32>,
    dc: HDC,
    bmp: HBITMAP,
    old_obj: HGDIOBJ,
    bits: *mut u32,
}

impl OffscreenCanvas {
    pub fn new(width: i32, height: i32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        unsafe {
            let dc = CreateCompatibleDC(0);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as DWORD,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                bmiColors: [0],
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, 0, 0);
            let old_obj = SelectObject(dc, bmp);
            SetBkMode(dc, TRANSPARENT);
            Self {
                width,
                height,
                pixels: vec![0u32; (width * height) as usize],
                dc,
                bmp,
                old_obj,
                bits: bits as *mut u32,
            }
        }
    }

    fn bits_slice(&mut self) -> Option<&mut [u32]> {
        if self.bits.is_null() {
            None
        } else {
            Some(unsafe {
                std::slice::from_raw_parts_mut(self.bits, (self.width * self.height) as usize)
            })
        }
    }

    /// The main card: soft two-layer shadow (guaranteed to fade to exact 0 alpha well before canvas edges),
    /// gradient body, subtle frosted grain, and single-pass pre-filtered SDF border + inner highlight.
    pub fn draw_card(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, s: f32, theme: &ThemePalette, glass: bool) {
        let hair = s.round().max(1.0);
        let shadow_a = (theme.shadow as f32 / 255.0) * if glass { 0.72 } else { 0.85 };
        let (amb_sigma, amb_dy, amb_a) = (4.2 * s, 2.8 * s, 0.28 * shadow_a);
        let (key_sigma, key_dy, key_a) = (1.1 * s, 1.0 * s, 0.22 * shadow_a);
        let max_shadow_dist = 10.5 * s;
        let body_alpha_scale = |a: u8| -> f32 {
            if glass {
                (theme.glass_alpha as f32 / 255.0).max(0.86)
            } else {
                a as f32 / 255.0
            }
        };
        let tex = if glass { (theme.texture.max(2)) as f32 } else { theme.texture as f32 };
        let border_rgb = rgb(theme.border);
        let border_a = theme.border.3 as f32 / 255.0;
        let hi_rgb = rgb(theme.highlight);
        let hi_a = theme.highlight.3 as f32 / 255.0;

        for py in 0..self.height {
            let fy = py as f32 + 0.5;
            let t = ((fy - y) / h).clamp(0.0, 1.0);
            let bg = mix(rgb(theme.bg_top), rgb(theme.bg_bottom), t);
            let ba = lerp(body_alpha_scale(theme.bg_top.3), body_alpha_scale(theme.bg_bottom.3), t);
            let top_w = (1.0 - (fy - y) / (h * 0.6)).clamp(0.0, 1.0);
            for px in 0..self.width {
                let fx = px as f32 + 0.5;
                let d = sdf_rrect(fx, fy, x, y, w, h, r);
                let cov = smooth_cov(d);
                let idx = (py * self.width + px) as usize;
                let mut p = self.pixels[idx];
                if shadow_a > 0.0 && cov < 1.0 && d < max_shadow_dist {
                    let d1 = sdf_rrect(fx, fy, x + s * 0.5, y + amb_dy, w - s, h - s * 0.5, r);
                    if d1 < max_shadow_dist {
                        let d2 = sdf_rrect(fx, fy, x, y + key_dy, w, h, r);
                        // Windowed radial fade ensures shadow alpha reaches exact 0.0 well inside the canvas margin,
                        // completely eliminating any semi-transparent rectangular box around the rounded corners!
                        let win_t = ((max_shadow_dist - d1) / (4.0 * s)).clamp(0.0, 1.0);
                        let win_fade = win_t * win_t * (3.0 - 2.0 * win_t);
                        let a1 = amb_a * soft_edge(d1, amb_sigma) * win_fade;
                        let a2 = key_a * soft_edge(d2, key_sigma) * win_fade;
                        let sa = (1.0 - (1.0 - a1) * (1.0 - a2)) * (1.0 - cov);
                        if sa > 1.0 / 512.0 {
                            p = blend(p, (0, 0, 0), sa);
                        }
                    }
                }
                if cov > 0.0 {
                    let mut c = bg;
                    if tex > 0.0 {
                        let n = hash_noise(px, py) * tex;
                        c = (
                            (c.0 as f32 + n).clamp(0.0, 255.0) as u8,
                            (c.1 as f32 + n).clamp(0.0, 255.0) as u8,
                            (c.2 as f32 + n).clamp(0.0, 255.0) as u8,
                        );
                    }
                    // Pre-composite inner highlight and hairline border into the card surface colour
                    // BEFORE applying the outer anti-aliased coverage `cov`, preventing double-alpha corner seams!
                    let inner_cov = smooth_cov(d + hair);
                    let inner2_cov = smooth_cov(d + 2.0 * hair);
                    let hi_band = (inner_cov - inner2_cov).max(0.0);
                    if hi_band > 0.0 && hi_a > 0.0 {
                        let wgt = (0.35 + 0.65 * top_w) * hi_a * hi_band;
                        c = mix(c, hi_rgb, wgt.clamp(0.0, 1.0));
                    }
                    let border_band = (1.0 - inner_cov).clamp(0.0, 1.0);
                    let mut surf_a = ba;
                    if border_band > 0.0 && border_a > 0.0 {
                        let bt = (border_band * border_a).clamp(0.0, 1.0);
                        c = mix(c, border_rgb, bt);
                        surf_a = lerp(ba, ba.max(border_a), border_band);
                    }
                    p = blend(p, c, surf_a * cov);
                }
                self.pixels[idx] = p;
            }
        }
        self.draw_theme_art(x, y, w, h, r, s, theme);
    }

    /// Subtle, editorial-grade anti-aliased vector illustration motifs for the 6 artistic themes:
    /// `crane_cloud`, `bamboo_panda`, `amber_cat`, `camellia_flora`, `starry_fox`, `forest_stag`.
    pub fn draw_theme_art(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, s: f32, theme: &ThemePalette) {
        let kind: u8 = match theme.slug {
            "crane_cloud" => 1,
            "bamboo_panda" => 2,
            "amber_cat" => 3,
            "camellia_flora" => 4,
            "starry_fox" => 5,
            "forest_stag" => 6,
            _ => return,
        };
        let x0 = (x.floor() as i32).max(0);
        let y0 = (y.floor() as i32).max(0);
        let x1 = ((x + w).ceil() as i32).min(self.width);
        let y1 = ((y + h).ceil() as i32).min(self.height);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let sd_segment = |px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32| -> f32 {
            let pax = px - ax;
            let pay = py - ay;
            let bax = bx - ax;
            let bay = by - ay;
            let h_proj = ((pax * bax + pay * bay) / (bax * bax + bay * bay).max(0.001)).clamp(0.0, 1.0);
            let dx = pax - bax * h_proj;
            let dy = pay - bay * h_proj;
            (dx * dx + dy * dy).sqrt()
        };
        let rx_anchor = x + w - 26.0 * s;
        let cy_anchor = y + h * 0.5;

        for py in y0..y1 {
            let fy = py as f32 + 0.5;
            for px in x0..x1 {
                let fx = px as f32 + 0.5;
                if fx < x + w - 135.0 * s {
                    continue;
                }
                let card_d = sdf_rrect(fx, fy, x, y, w, h, r);
                let clip = smooth_cov(card_d + 1.2 * s);
                if clip <= 0.0 {
                    continue;
                }
                let idx = (py * self.width + px) as usize;
                let mut p = self.pixels[idx];

                match kind {
                    1 => {
                        // 浮世白鹤 · Crane & Cloud: vermilion sun disc + golden auspicious clouds + crane silhouette
                        let sun_cx = rx_anchor - 4.0 * s;
                        let sun_cy = y + 15.0 * s;
                        let d_sun = ((fx - sun_cx).hypot(fy - sun_cy)) - 10.5 * s;
                        let cov_sun = smooth_cov(d_sun);
                        if cov_sun > 0.0 {
                            p = blend(p, (206, 58, 40), 0.18 * cov_sun * clip);
                        }
                        // Auspicious cloud arcs
                        let c1 = ((fx - (sun_cx - 12.0 * s)).hypot((fy - (sun_cy + 9.0 * s)) * 1.45)) - 9.0 * s;
                        let c2 = ((fx - (sun_cx + 6.0 * s)).hypot((fy - (sun_cy + 11.0 * s)) * 1.45)) - 7.5 * s;
                        let cov_cloud = smooth_cov(c1.min(c2));
                        if cov_cloud > 0.0 {
                            p = blend(p, (198, 148, 72), 0.16 * cov_cloud * clip);
                        }
                        // Soaring crane wings
                        let w1 = sd_segment(fx, fy, sun_cx - 18.0 * s, sun_cy + 2.0 * s, sun_cx - 3.0 * s, sun_cy - 6.0 * s) - 1.3 * s;
                        let w2 = sd_segment(fx, fy, sun_cx - 3.0 * s, sun_cy - 6.0 * s, sun_cx + 11.0 * s, sun_cy - 1.5 * s) - 1.3 * s;
                        let cov_wing = smooth_cov(w1.min(w2));
                        if cov_wing > 0.0 {
                            p = blend(p, (48, 42, 38), 0.22 * cov_wing * clip);
                        }
                    }
                    2 => {
                        // 幽谷翠竹 · Bamboo & Panda: diagonal bamboo stalk + pointed leaves + minimalist panda ears
                        let st1 = sd_segment(fx, fy, rx_anchor - 18.0 * s, y + h, rx_anchor - 6.0 * s, y + 2.0 * s) - 1.4 * s;
                        let lf1 = sd_segment(fx, fy, rx_anchor - 11.0 * s, cy_anchor - 4.0 * s, rx_anchor - 25.0 * s, cy_anchor + 4.0 * s) - 1.8 * s;
                        let lf2 = sd_segment(fx, fy, rx_anchor - 9.0 * s, cy_anchor - 8.0 * s, rx_anchor + 5.0 * s, cy_anchor - 1.0 * s) - 1.7 * s;
                        let cov_b = smooth_cov(st1.min(lf1).min(lf2));
                        if cov_b > 0.0 {
                            p = blend(p, (42, 124, 78), 0.19 * cov_b * clip);
                        }
                        // Subtle panda silhouette in bottom-right corner
                        let pcx = rx_anchor + 5.0 * s;
                        let pcy = y + h - 10.0 * s;
                        let d_head = (fx - pcx).hypot(fy - pcy) - 8.5 * s;
                        let d_ear1 = (fx - (pcx - 6.5 * s)).hypot(fy - (pcy - 6.5 * s)) - 3.4 * s;
                        let d_ear2 = (fx - (pcx + 6.5 * s)).hypot(fy - (pcy - 6.5 * s)) - 3.4 * s;
                        let cov_ears = smooth_cov(d_ear1.min(d_ear2));
                        if cov_ears > 0.0 {
                            p = blend(p, (38, 48, 40), 0.22 * cov_ears * clip);
                        }
                        let cov_head = smooth_cov(d_head.abs() - 1.1 * s);
                        if cov_head > 0.0 {
                            p = blend(p, (42, 124, 78), 0.18 * cov_head * clip);
                        }
                    }
                    3 => {
                        // 橘猫午后 · Amber Cat: cute minimalist paw prints + cat silhouette with pointed ears
                        let draw_paw = |cx: f32, cy: f32, sc: f32| -> f32 {
                            let pad = ((fx - cx).hypot((fy - cy) * 1.18)) - 3.8 * sc;
                            let t1 = (fx - (cx - 4.0 * sc)).hypot(fy - (cy - 4.5 * sc)) - 1.7 * sc;
                            let t2 = (fx - cx).hypot(fy - (cy - 5.5 * sc)) - 1.8 * sc;
                            let t3 = (fx - (cx + 4.0 * sc)).hypot(fy - (cy - 4.5 * sc)) - 1.7 * sc;
                            pad.min(t1).min(t2).min(t3)
                        };
                        let d_paws = draw_paw(rx_anchor - 22.0 * s, y + 14.0 * s, s * 0.95)
                            .min(draw_paw(rx_anchor - 4.0 * s, y + 23.0 * s, s * 0.85));
                        let cov_paw = smooth_cov(d_paws);
                        if cov_paw > 0.0 {
                            p = blend(p, (208, 102, 34), 0.19 * cov_paw * clip);
                        }
                        // Cat head outline in bottom-right
                        let ccx = rx_anchor + 6.0 * s;
                        let ccy = y + h - 10.0 * s;
                        let d_cat = ((fx - ccx).hypot(fy - ccy) - 8.0 * s).abs() - 1.1 * s;
                        let e1 = sd_segment(fx, fy, ccx - 6.5 * s, ccy - 4.0 * s, ccx - 4.5 * s, ccy - 11.0 * s) - 1.3 * s;
                        let e2 = sd_segment(fx, fy, ccx + 6.5 * s, ccy - 4.0 * s, ccx + 4.5 * s, ccy - 11.0 * s) - 1.3 * s;
                        let cov_cat = smooth_cov(d_cat.min(e1).min(e2));
                        if cov_cat > 0.0 {
                            p = blend(p, (186, 88, 26), 0.21 * cov_cat * clip);
                        }
                    }
                    4 => {
                        // 山茶花庭 · Camellia Garden: 5-petal botanical blossom + stamen + leaf
                        let fcx = rx_anchor;
                        let fcy = y + 17.0 * s;
                        let dx = fx - fcx;
                        let dy = fy - fcy;
                        let rad = dx.hypot(dy);
                        let ang = dy.atan2(dx);
                        let petal_r = (9.5 + 3.2 * (5.0 * ang).cos()) * s;
                        let d_petal = rad - petal_r;
                        let cov_petal = smooth_cov(d_petal);
                        if cov_petal > 0.0 {
                            p = blend(p, (196, 68, 92), 0.17 * cov_petal * clip);
                        }
                        let cov_ring = smooth_cov((rad - petal_r * 0.65).abs() - 0.9 * s);
                        if cov_ring > 0.0 {
                            p = blend(p, (184, 58, 82), 0.20 * cov_ring * clip);
                        }
                        let cov_core = smooth_cov(rad - 2.6 * s);
                        if cov_core > 0.0 {
                            p = blend(p, (218, 158, 48), 0.28 * cov_core * clip);
                        }
                        // Botanical leaf stem
                        let d_leaf = sd_segment(fx, fy, fcx - 10.0 * s, fcy + 7.0 * s, fcx - 24.0 * s, fcy + 14.0 * s) - 2.2 * s;
                        let cov_leaf = smooth_cov(d_leaf);
                        if cov_leaf > 0.0 {
                            p = blend(p, (56, 118, 92), 0.18 * cov_leaf * clip);
                        }
                    }
                    5 => {
                        // 星夜灵狐 · Starry Fox: golden crescent moon + 4-point twinkling stars + fox ears
                        let mcx = rx_anchor + 2.0 * s;
                        let mcy = y + 15.0 * s;
                        let d_outer = (fx - mcx).hypot(fy - mcy) - 8.5 * s;
                        let d_cut = (fx - (mcx + 3.6 * s)).hypot(fy - (mcy - 2.2 * s)) - 7.2 * s;
                        let d_moon = d_outer.max(-d_cut);
                        let cov_moon = smooth_cov(d_moon);
                        if cov_moon > 0.0 {
                            p = blend(p, (246, 198, 96), 0.26 * cov_moon * clip);
                        }
                        // Twinkling stars
                        let star1 = ((fx - (mcx - 20.0 * s)).abs() + (fy - (mcy + 3.0 * s)).abs()) - 2.6 * s;
                        let star2 = ((fx - (mcx - 11.0 * s)).abs() + (fy - (mcy + 13.0 * s)).abs()) - 2.0 * s;
                        let cov_star = smooth_cov(star1.min(star2));
                        if cov_star > 0.0 {
                            p = blend(p, (255, 224, 142), 0.30 * cov_star * clip);
                        }
                    }
                    6 => {
                        // 深林鹿影 · Forest Stag: branching antlers + glowing fireflies
                        let acx = rx_anchor - 4.0 * s;
                        let acy = y + h - 6.0 * s;
                        let b1 = sd_segment(fx, fy, acx, acy, acx - 9.0 * s, acy - 18.0 * s) - 1.1 * s;
                        let b2 = sd_segment(fx, fy, acx - 5.0 * s, acy - 10.0 * s, acx - 13.0 * s, acy - 14.0 * s) - 1.0 * s;
                        let b3 = sd_segment(fx, fy, acx + 4.0 * s, acy, acx + 12.0 * s, acy - 18.0 * s) - 1.1 * s;
                        let cov_ant = smooth_cov(b1.min(b2).min(b3));
                        if cov_ant > 0.0 {
                            p = blend(p, (112, 218, 166), 0.22 * cov_ant * clip);
                        }
                        let ff1 = (fx - (acx - 22.0 * s)).hypot(fy - (y + 14.0 * s)) - 2.2 * s;
                        let ff2 = (fx - (acx + 2.0 * s)).hypot(fy - (y + 11.0 * s)) - 1.8 * s;
                        let cov_ff = smooth_cov(ff1.min(ff2));
                        if cov_ff > 0.0 {
                            p = blend(p, (242, 208, 112), 0.34 * cov_ff * clip);
                        }
                    }
                    _ => {}
                }
                self.pixels[idx] = p;
            }
        }
    }

    /// Anti-aliased rounded rectangle fill with optional hairline border.
    pub fn fill_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, fill: Rgba, border: Option<Rgba>) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let x0 = (x - 1.5).floor().max(0.0) as i32;
        let y0 = (y - 1.5).floor().max(0.0) as i32;
        let x1 = ((x + w + 1.5).ceil() as i32).min(self.width);
        let y1 = ((y + h + 1.5).ceil() as i32).min(self.height);
        let fill_rgb = rgb(fill);
        let fill_a = fill.3 as f32 / 255.0;
        for py in y0..y1 {
            let fy = py as f32 + 0.5;
            for px in x0..x1 {
                let fx = px as f32 + 0.5;
                let d = sdf_rrect(fx, fy, x, y, w, h, r);
                let cov = smooth_cov(d);
                if cov <= 0.0 {
                    continue;
                }
                let idx = (py * self.width + px) as usize;
                let mut c = fill_rgb;
                let mut a = fill_a;
                if let Some(b) = border {
                    let inner_cov = smooth_cov(d + 1.0);
                    let border_band = (1.0 - inner_cov).clamp(0.0, 1.0);
                    let ba = b.3 as f32 / 255.0;
                    c = mix(c, rgb(b), (border_band * ba).clamp(0.0, 1.0));
                    a = lerp(fill_a, fill_a.max(ba), border_band);
                }
                self.pixels[idx] = blend(self.pixels[idx], c, a * cov);
            }
        }
    }

    /// Back-compat helper (old API name).
    pub fn draw_pill(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, fill: Rgba, border: Option<Rgba>) {
        self.fill_rrect(x, y, w, h, radius, fill, border);
    }

    pub fn measure_text(&self, font: HFONT, text: &str) -> (i32, i32) {
        measure(font, text)
    }

    /// Draw text with GDI ClearType onto the actual background colours, then
    /// re-premultiply with the original coverage. Returns the advance width.
    pub fn draw_text(&mut self, font: HFONT, text: &str, x: i32, y: i32, color: Rgb) -> i32 {
        if text.is_empty() {
            return 0;
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        let (tw, th) = measure(font, text);
        let x0 = (x - 3).max(0);
        let y0 = (y - 2).max(0);
        let x1 = (x + tw + 4).min(self.width);
        let y1 = (y + th + 3).min(self.height);
        if x1 <= x0 || y1 <= y0 {
            return tw;
        }
        let w = self.width;
        // Snapshot opaque background colours into the GDI surface.
        let mut snapshot: Vec<u32> = Vec::with_capacity(((x1 - x0) * (y1 - y0)) as usize);
        {
            let src: Vec<u32> = (y0..y1)
                .flat_map(|py| (x0..x1).map(move |px| (py * w + px) as usize))
                .map(|i| self.pixels[i])
                .collect();
            let bits = match self.bits_slice() {
                Some(b) => b,
                None => return tw,
            };
            let mut k = 0;
            for py in y0..y1 {
                for px in x0..x1 {
                    let p = src[k];
                    k += 1;
                    let a = (p >> 24) & 0xFF;
                    let opaque = if a == 0 {
                        0
                    } else {
                        let un = |c: u32| ((c * 255 + a / 2) / a).min(255);
                        (un((p >> 16) & 0xFF) << 16) | (un((p >> 8) & 0xFF) << 8) | un(p & 0xFF)
                    };
                    bits[(py * w + px) as usize] = opaque;
                    snapshot.push(opaque);
                }
            }
        }
        unsafe {
            ffi::GdiFlush();
            let old = SelectObject(self.dc, font);
            SetBkMode(self.dc, TRANSPARENT);
            SetTextColor(
                self.dc,
                (color.0 as u32) | ((color.1 as u32) << 8) | ((color.2 as u32) << 16),
            );
            TextOutW(self.dc, x, y, wide.as_ptr(), wide.len() as i32);
            ffi::GdiFlush();
            SelectObject(self.dc, old);
        }
        let changed: Vec<(usize, u32)> = {
            let bits = match self.bits_slice() {
                Some(b) => b,
                None => return tw,
            };
            let mut out = Vec::new();
            let mut k = 0;
            for py in y0..y1 {
                for px in x0..x1 {
                    let i = (py * w + px) as usize;
                    let nb = bits[i] & 0x00FF_FFFF;
                    if nb != snapshot[k] {
                        out.push((i, nb));
                    }
                    k += 1;
                }
            }
            out
        };
        for (i, nb) in changed {
            let p = self.pixels[i];
            let a = (p >> 24) & 0xFF;
            if a == 0 {
                continue;
            }
            let pm = |c: u32| (c * a + 127) / 255;
            self.pixels[i] = (a << 24) | (pm((nb >> 16) & 0xFF) << 16) | (pm((nb >> 8) & 0xFF) << 8) | pm(nb & 0xFF);
        }
        tw
    }

    pub fn into_image(self) -> RenderedImage {
        RenderedImage {
            width: self.width,
            height: self.height,
            pixels: self.pixels.clone(),
        }
    }

    pub fn present_to_layered_window(&mut self, hwnd: HWND, screen_pos: POINT) {
        let n = self.pixels.len();
        let src = self.pixels.as_ptr();
        if let Some(bits) = self.bits_slice() {
            unsafe {
                std::ptr::copy_nonoverlapping(src, bits.as_mut_ptr(), n);
            }
        } else {
            return;
        }
        unsafe {
            ffi::GdiFlush();
            let screen_dc = GetDC(0);
            let size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let src_pt = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA,
            };
            UpdateLayeredWindow(
                hwnd, screen_dc, &screen_pos, &size, self.dc, &src_pt, 0, &blend, ULW_ALPHA,
            );
            ReleaseDC(0, screen_dc);
        }
    }
}

impl Drop for OffscreenCanvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old_obj);
            DeleteObject(self.bmp);
            DeleteDC(self.dc);
        }
    }
}

// ---------------------------------------------------------------------------
// Monitor / DPI helpers
// ---------------------------------------------------------------------------

fn load_proc(dll: &str, name: &[u8]) -> *const c_void {
    unsafe {
        let w = to_wide(dll);
        let m = ffi::LoadLibraryW(w.as_ptr());
        if m == 0 {
            return std::ptr::null();
        }
        ffi::GetProcAddress(m, name.as_ptr())
    }
}

thread_local! {
    static DPI_FN: Option<ffi::GetDpiForMonitorFn> = {
        let p = load_proc("shcore.dll", b"GetDpiForMonitor\0");
        if p.is_null() { None } else { Some(unsafe { std::mem::transmute::<*const c_void, ffi::GetDpiForMonitorFn>(p) }) }
    };
    static SWCA_FN: Option<ffi::SetWindowCompositionAttributeFn> = {
        let p = load_proc("user32.dll", b"SetWindowCompositionAttribute\0");
        if p.is_null() { None } else { Some(unsafe { std::mem::transmute::<*const c_void, ffi::SetWindowCompositionAttributeFn>(p) }) }
    };
    static GLASS_STATE: RefCell<Vec<(HWND, bool, i32, i32)>> = RefCell::new(Vec::new());
}

/// DPI of the monitor containing `pt` (96 if unknown).
pub fn dpi_for_point(pt: POINT) -> u32 {
    unsafe {
        let mon = ffi::MonitorFromPoint(pt, ffi::MONITOR_DEFAULTTONEAREST);
        if mon != 0 {
            if let Some(f) = DPI_FN.with(|f| *f) {
                let (mut dx, mut dy) = (0u32, 0u32);
                if f(mon, 0, &mut dx, &mut dy) == 0 && dx >= 48 {
                    return dx;
                }
            }
        }
        let dc = GetDC(0);
        let d = ffi::GetDeviceCaps(dc, ffi::LOGPIXELSX);
        ReleaseDC(0, dc);
        if d >= 48 {
            d as u32
        } else {
            96
        }
    }
}

/// Work area (taskbar excluded) of the monitor containing `pt`.
pub fn work_area_for_point(pt: POINT) -> RECT {
    unsafe {
        let mon = ffi::MonitorFromPoint(pt, ffi::MONITOR_DEFAULTTONEAREST);
        let mut mi = ffi::MONITORINFO {
            cbSize: std::mem::size_of::<ffi::MONITORINFO>() as u32,
            ..Default::default()
        };
        if mon != 0 && ffi::GetMonitorInfoW(mon, &mut mi) != 0 {
            return mi.rcWork;
        }
        RECT {
            left: 0,
            top: 0,
            right: GetSystemMetrics(SM_CXSCREEN).max(800),
            bottom: GetSystemMetrics(SM_CYSCREEN).max(600),
        }
    }
}

fn effective_scale(dpi: u32, font_scale: f32) -> f32 {
    let fs = if font_scale.is_finite() { font_scale.clamp(0.75, 2.0) } else { 1.0 };
    ((dpi as f32 / 96.0) * fs).clamp(0.75, 4.0)
}

/// Whether DWM blur-behind is currently active on `hwnd` (set by the HUD renderer).
pub fn glass_active(hwnd: HWND) -> bool {
    GLASS_STATE.with(|g| g.borrow().iter().any(|e| e.0 == hwnd && e.1))
}

/// Ensure DWM rectangular blur-behind and 1-bit GDI regions (`SetWindowRgn`) are disabled
/// so `UpdateLayeredWindow` renders 100% smooth per-pixel anti-aliased rounded corners
/// with zero rectangular backdrop artifacts.
fn apply_glass(hwnd: HWND, _on: bool, w: i32, h: i32, _radius: i32) -> bool {
    if hwnd == 0 {
        return false;
    }
    let already_cleared = GLASS_STATE.with(|g| g.borrow().iter().any(|e| e.0 == hwnd && !e.1));
    if already_cleared {
        return false;
    }
    if let Some(f) = SWCA_FN.with(|f| *f) {
        let mut policy = ffi::ACCENT_POLICY {
            AccentState: ffi::ACCENT_DISABLED,
            AccentFlags: 0,
            GradientColor: 0,
            AnimationId: 0,
        };
        let mut data = ffi::WINCOMPATTRDATA {
            Attrib: ffi::WCA_ACCENT_POLICY,
            pvData: &mut policy as *mut _ as *mut c_void,
            cbData: std::mem::size_of::<ffi::ACCENT_POLICY>(),
        };
        unsafe {
            let _ = f(hwnd, &mut data);
        }
    }
    unsafe {
        ffi::SetWindowRgn(hwnd, 0, 1);
    }
    GLASS_STATE.with(|g| {
        let mut g = g.borrow_mut();
        g.retain(|e| e.0 != hwnd);
        g.push((hwnd, false, w, h));
    });
    false
}

// ---------------------------------------------------------------------------
// View model
// ---------------------------------------------------------------------------

/// Everything the candidate window needs, decoupled from the engine (used by the
/// live HUD, TSF bridges and the offline preview renderer).
#[derive(Clone, Debug)]
pub struct HudView {
    pub mode: LangMode,
    pub code: String,
    /// Inline Tab-completion label shown next to the code.
    pub ghost_tab: Option<String>,
    /// (text, comment)
    pub candidates: Vec<(String, String)>,
    pub selected: usize,
    pub page: usize,
    pub total_pages: usize,
    pub cpm: u32,
    /// (character, extra code suffix)
    pub shape_hints: Vec<(String, String)>,
    /// Post-commit continuation suggestions (shown when `code` is empty).
    pub post_commit_ghosts: Vec<String>,
}

impl HudView {
    pub fn from_engine(engine: &HeyuEngine) -> Self {
        let zh = engine.mode == LangMode::ZhFlypy;
        let candidates = engine
            .current_page_candidates()
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let (inline_part, sub_part) = match c.comment.find('\n') {
                    Some(pos) => (&c.comment[..pos], &c.comment[pos..]),
                    None => (c.comment.as_str(), ""),
                };
                let inline = if zh && i == 1 {
                    if inline_part.is_empty() || inline_part == ";" {
                        ";".to_string()
                    } else {
                        format!("; {}", inline_part)
                    }
                } else if zh && i == 2 {
                    if inline_part.is_empty() || inline_part == "'" {
                        "'".to_string()
                    } else {
                        format!("' {}", inline_part)
                    }
                } else {
                    inline_part.to_string()
                };
                (c.text.clone(), format!("{}{}", inline, sub_part))
            })
            .collect();
        Self {
            mode: engine.mode,
            code: engine.preedit_display(),
            ghost_tab: engine.ghost_tab_text.as_ref().map(|(_, l)| l.clone()),
            candidates,
            selected: 0,
            page: engine.page,
            total_pages: engine.total_pages(),
            cpm: engine.current_cpm(),
            shape_hints: engine
                .active_shape_hints
                .iter()
                .map(|h| (h.ch.clone(), h.suffix.clone()))
                .collect(),
            post_commit_ghosts: engine.post_commit_ghosts.clone(),
        }
    }

    pub fn is_visible(&self) -> bool {
        !self.code.is_empty() || !self.post_commit_ghosts.is_empty()
    }
}

fn mode_short(mode: LangMode) -> &'static str {
    match mode {
        LangMode::ZhFlypy => "鹤",
        LangMode::It => "IT",
        LangMode::Es => "ES",
        LangMode::En => "EN",
        LangMode::Direct => "英",
    }
}

fn mode_long(mode: LangMode) -> &'static str {
    match mode {
        LangMode::ZhFlypy => "鹤羽",
        LangMode::It => "Italiano",
        LangMode::Es => "Español",
        LangMode::En => "English",
        LangMode::Direct => "英文直通",
    }
}

// ---------------------------------------------------------------------------
// Candidate window rendering
// ---------------------------------------------------------------------------

struct Item {
    label: String,
    text: String,
    comment: String,
    sub_lines: Vec<String>,
    label_w: i32,
    text_w: i32,
    comment_w: i32,
    w: i32,
    keycap: bool,
}

/// Rendered HUD plus where the card sits inside the bitmap.
pub struct HudFrame {
    pub canvas: OffscreenCanvas,
    /// Offset of the card's top-left inside the bitmap (shadow margin).
    pub card_x: i32,
    pub card_y: i32,
    pub card_w: i32,
    pub card_h: i32,
    pub radius: i32,
    pub glass: bool,
}

/// Render the candidate window into an offscreen frame.
/// `max_card_w` limits the card width (e.g. derived from the monitor work area).
pub fn render_candidate_frame(
    view: &HudView,
    theme: &ThemePalette,
    fonts: &FontSet,
    scale: f32,
    layout: HudLayout,
    glass: bool,
    max_card_w: i32,
) -> HudFrame {
    let f = fonts.scaled(scale);
    let sp = |v: f32| -> i32 { (v * scale).round() as i32 };
    let (m_side, m_top, m_bot) = (sp(18.0), sp(12.0), sp(26.0));
    let pad_x = sp(10.0);
    let pad_top = sp(8.0);
    let pad_bot = sp(8.0);
    let head_h = sp(32.0);
    let head_gap = sp(4.0);
    let item_h = sp(40.0);
    let item_pad = sp(9.0);
    let item_gap = sp(2.0);
    let line_gap = sp(2.0);
    let label_gap = sp(7.0);
    let comment_gap = sp(6.0);
    let radius = sp(12.0);
    let item_r = sp(8.0) as f32;
    let vertical = layout == HudLayout::Vertical;

    let ghost_mode = view.code.is_empty() && !view.post_commit_ghosts.is_empty();

    // ---- Items -------------------------------------------------------------
    let max_text = if vertical { sp(440.0) } else { sp(360.0) };
    let max_comment = sp(320.0);
    let max_sub_w = sp(148.0);
    let mut items: Vec<Item> = Vec::new();
    let sources: Vec<(String, String, String)> = if ghost_mode {
        view.post_commit_ghosts
            .iter()
            .enumerate()
            .map(|(i, g)| {
                (
                    if i == 0 { "1/Tab".to_string() } else { format!("{}", i + 1) },
                    g.clone(),
                    String::new(),
                )
            })
            .collect()
    } else {
        view.candidates
            .iter()
            .enumerate()
            .map(|(i, (t, c))| (format!("{}", (i + 1) % 10), t.clone(), c.clone()))
            .collect()
    };
    for (label, text, raw_comment) in sources {
        let mut c_parts = raw_comment.split('\n');
        let inline_raw = c_parts.next().unwrap_or("");
        let sub_lines: Vec<String> = c_parts
            .filter(|s| !s.is_empty())
            .map(|s| ellipsize(f.comment, s, max_sub_w))
            .collect();
        let text = ellipsize(f.cand, &text, max_text);
        let comment = if inline_raw.is_empty() {
            String::new()
        } else {
            ellipsize(f.comment, inline_raw, max_comment)
        };
        let is_tab_badge = label == "1/Tab";
        let label_w = if is_tab_badge {
            measure(f.chip, "Tab ⇥").0 + sp(14.0)
        } else {
            measure(f.index, &label).0
        };
        let text_w = measure(f.cand, &text).0;
        let keycap = comment == ";" || comment == "'";
        let comment_w = if keycap {
            (measure(f.index, &comment).0 + sp(8.0)).max(sp(17.0))
        } else {
            measure(f.comment, &comment).0
        };
        let sub_w = sub_lines.iter().map(|s| measure(f.comment, s).0).max().unwrap_or(0);
        let main_w = item_pad * 2 + label_w + label_gap + text_w + if comment_w > 0 { comment_gap + comment_w } else { 0 };
        let w = if sub_w > 0 {
            main_w.max(item_pad * 2 + sp(2.0) + sub_w)
        } else {
            main_w
        };
        items.push(Item { label, text, comment, sub_lines, label_w, text_w, comment_w, w, keycap });
    }

    // ---- Header ------------------------------------------------------------
    let chip_text = if ghost_mode { "Tab 续写" } else { mode_short(view.mode) };
    let chip_tw = measure(f.chip, chip_text).0;
    let chip_h = sp(24.0);
    let chip_w = (chip_tw + sp(14.0)).max(chip_h);
    let code_text = if ghost_mode { "按 Tab 或 1 直出首选 · 2/3 选词".to_string() } else { view.code.clone() };
    let code_font = if ghost_mode { f.label } else { f.code };
    let code_w = measure(code_font, &code_text).0;
    let ghost_lbl = if ghost_mode { None } else { view.ghost_tab.clone() };
    let tab_tag_str = "Tab ⇥";
    let tab_tag_w = measure(f.chip, tab_tag_str).0 + sp(12.0);
    let ghost_w = ghost_lbl
        .as_ref()
        .map(|g| tab_tag_w + sp(8.0) + measure(f.hint, g).0 + sp(12.0))
        .unwrap_or(0);
    let right_info = if !ghost_mode && view.total_pages > 1 {
        format!("{} / {}", view.page + 1, view.total_pages)
    } else if view.cpm > 0 {
        format!("{} 字/分", view.cpm)
    } else {
        String::new()
    };
    let right_w = measure(if view.total_pages > 1 && !ghost_mode { f.index } else { f.label }, &right_info).0 + if view.total_pages > 1 && !ghost_mode { sp(30.0) } else { 0 };
    let header_w = sp(4.0) + chip_w + sp(9.0) + code_w + if ghost_w > 0 { sp(10.0) + ghost_w } else { 0 } + if right_w > 0 { sp(18.0) + right_w } else { 0 } + sp(4.0);

    // ---- Shape hints ---------------------------------------------------------
    let has_hints = !ghost_mode && !view.shape_hints.is_empty();
    let hint_label = "鹤形";
    let hint_chip_w = measure(f.chip, hint_label).0 + sp(12.0);
    let mut hint_w = 0;
    if has_hints {
        hint_w = sp(4.0) + hint_chip_w + sp(10.0);
        for (ch, sfx) in &view.shape_hints {
            hint_w += measure(f.hint, ch).0 + sp(2.0) + measure(f.code, sfx).0 + sp(14.0);
        }
    }

    // ---- Width & line breaking ------------------------------------------------
    let max_inner = (max_card_w - pad_x * 2).max(sp(240.0));
    let min_inner = if vertical { sp(200.0) } else { sp(240.0) };
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let cand_w;
    if vertical {
        for i in 0..items.len() {
            lines.push(vec![i]);
        }
        cand_w = items.iter().map(|it| it.w).max().unwrap_or(0) + sp(16.0);
    } else {
        let mut cur: Vec<usize> = Vec::new();
        let mut cur_w = 0;
        let mut widest = 0;
        for (i, it) in items.iter().enumerate() {
            let add = if cur.is_empty() { it.w } else { item_gap + it.w };
            if !cur.is_empty() && cur_w + add > max_inner {
                widest = widest.max(cur_w);
                lines.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            cur_w += if cur.is_empty() { it.w } else { item_gap + it.w };
            cur.push(i);
        }
        widest = widest.max(cur_w);
        if !cur.is_empty() {
            lines.push(cur);
        }
        cand_w = widest;
    }
    let empty_msg = "无候选 · 继续输入";
    let empty = items.is_empty();
    let empty_w = if empty { measure(f.label, empty_msg).0 + sp(16.0) } else { 0 };
    let inner_w = header_w.max(cand_w).max(hint_w).max(empty_w).max(min_inner).min(max_inner);
    let sub_line_h = (line_height(f.comment) as f32 * 1.05).round().max(12.0) as i32;
    let line_height_for = |line: &[usize]| -> i32 {
        let max_subs = line.iter().map(|&i| items[i].sub_lines.len()).max().unwrap_or(0);
        item_h + if max_subs > 0 { sp(2.0) + (max_subs as i32) * sub_line_h + sp(4.0) } else { 0 }
    };
    let total_cands_h = if empty {
        item_h
    } else {
        lines.iter().map(|l| line_height_for(l)).sum::<i32>() + ((lines.len() as i32 - 1).max(0)) * line_gap
    };
    let hint_block = if has_hints { sp(6.0) + sp(1.0) + sp(4.0) + sp(28.0) } else { 0 };
    let card_w = inner_w + pad_x * 2;
    let card_h = pad_top + head_h + head_gap + total_cands_h + hint_block + pad_bot;

    let mut cv = OffscreenCanvas::new(card_w + m_side * 2, card_h + m_top + m_bot);
    let (cx0, cy0) = (m_side, m_top);
    cv.draw_card(cx0 as f32, cy0 as f32, card_w as f32, card_h as f32, radius as f32, scale, theme, glass);

    let center_y = |top: i32, h: i32, font: HFONT| -> i32 { top + (h - line_height(font)) / 2 };

    // ---- Draw header -------------------------------------------------------
    let hx0 = cx0 + pad_x + sp(4.0);
    let htop = cy0 + pad_top;
    let chip_y = htop + (head_h - chip_h) / 2;
    let (chip_fill, chip_fg) = if ghost_mode {
        let g = theme.ghost_accent;
        ((g.0, g.1, g.2, 34), g)
    } else {
        (theme.accent_soft, theme.accent)
    };
    cv.fill_rrect(hx0 as f32, chip_y as f32, chip_w as f32, chip_h as f32, sp(6.0) as f32, chip_fill, None);
    cv.draw_text(f.chip, chip_text, hx0 + (chip_w - chip_tw) / 2, center_y(chip_y, chip_h, f.chip), chip_fg);
    let mut hx = hx0 + chip_w + sp(9.0);
    let code_color = if ghost_mode { theme.text_muted } else { theme.text_primary };
    let right_edge = cx0 + card_w - pad_x - sp(4.0);
    let right_reserved = if right_w > 0 { right_w + sp(18.0) } else { 0 };
    let code_max = (right_edge - right_reserved - hx).max(sp(40.0));
    let code_draw = ellipsize(code_font, &code_text, code_max);
    let cw = cv.draw_text(code_font, &code_draw, hx, center_y(htop, head_h, code_font), code_color);
    if !ghost_mode {
        // caret bar after the code
        let bar_h = sp(16.0);
        cv.fill_rrect((hx + cw + sp(3.0)) as f32, (htop + (head_h - bar_h) / 2) as f32, (scale * 1.5).max(1.0), bar_h as f32, 1.0, (theme.accent.0, theme.accent.1, theme.accent.2, 230), None);
    }
    hx += cw + sp(10.0);
    if let Some(g) = ghost_lbl {
        let avail_w = right_edge - right_reserved - hx;
        let max_g_text_w = avail_w - tab_tag_w - sp(20.0);
        if max_g_text_w >= sp(24.0) {
            let g_draw = ellipsize(f.hint, &g, max_g_text_w);
            let g_tw = measure(f.hint, &g_draw).0;
            let pill_h = sp(26.0);
            let pill_y = htop + (head_h - pill_h) / 2;
            let gw = tab_tag_w + sp(8.0) + g_tw + sp(10.0);
            let ga = theme.ghost_accent;
            // Outer pill with clear border and tinted background
            cv.fill_rrect(
                hx as f32,
                pill_y as f32,
                gw as f32,
                pill_h as f32,
                sp(7.0) as f32,
                (ga.0, ga.1, ga.2, 52),
                Some((ga.0, ga.1, ga.2, 175)),
            );
            // Left solid/bold keycap badge "Tab ⇥"
            let tag_h = (pill_h - sp(4.0)).max(sp(18.0));
            let tag_y = pill_y + (pill_h - tag_h) / 2;
            let tag_x = hx + sp(3.0);
            cv.fill_rrect(
                tag_x as f32,
                tag_y as f32,
                tab_tag_w as f32,
                tag_h as f32,
                sp(5.0) as f32,
                (ga.0, ga.1, ga.2, 220),
                None,
            );
            let tag_tw = measure(f.chip, tab_tag_str).0;
            cv.draw_text(
                f.chip,
                tab_tag_str,
                tag_x + (tab_tag_w - tag_tw) / 2,
                center_y(tag_y, tag_h, f.chip),
                (255, 255, 255),
            );
            // Right completion phrase in larger semibold f.hint with high contrast
            let g_col = mix(theme.text_primary, ga, 0.28);
            cv.draw_text(
                f.hint,
                &g_draw,
                tag_x + tab_tag_w + sp(7.0),
                center_y(pill_y, pill_h, f.hint),
                g_col,
            );
        }
    }
    if !right_info.is_empty() {
        let paging = view.total_pages > 1 && !ghost_mode;
        let rf = if paging { f.index } else { f.label };
        let rw = measure(rf, &right_info).0;
        if paging {
            // ‹ n / m ›  with drawn chevrons
            let ty = center_y(htop, head_h, f.index);
            let rx = right_edge - rw - sp(14.0);
            cv.draw_text(f.index, "‹", rx - sp(14.0), ty, if view.page > 0 { theme.text_secondary } else { theme.text_muted });
            cv.draw_text(f.index, &right_info, rx, ty, theme.text_muted);
            cv.draw_text(f.index, "›", rx + rw + sp(8.0), ty, if view.page + 1 < view.total_pages { theme.text_secondary } else { theme.text_muted });
        } else {
            let col = if view.cpm >= 200 { theme.accent } else { theme.text_muted };
            cv.draw_text(rf, &right_info, right_edge - rw, center_y(htop, head_h, rf), col);
        }
    }

    // ---- Draw candidates ---------------------------------------------------
    let ix0 = cx0 + pad_x;
    let mut ly = htop + head_h + head_gap;
    if empty {
        cv.draw_text(f.label, empty_msg, ix0 + sp(8.0), center_y(ly, item_h, f.label), theme.text_muted);
    }
    let sub_word_col = mix(theme.text_muted, rgb(theme.bg_bottom), 0.28);
    let sub_tag_col = mix(theme.accent, sub_word_col, 0.62);
    for line in &lines {
        let mut ix = ix0;
        let row_h = line_height_for(line);
        for &i in line {
            let it = &items[i];
            let selected = i == view.selected;
            let iw = if vertical { inner_w } else { it.w };
            if selected {
                let sb = theme.selected_bg;
                cv.fill_rrect(ix as f32, ly as f32, iw as f32, item_h as f32, item_r, sb, None);
            }
            let (lc, tc, cc) = if selected {
                let filled = theme.selected_bg.3 > 200;
                let cc = if filled { mix(theme.selected_text, rgb(theme.selected_bg), 0.28) } else { theme.text_muted };
                (theme.selected_index, theme.selected_text, cc)
            } else {
                (theme.text_muted, theme.text_primary, theme.text_muted)
            };
            let mut tx = ix + item_pad;
            // Align label/comment baselines with the candidate baseline.
            let cand_y = center_y(ly, item_h, f.cand);
            let base = cand_y + font_ascent(f.cand);
            if it.label == "1/Tab" {
                let ga = theme.ghost_accent;
                let bh = sp(22.0);
                let by = ly + (item_h - bh) / 2;
                cv.fill_rrect(
                    tx as f32,
                    by as f32,
                    it.label_w as f32,
                    bh as f32,
                    sp(5.0) as f32,
                    (ga.0, ga.1, ga.2, 215),
                    None,
                );
                let btw = measure(f.chip, "Tab ⇥").0;
                cv.draw_text(
                    f.chip,
                    "Tab ⇥",
                    tx + (it.label_w - btw) / 2,
                    center_y(by, bh, f.chip),
                    (255, 255, 255),
                );
            } else {
                cv.draw_text(f.index, &it.label, tx, base - font_ascent(f.index), lc);
            }
            tx += it.label_w + label_gap;
            cv.draw_text(f.cand, &it.text, tx, cand_y, tc);
            tx += it.text_w;
            if it.comment_w > 0 {
                tx += comment_gap;
                if it.keycap {
                    let kh = sp(17.0);
                    let ky = base - kh + sp(3.0);
                    let kc = if selected { lc } else { theme.text_muted };
                    cv.fill_rrect(tx as f32, ky as f32, it.comment_w as f32, kh as f32, sp(4.0) as f32, (kc.0, kc.1, kc.2, 18), Some((kc.0, kc.1, kc.2, 90)));
                    let (kw, _) = measure(f.index, &it.comment);
                    cv.draw_text(f.index, &it.comment, tx + (it.comment_w - kw) / 2, ky + (kh - line_height(f.index)) / 2, kc);
                } else {
                    cv.draw_text(f.comment, &it.comment, tx, base - font_ascent(f.comment), cc);
                }
            }
            // Subdued vertical foreign-language column beneath the Chinese candidate
            if !it.sub_lines.is_empty() {
                let sx = ix + item_pad + sp(2.0);
                for (k, sub) in it.sub_lines.iter().enumerate() {
                    let sy = ly + item_h + sp(2.0) + (k as i32) * sub_line_h;
                    if let Some((tag, rest)) = sub.split_once(' ') {
                        let tw_tag = cv.draw_text(f.comment, tag, sx, sy, sub_tag_col);
                        cv.draw_text(f.comment, rest, sx + tw_tag + sp(4.0), sy, sub_word_col);
                    } else {
                        cv.draw_text(f.comment, sub, sx, sy, sub_word_col);
                    }
                }
            }
            ix += iw + item_gap;
        }
        ly += row_h + line_gap;
    }
    if empty {
        ly += item_h + line_gap;
    }

    // ---- Shape-hint row ----------------------------------------------------
    if has_hints {
        let dy = ly - line_gap + sp(6.0);
        cv.fill_rrect((ix0 + sp(4.0)) as f32, dy as f32, (inner_w - sp(8.0)) as f32, scale.max(1.0).round(), 0.0, theme.divider, None);
        let rtop = dy + sp(1.0) + sp(4.0);
        let rh = sp(28.0);
        let mut hx = ix0 + sp(4.0);
        let chip_y = rtop + (rh - chip_h) / 2;
        let ga = theme.ghost_accent;
        cv.fill_rrect(hx as f32, chip_y as f32, hint_chip_w as f32, chip_h as f32, sp(6.0) as f32, (ga.0, ga.1, ga.2, 30), None);
        cv.draw_text(f.chip, hint_label, hx + sp(6.0), center_y(chip_y, chip_h, f.chip), ga);
        hx += hint_chip_w + sp(10.0);
        let limit = cx0 + card_w - pad_x - sp(4.0);
        let hint_y = center_y(rtop, rh, f.hint);
        let hint_base = hint_y + font_ascent(f.hint);
        for (ch, sfx) in &view.shape_hints {
            let need = measure(f.hint, ch).0 + sp(2.0) + measure(f.code, sfx).0;
            if hx + need > limit {
                cv.draw_text(f.hint, "…", hx, hint_y, theme.text_muted);
                break;
            }
            let w1 = cv.draw_text(f.hint, ch, hx, hint_y, theme.text_primary);
            let w2 = cv.draw_text(f.code, sfx, hx + w1 + sp(2.0), hint_base - font_ascent(f.code), theme.accent);
            hx += w1 + sp(2.0) + w2 + sp(14.0);
        }
    }

    HudFrame { canvas: cv, card_x: cx0, card_y: cy0, card_w, card_h, radius, glass }
}

fn font_ascent(font: HFONT) -> i32 {
    with_measure_dc(|dc| unsafe {
        let old = SelectObject(dc, font);
        let mut tm: ffi::TEXTMETRICW = std::mem::zeroed();
        let ok = ffi::GetTextMetricsW(dc, &mut tm);
        SelectObject(dc, old);
        if ok != 0 {
            tm.tmAscent
        } else {
            (line_height(font) as f32 * 0.78).round() as i32
        }
    })
}

/// Render to a standalone image (for previews / tests).
pub fn render_candidate_image(
    view: &HudView,
    theme: &ThemePalette,
    fonts: &FontSet,
    scale: f32,
    layout: HudLayout,
    glass: bool,
) -> RenderedImage {
    render_candidate_frame(view, theme, fonts, scale, layout, glass, (900.0 * scale) as i32)
        .canvas
        .into_image()
}

/// Place a frame next to the caret rect inside the work area. Returns window top-left.
fn place_frame(frame: &HudFrame, caret: RECT, scale: f32) -> POINT {
    let wa = work_area_for_point(POINT { x: caret.left, y: caret.bottom });
    let gap = (24.0 * scale).round() as i32;
    let edge = (6.0 * scale).round() as i32;
    let mut card_x = caret.left + (18.0 * scale) as i32;
    let mut card_y = caret.bottom + gap;
    if card_y + frame.card_h > wa.bottom - edge {
        // Near bottom of screen: shift further right so we don't cover text directly above the caret!
        let right_x = caret.right + (44.0 * scale) as i32;
        if right_x + frame.card_w <= wa.right - edge {
            card_x = right_x;
            card_y = (wa.bottom - edge - frame.card_h).max(wa.top + edge);
        } else {
            let above = caret.top - (36.0 * scale).round() as i32 - frame.card_h;
            card_y = if above >= wa.top + edge { above } else { (wa.bottom - edge - frame.card_h).max(wa.top) };
        }
    }
    card_x = card_x.min(wa.right - edge - frame.card_w).max(wa.left + edge);
    card_y = card_y.max(wa.top);
    POINT { x: card_x - frame.card_x, y: card_y - frame.card_y }
}

/// Present a view next to `caret` (screen coordinates). Hides the window if empty.
pub fn present_candidate_view(
    hwnd_hud: HWND,
    view: &HudView,
    theme: &ThemePalette,
    fonts: &FontSet,
    font_scale: f32,
    style: HudStyle,
    caret: RECT,
) {
    if !view.is_visible() {
        unsafe {
            ShowWindow(hwnd_hud, SW_HIDE);
        }
        return;
    }
    let anchor = POINT { x: caret.left, y: caret.bottom };
    let scale = effective_scale(dpi_for_point(anchor), font_scale);
    let wa = work_area_for_point(anchor);
    let max_card_w = ((wa.right - wa.left) - (40.0 * scale) as i32).min((1360.0 * scale) as i32);
    let want_glass = style.glass && theme.glass;
    let mut frame = render_candidate_frame(view, theme, fonts, scale, style.layout, want_glass, max_card_w);
    apply_glass(hwnd_hud, false, frame.card_w, frame.card_h, frame.radius);
    let pos = place_frame(&frame, caret, scale);
    frame.canvas.present_to_layered_window(hwnd_hud, pos);
    unsafe {
        ShowWindow(hwnd_hud, SW_SHOWNOACTIVATE);
        // Always raise `hwnd_hud` to the very top of the HWND_TOPMOST Z-order band so
        // popup windows (such as ✏ 快速造词 or standalone mini-windows) never cover the candidate bar!
        SetWindowPos(
            hwnd_hud,
            HWND_TOPMOST,
            pos.x,
            pos.y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

/// TSF-friendly entry point: position relative to the caret/composition rectangle.
pub fn update_candidate_hud_at_rect(hwnd_hud: HWND, engine: &HeyuEngine, fonts: &FontSet, caret: RECT) {
    if !engine.is_hud_visible() || engine.config.hide_candidate_hud {
        unsafe {
            ShowWindow(hwnd_hud, SW_HIDE);
        }
        return;
    }
    let effective_caret = if engine.config.hud_x > 10 && engine.config.hud_y > 10 {
        RECT {
            left: engine.config.hud_x,
            top: engine.config.hud_y,
            right: engine.config.hud_x + 1,
            bottom: engine.config.hud_y,
        }
    } else {
        caret
    };
    let view = HudView::from_engine(engine);
    let theme = theme_at(engine.config.theme_index);
    let style = load_hud_style(&engine.data_dir);
    present_candidate_view(hwnd_hud, &view, theme, fonts, engine.config.font_scale, style, effective_caret);
}

/// Render and present the floating candidate HUD. `pos` is the point below
/// the caret, as produced by `query_caret_screen_position`.
pub fn update_candidate_hud(hwnd_hud: HWND, engine: &HeyuEngine, fonts: &FontSet, pos: POINT) {
    let scale = effective_scale(dpi_for_point(pos), engine.config.font_scale);
    let bottom = pos.y;
    let caret = RECT {
        left: pos.x,
        top: bottom - (22.0 * scale) as i32,
        right: pos.x + 1,
        bottom,
    };
    update_candidate_hud_at_rect(hwnd_hud, engine, fonts, caret);
}

// ---------------------------------------------------------------------------
// Status pill
// ---------------------------------------------------------------------------

/// Default logical (100%) width/height of the status pill.
pub const PILL_W: i32 = 412;
pub const PILL_H: i32 = 44;
pub const PILL_GEAR_X: i32 = 226;

/// Interactive regions on the upgraded composable floating status pill dock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PillHit {
    ModeBadge,
    DragBody,
    SchemeToggle,
    TradToggle,
    PunctToggle,
    ImmersionToggle,
    BlindToggle,
    VoiceInput,
    StudioRecorder,
    QuickAdd,
    DictPage,
    ThemeCycle,
    Settings,
}

#[derive(Clone, Debug)]
pub struct PillLayout {
    pub total_w_log: f32,
    pub badge_range: Option<(f32, f32, f32)>,
    pub text_range: Option<(f32, f32)>,
    pub has_cpm: bool,
    pub has_sub: bool,
    pub divider_x: Option<f32>,
    pub buttons: Vec<(PillHit, f32, f32, f32)>,
}

pub fn compute_pill_layout(pill_items: &str, badge_w_log: f32) -> PillLayout {
    let mut has_badge = false;
    let mut has_cpm = false;
    let mut has_sub = false;
    let mut btn_list: Vec<PillHit> = Vec::new();

    for raw in pill_items.split(',') {
        let tok = raw.trim().to_ascii_lowercase();
        let hit_opt = match tok.as_str() {
            "badge" => {
                has_badge = true;
                None
            }
            "cpm" => {
                has_cpm = true;
                None
            }
            "sub" => {
                has_sub = true;
                None
            }
            "scheme" => Some(PillHit::SchemeToggle),
            "trad" => Some(PillHit::TradToggle),
            "punct" => Some(PillHit::PunctToggle),
            "immersion" => Some(PillHit::ImmersionToggle),
            "blind" => Some(PillHit::BlindToggle),
            "voice" => Some(PillHit::VoiceInput),
            "recorder" => Some(PillHit::StudioRecorder),
            "quick_add" => Some(PillHit::QuickAdd),
            "dict" => Some(PillHit::DictPage),
            "theme" => Some(PillHit::ThemeCycle),
            "settings" => Some(PillHit::Settings),
            _ => None,
        };
        if let Some(h) = hit_opt {
            if !btn_list.contains(&h) {
                btn_list.push(h);
            }
        }
    }

    if !has_badge && !has_cpm && !has_sub && btn_list.is_empty() {
        has_badge = true;
        btn_list.push(PillHit::Settings);
    }

    let mut cur_x: f32 = 9.0; // 5px window inset + 4px card padding
    let badge_range = if has_badge {
        let bw = badge_w_log.clamp(28.0, 68.0);
        let r = (cur_x, cur_x + bw, cur_x + bw * 0.5);
        cur_x += bw + 6.0;
        Some(r)
    } else {
        None
    };

    let text_range = if has_cpm || has_sub {
        let tw = if has_cpm && has_sub {
            152.0
        } else if has_cpm {
            76.0
        } else {
            92.0
        };
        let r = (cur_x, cur_x + tw);
        cur_x += tw + 6.0;
        Some(r)
    } else {
        None
    };

    let mut divider_x = None;
    let mut buttons = Vec::with_capacity(btn_list.len());
    if !btn_list.is_empty() {
        if has_badge || has_cpm || has_sub {
            divider_x = Some(cur_x);
            cur_x += 6.0;
        }
        let btn_d = 29.0f32;
        let gap = 5.0f32;
        let n_btns = btn_list.len();
        for (i, hit) in btn_list.iter().copied().enumerate() {
            let x0 = cur_x;
            let x1 = cur_x + btn_d;
            let cx = cur_x + btn_d * 0.5;
            let hit_x0 = if i == 0 { x0 - 4.0 } else { x0 - gap * 0.5 };
            let hit_x1 = if i + 1 == n_btns { x1 + 5.0 } else { x1 + gap * 0.5 };
            buttons.push((hit, hit_x0, hit_x1, cx));
            cur_x += btn_d;
            if i + 1 < n_btns {
                cur_x += gap;
            }
        }
        cur_x += 9.0;
    } else {
        cur_x = (cur_x - 6.0).max(9.0) + 9.0;
    }

    PillLayout {
        total_w_log: cur_x.max(46.0),
        badge_range,
        text_range,
        has_cpm,
        has_sub,
        divider_x,
        buttons,
    }
}

pub fn compute_badge_w_log(
    mode: LangMode,
    user_name: &str,
    has_avatar: bool,
    avatar_size_logical: u32,
    chip_font: HFONT,
    scale: f32,
) -> f32 {
    if has_avatar {
        (avatar_size_logical.clamp(16, 36) as f32).max(28.0)
    } else {
        let uname = user_name.trim();
        let tag_str: String = if mode == LangMode::ZhFlypy && !uname.is_empty() {
            uname.chars().take(4).collect()
        } else {
            mode_short(mode).to_string()
        };
        let (tw_px, _) = measure(chip_font, &tag_str);
        ((tw_px as f32 / scale.max(0.1)) + 12.0).clamp(29.0, 68.0)
    }
}

fn estimate_badge_w_log_at_scale(engine: &HeyuEngine, scale: f32) -> f32 {
    let has_avatar = engine.config.avatar_enabled && load_pill_avatar(&engine.data_dir).is_some();
    MEASURE_FONTS.with(|fonts| {
        let f = fonts.scaled(scale);
        compute_badge_w_log(
            engine.mode,
            &engine.config.user_name,
            has_avatar,
            engine.config.avatar_size,
            f.chip,
            scale,
        )
    })
}

pub fn status_pill_width_px(pill_origin: POINT, engine: &HeyuEngine) -> i32 {
    let scale = effective_scale(dpi_for_point(pill_origin), engine.config.pill_scale.clamp(0.70, 1.65));
    let layout = compute_pill_layout(&engine.config.pill_items, estimate_badge_w_log_at_scale(engine, scale));
    (layout.total_w_log * scale).round() as i32
}

pub fn status_pill_hit_test_with_engine(pill_origin: POINT, screen_x: i32, engine: &HeyuEngine) -> PillHit {
    let scale = effective_scale(dpi_for_point(pill_origin), engine.config.pill_scale.clamp(0.70, 1.65));
    let px = screen_x - pill_origin.x;
    let rel_x = px as f32 / scale.max(0.1);
    let layout = compute_pill_layout(&engine.config.pill_items, estimate_badge_w_log_at_scale(engine, scale));
    if let (Some(&(_, first_x0, _, _)), Some(&(_, _, last_x1, _))) = (layout.buttons.first(), layout.buttons.last()) {
        if rel_x >= first_x0 && rel_x <= last_x1 {
            if let Some(&(best_hit, _, _, _)) = layout
                .buttons
                .iter()
                .min_by_key(|&&(_, _, _, cx_log)| {
                    let gcx = (cx_log * scale).round() as i32;
                    (px - gcx).abs()
                })
            {
                return best_hit;
            }
        }
    }
    if let Some((bx0, bx1, _)) = layout.badge_range {
        if rel_x >= bx0 - 5.0 && rel_x <= bx1 + 4.0 {
            return PillHit::ModeBadge;
        }
    }
    PillHit::DragBody
}

/// Legacy hit-test helper for callers without an engine reference.
pub fn status_pill_hit_test(pill_origin: POINT, screen_x: i32) -> PillHit {
    let scale = effective_scale(dpi_for_point(pill_origin), 1.0);
    let rel_x = ((screen_x - pill_origin.x) as f32 / scale).round() as i32;
    if rel_x <= 46 {
        PillHit::ModeBadge
    } else if rel_x < PILL_GEAR_X {
        PillHit::DragBody
    } else if rel_x < 261 {
        PillHit::TradToggle
    } else if rel_x < 296 {
        PillHit::VoiceInput
    } else if rel_x < 331 {
        PillHit::StudioRecorder
    } else if rel_x < 366 {
        PillHit::QuickAdd
    } else {
        PillHit::Settings
    }
}

/// Render the status pill to an offscreen canvas.
pub fn render_status_pill_canvas(mode: LangMode, cpm: u32, theme: &ThemePalette, fonts: &FontSet, scale: f32) -> OffscreenCanvas {
    render_status_pill_canvas_full(mode, cpm, "", false, theme, fonts, scale)
}

pub fn render_status_pill_canvas_with_sub(
    mode: LangMode,
    cpm: u32,
    custom_sub: &str,
    theme: &ThemePalette,
    fonts: &FontSet,
    scale: f32,
) -> OffscreenCanvas {
    render_status_pill_canvas_full(mode, cpm, custom_sub, false, theme, fonts, scale)
}

pub fn render_status_pill_canvas_full(
    mode: LangMode,
    cpm: u32,
    custom_sub: &str,
    traditional_chinese: bool,
    theme: &ThemePalette,
    fonts: &FontSet,
    scale: f32,
) -> OffscreenCanvas {
    render_status_pill_canvas_with_hover(
        mode,
        cpm,
        custom_sub,
        traditional_chinese,
        None,
        "Ctrl+Alt+N",
        theme,
        fonts,
        scale,
    )
}

pub fn render_status_pill_canvas_with_hover(
    mode: LangMode,
    cpm: u32,
    custom_sub: &str,
    traditional_chinese: bool,
    hovered: Option<PillHit>,
    quick_add_hotkey: &str,
    theme: &ThemePalette,
    fonts: &FontSet,
    scale: f32,
) -> OffscreenCanvas {
    render_status_pill_canvas_with_identity(
        mode,
        cpm,
        custom_sub,
        traditional_chinese,
        hovered,
        quick_add_hotkey,
        "",
        None,
        26,
        theme,
        fonts,
        scale,
    )
}

static PILL_AVATAR_CACHE: std::sync::Mutex<Option<(std::time::SystemTime, u32, u32, Vec<u8>)>> =
    std::sync::Mutex::new(None);

fn load_pill_avatar(data_dir: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let p = data_dir.join("pill_avatar.rgba");
    let mt = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
    if let Ok(guard) = PILL_AVATAR_CACHE.lock() {
        if let Some((cached_mt, w, h, ref buf)) = *guard {
            if cached_mt == mt {
                return Some((w, h, buf.clone()));
            }
        }
    }
    let raw = std::fs::read(&p).ok()?;
    if raw.len() < 8 {
        return None;
    }
    let w = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    let h = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
    let expected = 8usize.saturating_add((w as usize).saturating_mul(h as usize).saturating_mul(4));
    if w == 0 || h == 0 || w > 512 || h > 512 || raw.len() < expected {
        return None;
    }
    let rgba = raw[8..expected].to_vec();
    if let Ok(mut guard) = PILL_AVATAR_CACHE.lock() {
        *guard = Some((mt, w, h, rgba.clone()));
    }
    Some((w, h, rgba))
}

fn draw_circular_avatar(
    cv: &mut OffscreenCanvas,
    cx: f32,
    cy: f32,
    diam: f32,
    src_w: u32,
    src_h: u32,
    rgba: &[u8],
    ring_col: Rgba,
    scale: f32,
) {
    let r = (diam * 0.5).max(4.0);
    let hair = scale.round().max(1.0);
    let x0 = ((cx - r - 2.0).floor() as i32).max(0);
    let y0 = ((cy - r - 2.0).floor() as i32).max(0);
    let x1 = ((cx + r + 2.0).ceil() as i32).min(cv.width);
    let y1 = ((cy + r + 2.0).ceil() as i32).min(cv.height);
    let sw = src_w as f32;
    let sh = src_h as f32;

    for py in y0..y1 {
        let fy = py as f32 + 0.5;
        for px in x0..x1 {
            let fx = px as f32 + 0.5;
            let dist = ((fx - cx) * (fx - cx) + (fy - cy) * (fy - cy)).sqrt();
            let d = dist - r;
            let cov = smooth_cov(d);
            if cov <= 0.0 {
                continue;
            }
            let u = (((fx - (cx - r)) / (2.0 * r)).clamp(0.0, 1.0)) * (sw - 1.0);
            let v = (((fy - (cy - r)) / (2.0 * r)).clamp(0.0, 1.0)) * (sh - 1.0);
            let ux = (u.floor() as u32).min(src_w - 1);
            let uy = (v.floor() as u32).min(src_h - 1);
            let ux1 = (ux + 1).min(src_w - 1);
            let uy1 = (uy + 1).min(src_h - 1);
            let tx = u - ux as f32;
            let ty = v - uy as f32;

            let sample = |sx: u32, sy: u32| -> (f32, f32, f32, f32) {
                let off = ((sy * src_w + sx) as usize) * 4;
                (
                    rgba[off] as f32,
                    rgba[off + 1] as f32,
                    rgba[off + 2] as f32,
                    rgba[off + 3] as f32 / 255.0,
                )
            };
            let p00 = sample(ux, uy);
            let p10 = sample(ux1, uy);
            let p01 = sample(ux, uy1);
            let p11 = sample(ux1, uy1);
            let sr = lerp(lerp(p00.0, p10.0, tx), lerp(p01.0, p11.0, tx), ty).round().clamp(0.0, 255.0) as u8;
            let sg = lerp(lerp(p00.1, p10.1, tx), lerp(p01.1, p11.1, tx), ty).round().clamp(0.0, 255.0) as u8;
            let sb = lerp(lerp(p00.2, p10.2, tx), lerp(p01.2, p11.2, tx), ty).round().clamp(0.0, 255.0) as u8;
            let sa = lerp(lerp(p00.3, p10.3, tx), lerp(p01.3, p11.3, tx), ty).clamp(0.0, 1.0);

            let idx = (py * cv.width + px) as usize;
            let mut p = cv.pixels[idx];
            p = blend(p, (sr, sg, sb), sa * cov);

            let inner_cov = smooth_cov(d + hair);
            let ring_w = (cov - inner_cov).clamp(0.0, 1.0);
            if ring_w > 0.0 && ring_col.3 > 0 {
                let ra = (ring_col.3 as f32 / 255.0) * ring_w;
                p = blend(p, rgb(ring_col), ra);
            }
            cv.pixels[idx] = p;
        }
    }
}

pub struct PillRenderParams<'a> {
    pub mode: LangMode,
    pub cpm: u32,
    pub custom_sub: &'a str,
    pub input_scheme: &'a str,
    pub traditional_chinese: bool,
    pub chinese_punctuation: bool,
    pub immersion_mode: &'a str,
    pub hide_candidate_hud: bool,
    pub pill_items: &'a str,
    pub hovered: Option<PillHit>,
    pub quick_add_hotkey: &'a str,
    pub user_name: &'a str,
    pub avatar: Option<(u32, u32, &'a [u8])>,
    pub avatar_size_logical: u32,
}

pub fn render_status_pill_canvas_with_identity(
    mode: LangMode,
    cpm: u32,
    custom_sub: &str,
    traditional_chinese: bool,
    hovered: Option<PillHit>,
    quick_add_hotkey: &str,
    user_name: &str,
    avatar: Option<(u32, u32, &[u8])>,
    avatar_size_logical: u32,
    theme: &ThemePalette,
    fonts: &FontSet,
    scale: f32,
) -> OffscreenCanvas {
    let params = PillRenderParams {
        mode,
        cpm,
        custom_sub,
        input_scheme: "yinxing",
        traditional_chinese,
        chinese_punctuation: true,
        immersion_mode: "ALL",
        hide_candidate_hud: false,
        pill_items: "badge,cpm,sub,trad,voice,recorder,quick_add,settings",
        hovered,
        quick_add_hotkey,
        user_name,
        avatar,
        avatar_size_logical,
    };
    render_status_pill_canvas_composable(&params, theme, fonts, scale)
}

pub fn render_status_pill_canvas_composable(
    params: &PillRenderParams<'_>,
    theme: &ThemePalette,
    fonts: &FontSet,
    scale: f32,
) -> OffscreenCanvas {
    let f = fonts.scaled(scale);
    let sp = |v: f32| -> i32 { (v * scale).round() as i32 };

    let uname = params.user_name.trim();
    let tag_str: String = if params.mode == LangMode::ZhFlypy && !uname.is_empty() && params.avatar.is_none() {
        uname.chars().take(4).collect()
    } else {
        mode_short(params.mode).to_string()
    };
    let badge_w_log = compute_badge_w_log(
        params.mode,
        params.user_name,
        params.avatar.is_some(),
        params.avatar_size_logical,
        f.chip,
        scale,
    );

    let layout = compute_pill_layout(params.pill_items, badge_w_log);
    let w = sp(layout.total_w_log);
    let h = sp(PILL_H as f32);
    let mut cv = OffscreenCanvas::new(w, h);
    let inset = sp(5.0);
    let ch = h - inset * 2 - sp(1.0);
    let mut t = *theme;
    t.shadow = (theme.shadow as f32 * 0.8) as u8;
    draw_small_card(&mut cv, inset as f32, (inset - sp(1.0)) as f32, (w - inset * 2) as f32, ch as f32, ch as f32 / 2.0, scale, &t);

    let top = inset - sp(1.0);
    let d = ch - sp(8.0);
    let by = top + sp(4.0);

    // 1. Optional Left Mode / Identity Badge
    if let Some((bx0_log, bx1_log, _)) = layout.badge_range {
        let bx = sp(bx0_log);
        let bw = (sp(bx1_log) - bx).max(d);
        if let Some((aw, ah, rgba)) = params.avatar {
            let av_d = sp((params.avatar_size_logical.clamp(16, 34)) as f32).min(ch - sp(2.0));
            let av_cx = bx as f32 + (bw as f32) * 0.5;
            let av_cy = top as f32 + (ch as f32) * 0.5;
            let ring = if params.hovered == Some(PillHit::ModeBadge) {
                (theme.highlight.0, theme.highlight.1, theme.highlight.2, 245)
            } else {
                (theme.accent.0, theme.accent.1, theme.accent.2, 210)
            };
            draw_circular_avatar(&mut cv, av_cx, av_cy, av_d as f32, aw, ah, rgba, ring, scale);
        } else {
            let tag_w = measure(f.chip, &tag_str).0;
            let badge_border = if params.hovered == Some(PillHit::ModeBadge) {
                Some((theme.highlight.0, theme.highlight.1, theme.highlight.2, 240))
            } else {
                None
            };
            cv.fill_rrect(
                bx as f32,
                by as f32,
                bw as f32,
                d as f32,
                d as f32 / 2.0,
                (theme.accent.0, theme.accent.1, theme.accent.2, 255),
                badge_border,
            );
            let lh = line_height(f.chip);
            cv.draw_text(f.chip, &tag_str, bx + (bw - tag_w) / 2, by + (d - lh) / 2, theme.badge_text);
        }
    }

    // 2. Optional Center Text Area (Speed `字/分` and/or Subtitle)
    if let Some((tx0_log, tx1_log)) = layout.text_range {
        let lx = sp(tx0_log);
        let max_rx = sp(tx1_log);
        let ly = top + (ch - line_height(f.label)) / 2;

        let hover_tip: Option<(String, String)> = match params.hovered {
            Some(PillHit::SchemeToggle) => {
                let s_lbl = match params.input_scheme {
                    "quanpin" => " · 智能全拼",
                    "shuangpin" => " · 鹤羽双拼",
                    _ => " · 鹤羽音形",
                };
                Some(("编码方案".to_string(), s_lbl.to_string()))
            }
            Some(PillHit::TradToggle) => Some((
                "简繁切换".to_string(),
                if params.traditional_chinese { " · 繁体".to_string() } else { " · 简体".to_string() },
            )),
            Some(PillHit::PunctToggle) => Some((
                "标点模式".to_string(),
                if params.chinese_punctuation { " · 全角中文".to_string() } else { " · 半角英文".to_string() },
            )),
            Some(PillHit::ImmersionToggle) => Some(("外语伴学".to_string(), format!(" · {}", params.immersion_mode))),
            Some(PillHit::BlindToggle) => Some((
                "盲打模式".to_string(),
                if params.hide_candidate_hud { " · 已开启".to_string() } else { " · 已关闭".to_string() },
            )),
            Some(PillHit::VoiceInput) => Some(("语音/电脑转写".to_string(), " · 小窗".to_string())),
            Some(PillHit::StudioRecorder) => Some(("专业录音棚".to_string(), " · 48kHz".to_string())),
            Some(PillHit::QuickAdd) => {
                let hk = if params.quick_add_hotkey.trim().is_empty() {
                    "点击造词".to_string()
                } else {
                    params.quick_add_hotkey.trim().to_string()
                };
                Some(("快速造词".to_string(), format!(" · {}", hk)))
            }
            Some(PillHit::DictPage) => Some(("个人词库".to_string(), " · 加密管理".to_string())),
            Some(PillHit::ThemeCycle) => Some(("切换主题".to_string(), " · 15款艺术皮肤".to_string())),
            Some(PillHit::Settings) => Some(("设置中心".to_string(), " · 词库与主题".to_string())),
            Some(PillHit::ModeBadge) => Some(("切换语言".to_string(), " · Ctrl+Shift+L".to_string())),
            _ => None,
        };

        if let Some((title_s, sub_s)) = hover_tip {
            let lw = cv.draw_text(
                f.label,
                &ellipsize(f.label, &title_s, (max_rx - lx).max(sp(20.0))),
                lx,
                ly,
                theme.accent,
            );
            let avail = max_rx - (lx + lw);
            if avail > sp(16.0) {
                let drawn_sub = ellipsize(f.label, &sub_s, avail);
                cv.draw_text(f.label, &drawn_sub, lx + lw, ly, theme.text_secondary);
            }
        } else {
            let mut cur_tx = lx;
            if layout.has_cpm {
                let speed_str = format!("{} 字/分", params.cpm);
                let speed_color = if params.cpm >= 120 { theme.accent } else { theme.text_primary };
                let lw = cv.draw_text(
                    f.label,
                    &ellipsize(f.label, &speed_str, (max_rx - cur_tx).max(sp(20.0))),
                    cur_tx,
                    ly,
                    speed_color,
                );
                cur_tx += lw;
            }
            if layout.has_sub {
                let raw_sub = if !params.custom_sub.is_empty() {
                    params.custom_sub.to_string()
                } else if params.avatar.is_some() && !uname.is_empty() {
                    format!("{} · {}", uname, mode_long(params.mode))
                } else {
                    mode_long(params.mode).to_string()
                };
                let sub_str = if layout.has_cpm {
                    format!(" · {}", raw_sub)
                } else {
                    raw_sub
                };
                let avail = max_rx - cur_tx;
                if avail > sp(16.0) {
                    let drawn_sub = ellipsize(f.label, &sub_str, avail);
                    cv.draw_text(
                        f.label,
                        &drawn_sub,
                        cur_tx,
                        ly,
                        if layout.has_cpm { theme.text_muted } else { theme.text_primary },
                    );
                }
            }
        }
    }

    // 3. Optional Divider
    if let Some(div_log) = layout.divider_x {
        let dx = sp(div_log);
        cv.fill_rrect(
            dx as f32,
            (top + sp(8.0)) as f32,
            scale.max(1.0).round(),
            (ch - sp(16.0)) as f32,
            0.0,
            theme.divider,
            None,
        );
    }

    // 4. Composable Action Buttons
    let btn_d = (ch - sp(6.0)).max(d);
    let btn_y = top + (ch - btn_d) / 2;
    for &(hit, _, _, cx_log) in &layout.buttons {
        let gcx = sp(cx_log);
        let is_active = match hit {
            PillHit::TradToggle => params.traditional_chinese,
            PillHit::SchemeToggle => params.input_scheme == "quanpin" || params.input_scheme == "shuangpin",
            PillHit::PunctToggle => params.chinese_punctuation,
            PillHit::ImmersionToggle => !params.immersion_mode.eq_ignore_ascii_case("OFF"),
            PillHit::BlindToggle => params.hide_candidate_hud,
            _ => false,
        };
        let is_hovered = params.hovered == Some(hit);
        let bg_col = if is_active {
            (theme.accent.0, theme.accent.1, theme.accent.2, 230)
        } else if is_hovered {
            (theme.accent.0, theme.accent.1, theme.accent.2, 58)
        } else {
            theme.accent_soft
        };
        let border_col = if is_hovered {
            Some((theme.accent.0, theme.accent.1, theme.accent.2, 220))
        } else {
            None
        };
        cv.fill_rrect(
            (gcx - btn_d / 2) as f32,
            btn_y as f32,
            btn_d as f32,
            btn_d as f32,
            btn_d as f32 / 2.0,
            bg_col,
            border_col,
        );
        let active_fg = if is_active {
            theme.badge_text
        } else if is_hovered {
            theme.accent
        } else {
            theme.text_primary
        };
        let (glyph, font, fg_col): (&str, HFONT, Rgb) = match hit {
            PillHit::SchemeToggle => {
                let g = match params.input_scheme {
                    "quanpin" => "全",
                    "shuangpin" => "双",
                    _ => "音",
                };
                (g, f.chip, active_fg)
            }
            PillHit::TradToggle => (
                if params.traditional_chinese { "繁" } else { "简" },
                f.chip,
                active_fg,
            ),
            PillHit::PunctToggle => (
                if params.chinese_punctuation { "。" } else { "." },
                f.chip,
                active_fg,
            ),
            PillHit::ImmersionToggle => ("伴", f.chip, active_fg),
            PillHit::BlindToggle => ("盲", f.chip, active_fg),
            PillHit::VoiceInput => {
                if f.has_icon_font {
                    ("\u{E720}", f.icon, theme.accent)
                } else {
                    ("🎙", f.label, theme.accent)
                }
            }
            PillHit::StudioRecorder => (
                "◉",
                f.label,
                if is_hovered { theme.accent } else { theme.ghost_accent },
            ),
            PillHit::QuickAdd => {
                if f.has_icon_font {
                    ("\u{E70F}", f.icon, if is_hovered { theme.accent } else { theme.text_secondary })
                } else {
                    ("✏", f.label, if is_hovered { theme.accent } else { theme.text_secondary })
                }
            }
            PillHit::DictPage => ("词", f.chip, if is_hovered { theme.accent } else { theme.text_primary }),
            PillHit::ThemeCycle => ("彩", f.chip, if is_hovered { theme.accent } else { theme.ghost_accent }),
            PillHit::Settings | PillHit::ModeBadge | PillHit::DragBody => {
                if f.has_icon_font {
                    ("\u{E713}", f.icon, if is_hovered { theme.accent } else { theme.text_secondary })
                } else {
                    ("⚙", f.label, if is_hovered { theme.accent } else { theme.text_secondary })
                }
            }
        };
        let (gw, gh) = measure(font, glyph);
        cv.draw_text(font, glyph, gcx - gw / 2, btn_y + (btn_d - gh) / 2, fg_col);
    }
    cv
}

fn draw_small_card(cv: &mut OffscreenCanvas, x: f32, y: f32, w: f32, h: f32, r: f32, s: f32, theme: &ThemePalette) {
    // Same silky-smooth pre-composited corner anti-aliasing as draw_card with a tight shadow that fits the ~5px margin.
    let hair = s.round().max(1.0);
    let sa_max = theme.shadow as f32 / 255.0;
    let max_margin = x.min(y).max(2.0);
    for py in 0..cv.height {
        let fy = py as f32 + 0.5;
        let t = ((fy - y) / h).clamp(0.0, 1.0);
        let bg = mix(rgb(theme.bg_top), rgb(theme.bg_bottom), t);
        let ba = lerp(theme.bg_top.3 as f32, theme.bg_bottom.3 as f32, t) / 255.0;
        let top_w = (1.0 - (fy - y) / (h * 0.6)).clamp(0.0, 1.0);
        for px in 0..cv.width {
            let fx = px as f32 + 0.5;
            let d = sdf_rrect(fx, fy, x, y, w, h, r);
            let cov = smooth_cov(d);
            let idx = (py * cv.width + px) as usize;
            let mut p = cv.pixels[idx];
            if cov < 1.0 && sa_max > 0.0 {
                let ds = sdf_rrect(fx, fy, x, y + 1.0 * s, w, h, r);
                let u_win = (ds.max(0.0) / max_margin).clamp(0.0, 1.0);
                let win_fade = (1.0 - u_win * u_win) * (1.0 - u_win);
                let sa = 0.42 * sa_max * soft_edge(ds, 1.4 * s) * win_fade * (1.0 - cov);
                p = blend(p, (0, 0, 0), sa);
            }
            if cov > 0.0 {
                let mut c = bg;
                let cov_inner1 = smooth_cov(d + hair);
                let cov_inner2 = smooth_cov(d + 2.0 * hair);
                let border_w = (1.0 - cov_inner1).clamp(0.0, 1.0);
                let hi_w = (cov_inner1 - cov_inner2).clamp(0.0, 1.0);
                if hi_w > 0.0 && theme.highlight.3 > 0 {
                    let ha = (theme.highlight.3 as f32 / 255.0) * hi_w * (0.35 + 0.65 * top_w);
                    c = mix(c, rgb(theme.highlight), ha.clamp(0.0, 1.0));
                }
                let mut surf_a = ba;
                if border_w > 0.0 && theme.border.3 > 0 {
                    let bdr_a = theme.border.3 as f32 / 255.0;
                    let bt = (border_w * bdr_a).clamp(0.0, 1.0);
                    c = mix(c, rgb(theme.border), bt);
                    surf_a = lerp(ba, ba.max(bdr_a), border_w);
                }
                p = blend(p, c, surf_a * cov);
            }
            cv.pixels[idx] = p;
        }
    }
    cv.draw_theme_art(x, y, w, h, r, s, theme);
}

pub fn render_status_pill_image(mode: LangMode, cpm: u32, theme: &ThemePalette, fonts: &FontSet, scale: f32) -> RenderedImage {
    render_status_pill_canvas(mode, cpm, theme, fonts, scale).into_image()
}

/// X offset (relative to the pill window) where the settings/gear area begins.
#[allow(dead_code)]
pub fn status_pill_gear_x(pos: POINT, _font_scale: f32) -> i32 {
    (PILL_GEAR_X as f32 * effective_scale(dpi_for_point(pos), 1.0)).round() as i32
}

/// Render and present the draggable floating status pill.
pub fn update_status_pill(hwnd_pill: HWND, engine: &HeyuEngine, fonts: &FontSet, pos: POINT) {
    update_status_pill_with_hover(hwnd_pill, engine, fonts, pos, None);
}

/// Render and present the draggable floating status pill with optional hovered button state.
pub fn update_status_pill_with_hover(
    hwnd_pill: HWND,
    engine: &HeyuEngine,
    fonts: &FontSet,
    pos: POINT,
    hovered: Option<PillHit>,
) {
    if !engine.config.show_status_pill {
        unsafe {
            ShowWindow(hwnd_pill, SW_HIDE);
        }
        return;
    }
    let scale = effective_scale(dpi_for_point(pos), engine.config.pill_scale.clamp(0.70, 1.65));
    let theme = theme_at(engine.config.theme_index);
    let custom_sub = if engine.mode == LangMode::ZhFlypy
        && !engine.config.immersion_mode.eq_ignore_ascii_case("OFF")
        && !engine.last_immersion_tip.is_empty()
    {
        engine.last_immersion_tip.as_str()
    } else if engine.config.hide_candidate_hud && engine.mode == LangMode::ZhFlypy {
        "盲打"
    } else if engine.mode == LangMode::ZhFlypy {
        match engine.config.input_scheme.as_str() {
            "quanpin" => "智能全拼",
            "shuangpin" => "鹤羽双拼",
            _ => "",
        }
    } else {
        ""
    };
    let avatar_loaded = if engine.config.avatar_enabled {
        load_pill_avatar(&engine.data_dir)
    } else {
        None
    };
    let avatar_ref = avatar_loaded.as_ref().map(|(w, h, b)| (*w, *h, b.as_slice()));
    let params = PillRenderParams {
        mode: engine.mode,
        cpm: engine.current_cpm(),
        custom_sub,
        input_scheme: &engine.config.input_scheme,
        traditional_chinese: engine.config.traditional_chinese,
        chinese_punctuation: engine.config.chinese_punctuation,
        immersion_mode: &engine.config.immersion_mode,
        hide_candidate_hud: engine.config.hide_candidate_hud,
        pill_items: &engine.config.pill_items,
        hovered,
        quick_add_hotkey: &engine.config.quick_add_hotkey,
        user_name: &engine.config.user_name,
        avatar: avatar_ref,
        avatar_size_logical: engine.config.avatar_size,
    };
    let mut cv = render_status_pill_canvas_composable(&params, theme, fonts, scale);
    cv.present_to_layered_window(hwnd_pill, pos);
    unsafe {
        ShowWindow(hwnd_pill, SW_SHOWNOACTIVATE);
    }
}

/// Render and present a floating tooltip callout bubble right above (or below) the hovered
/// button on the status capsule so the user immediately sees its function and hotkey.
pub fn update_pill_tooltip(
    hwnd_tip: HWND,
    engine: &HeyuEngine,
    fonts: &FontSet,
    pill_pos: POINT,
    hovered: Option<PillHit>,
) {
    if hwnd_tip == 0 || !engine.config.show_status_pill {
        if hwnd_tip != 0 {
            unsafe {
                ShowWindow(hwnd_tip, SW_HIDE);
            }
        }
        return;
    }
    let Some(hit) = hovered else {
        unsafe {
            ShowWindow(hwnd_tip, SW_HIDE);
        }
        return;
    };
    if hit == PillHit::DragBody {
        unsafe {
            ShowWindow(hwnd_tip, SW_HIDE);
        }
        return;
    }

    let scale = effective_scale(dpi_for_point(pill_pos), engine.config.pill_scale.clamp(0.70, 1.65));
    let layout = compute_pill_layout(&engine.config.pill_items, estimate_badge_w_log_at_scale(engine, scale));
    let anchor_cx_logical = if hit == PillHit::ModeBadge {
        layout.badge_range.map(|(_, _, cx)| cx).unwrap_or(24.0)
    } else {
        layout
            .buttons
            .iter()
            .find(|&&(b, _, _, _)| b == hit)
            .map(|&(_, _, _, cx)| cx)
            .unwrap_or(layout.total_w_log * 0.5)
    };

    let hk = if engine.config.quick_add_hotkey.trim().is_empty() {
        "Ctrl+Alt+N"
    } else {
        engine.config.quick_add_hotkey.trim()
    };
    let (title_txt, badge_txt): (&str, String) = match hit {
        PillHit::SchemeToggle => (
            match engine.config.input_scheme.as_str() {
                "quanpin" => "中文编码方案 · 当前：智能全拼 (点击切双拼)",
                "shuangpin" => "中文编码方案 · 当前：鹤羽双拼 (点击切音形)",
                _ => "中文编码方案 · 当前：鹤羽音形 (点击切全拼)",
            },
            "一键切换".to_string(),
        ),
        PillHit::TradToggle => (
            if engine.config.traditional_chinese {
                "简繁切换 · 当前繁体输出"
            } else {
                "简繁切换 · 当前简体输出"
            },
            "点击切换".to_string(),
        ),
        PillHit::PunctToggle => (
            if engine.config.chinese_punctuation {
                "标点符号 · 当前：中文全角标点"
            } else {
                "标点符号 · 当前：英文半角标点"
            },
            "点击切换".to_string(),
        ),
        PillHit::ImmersionToggle => (
            "外语伴学提示切换",
            format!("当前 {}", engine.config.immersion_mode),
        ),
        PillHit::BlindToggle => (
            if engine.config.hide_candidate_hud {
                "盲打模式 · 当前已隐藏候选框"
            } else {
                "盲打模式 · 点击隐藏候选框"
            },
            "Ctrl+Shift+B".to_string(),
        ),
        PillHit::VoiceInput => ("🎙 语音/电脑声音转写 · 独立小窗", "Ctrl+Alt+V".to_string()),
        PillHit::StudioRecorder => ("◉ 48kHz 专业内录录音棚 · 独立小窗", "点击打开".to_string()),
        PillHit::QuickAdd => ("✏ 快速造词 · 添加个人词", hk.to_string()),
        PillHit::DictPage => ("📚 个人加密词库 · 管理与导入导出", "点击打开".to_string()),
        PillHit::ThemeCycle => (
            "🎨 切换候选框与胶囊主题外观",
            THEMES[engine.config.theme_index % THEMES.len()].name.to_string(),
        ),
        PillHit::Settings => ("⚙ 鹤羽设置中心 · 词库与主题", "点击打开".to_string()),
        PillHit::ModeBadge | PillHit::DragBody => ("切换中/意/西/英语言模式", "Ctrl+Shift+L".to_string()),
    };

    let scale = effective_scale(dpi_for_point(pill_pos), engine.config.pill_scale.clamp(0.70, 1.65));
    let sp = |v: f32| -> i32 { (v * scale).round() as i32 };
    let theme = theme_at(engine.config.theme_index);
    let f = fonts.scaled(scale);

    let (tw, th) = measure(f.label, title_txt);
    let (bw, bh) = measure(f.chip, &badge_txt);
    let inset = sp(4.0);
    let pad_x = sp(11.0);
    let gap = sp(8.0);
    let badge_pad_x = sp(7.0);
    let badge_h = bh + sp(4.0);
    let badge_w = bw + badge_pad_x * 2;
    let card_h = th.max(badge_h) + sp(12.0);
    let card_w = pad_x * 2 + tw + gap + badge_w;
    let w = card_w + inset * 2;
    let h = card_h + inset * 2;

    let mut cv = OffscreenCanvas::new(w, h);
    let mut t = *theme;
    t.shadow = (theme.shadow as f32 * 0.85) as u8;
    draw_small_card(
        &mut cv,
        inset as f32,
        inset as f32,
        card_w as f32,
        card_h as f32,
        sp(9.0) as f32,
        scale,
        &t,
    );

    let ty = inset + (card_h - th) / 2;
    cv.draw_text(f.label, title_txt, inset + pad_x, ty, theme.text_primary);

    let bx = inset + pad_x + tw + gap;
    let by = inset + (card_h - badge_h) / 2;
    cv.fill_rrect(
        bx as f32,
        by as f32,
        badge_w as f32,
        badge_h as f32,
        sp(5.0) as f32,
        theme.accent_soft,
        Some((theme.accent.0, theme.accent.1, theme.accent.2, 160)),
    );
    cv.draw_text(
        f.chip,
        &badge_txt,
        bx + badge_pad_x,
        by + (badge_h - bh) / 2,
        theme.accent,
    );

    let wa = work_area_for_point(pill_pos);
    let pill_h = sp(PILL_H as f32);
    let anchor_screen_x = pill_pos.x + sp(anchor_cx_logical);
    let mut tip_x = anchor_screen_x - w / 2;
    tip_x = tip_x.clamp(wa.left + sp(4.0), (wa.right - w - sp(4.0)).max(wa.left + sp(4.0)));
    let mut tip_y = pill_pos.y - h - sp(2.0);
    if tip_y < wa.top + sp(4.0) {
        tip_y = pill_pos.y + pill_h + sp(2.0);
    }

    cv.present_to_layered_window(hwnd_tip, POINT { x: tip_x, y: tip_y });
    unsafe {
        ShowWindow(hwnd_tip, SW_SHOWNOACTIVATE);
        SetWindowPos(
            hwnd_tip,
            HWND_TOPMOST,
            tip_x,
            tip_y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_are_well_formed() {
        assert!(THEMES.len() >= 8);
        let light = THEMES.iter().filter(|t| !t.is_dark).count();
        let dark = THEMES.iter().filter(|t| t.is_dark).count();
        assert!(light >= 4 && dark >= 4);
        for t in THEMES.iter() {
            assert!(t.name.contains(" · "));
            assert!(!t.slug.is_empty() && t.slug.is_ascii());
        }
        assert_eq!(theme_at(THEMES.len() + 1).slug, THEMES[1].slug);
    }

    #[test]
    fn style_parser() {
        assert_eq!(parse_hud_style("").layout, HudLayout::Horizontal);
        let s = parse_hud_style("{\"layout\": \"vertical\", \"glass\": false}");
        assert_eq!(s.layout, HudLayout::Vertical);
        assert!(!s.glass);
        assert!(parse_hud_style("{\"glass\":true}").glass);
    }

    #[test]
    fn blend_and_sdf() {
        assert_eq!(blend(0, (255, 255, 255), 1.0), 0xFFFF_FFFF);
        assert_eq!(blend(0x1234_5678 & 0, (0, 0, 0), 0.0), 0);
        assert!(sdf_rrect(5.0, 5.0, 0.0, 0.0, 10.0, 10.0, 2.0) < 0.0);
        assert!(sdf_rrect(-5.0, 5.0, 0.0, 0.0, 10.0, 10.0, 2.0) > 0.0);
        assert!((soft_edge(0.0, 3.0) - 0.5).abs() < 1e-3);
        assert!(soft_edge(20.0, 3.0) < 1e-3);
    }

    #[test]
    fn renders_offscreen() {
        let fonts = FontSet::new();
        let view = HudView {
            mode: LangMode::ZhFlypy,
            code: "hy".into(),
            ghost_tab: None,
            candidates: vec![("鹤羽".into(), String::new()), ("和".into(), ";".into())],
            selected: 0,
            page: 0,
            total_pages: 1,
            cpm: 0,
            shape_hints: vec![],
            post_commit_ghosts: vec![],
        };
        for t in THEMES.iter() {
            for layout in [HudLayout::Horizontal, HudLayout::Vertical] {
                let img = render_candidate_image(&view, t, &fonts, 1.0, layout, false);
                assert!(img.width > 100 && img.height > 40);
                assert!(img.pixels.iter().any(|p| (p >> 24) == 0xFF || (p >> 24) > 200));
            }
        }
        let pill = render_status_pill_image(LangMode::En, 0, &THEMES[4], &fonts, 1.5);
        assert!(pill.width >= 500 && pill.height == 66);
        let min_layout = compute_pill_layout("badge,settings", 29.0);
        assert!(min_layout.total_w_log < 110.0);
        assert_eq!(min_layout.buttons.len(), 1);
        assert_eq!(min_layout.buttons[0].0, PillHit::Settings);
    }

    #[test]
    fn pill_hit_test_matches_rendered_buttons_with_custom_username() {
        let fonts = FontSet::new();
        let origin = POINT { x: 800, y: 900 };
        let mut engine = HeyuEngine::new(std::path::Path::new("data"));
        engine.mode = LangMode::ZhFlypy;
        engine.config.avatar_enabled = false;
        engine.config.pill_items = "badge,cpm,voice,recorder,quick_add,theme,settings".to_string();

        for uname in ["Luca", "鹤羽音形", "A", ""] {
            engine.config.user_name = uname.to_string();
            for pill_scale in [0.80f32, 1.0f32, 1.25f32, 1.50f32] {
                engine.config.pill_scale = pill_scale;
                let scale = effective_scale(dpi_for_point(origin), pill_scale);
                let f = fonts.scaled(scale);
                let bw_render = compute_badge_w_log(
                    engine.mode,
                    &engine.config.user_name,
                    false,
                    engine.config.avatar_size,
                    f.chip,
                    scale,
                );
                let bw_hittest = estimate_badge_w_log_at_scale(&engine, scale);
                assert!(
                    (bw_render - bw_hittest).abs() < 1e-4,
                    "badge width mismatch for uname={}: render={} hittest={}",
                    uname,
                    bw_render,
                    bw_hittest
                );
                let layout = compute_pill_layout(&engine.config.pill_items, bw_render);
                // Every rendered button's exact pixel center and +-8px around it must hit that exact button!
                for &(expected_hit, _, _, cx_log) in &layout.buttons {
                    let gcx = (cx_log * scale).round() as i32;
                    for offset in [-6, -3, 0, 3, 6] {
                        let got = status_pill_hit_test_with_engine(origin, origin.x + gcx + offset, &engine);
                        assert_eq!(
                            got, expected_hit,
                            "hit test mismatch at offset {} for {:?} (uname={}, scale={})",
                            offset, expected_hit, uname, pill_scale
                        );
                    }
                }
                // Badge center must hit ModeBadge
                if let Some((_, _, bcx_log)) = layout.badge_range {
                    let bcx = (bcx_log * scale).round() as i32;
                    assert_eq!(
                        status_pill_hit_test_with_engine(origin, origin.x + bcx, &engine),
                        PillHit::ModeBadge
                    );
                }
            }
        }
    }
}
