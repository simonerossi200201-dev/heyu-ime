//! HEYU (鹤羽输入法) - ✏ 快速造词 Quick-Add Popup
//! ============================================
//! Runs as a **separate process** (`heyu-ime.exe --quick-add [--anchor x,y]`), launched by the
//! server on the global hotkey (default Ctrl+Alt+N) or the tray menu. Living in its own process
//! means the TSF DLL loaded into this window's process talks to the server over the named pipe
//! exactly like for any other app, so typing Chinese *into the popup with HEYU itself* works and
//! can never deadlock the server's UI thread (which services the pipe).
//!
//! * Two native EDIT controls (词句 / 编码) on a themed, DPI-aware, rounded (Win11) popup.
//! * The code is suggested live with the 音形 phrase rule ([`suggest_phrase_code`]); the hint
//!   line shows whether the code is free or which entries already use it.
//! * Enter = save (top of `user_custom.tsv` in every synced data dir), Esc = cancel, Tab cycles.
//! * Dictionaries are loaded on a background thread *after* the window is shown, so it opens
//!   in a few milliseconds.
//!
//! Re-entrancy: like `main.rs`, state is moved out of the thread-local cell while in use
//! ([`with_popup`]); anything that sends messages (SetWindowText, SetFocus, DestroyWindow...)
//! is called outside of it.

use crate::{launch_studio, WM_APP_RELOAD_USER};
use heyu_core::engine::{suggest_phrase_code, write_user_phrase_file};
use heyu_core::hud::{ThemePalette, THEMES};
use heyu_core::paths;
use heyu_core::win32::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const CLASS_NAME: &str = "HeyuQuickAddWindow";
const WM_APP_LOADED: u32 = WM_USER + 301;
const ES_LOWERCASE: u32 = 0x0010;
const ID_WORD: usize = 101;
const ID_CODE: usize = 102;
const TIMER_HOLD: usize = 1;
const TIMER_FADE: usize = 2;
const WORD_PREFILL_MAX_CHARS: usize = 40;

// Layout in logical (96-dpi) pixels.
const LW: i32 = 440;
const LH: i32 = 232;
const R_CLOSE: (i32, i32, i32, i32) = (392, 14, 420, 42);
const R_WORD: (i32, i32, i32, i32) = (70, 56, 418, 92);
const R_CODE: (i32, i32, i32, i32) = (70, 102, 212, 138);
const R_TOGGLE: (i32, i32, i32, i32) = (224, 102, 418, 138);
const R_HINT: (i32, i32, i32, i32) = (72, 142, 418, 166);
const Y_DIVIDER: i32 = 176;
const R_FOOT: (i32, i32, i32, i32) = (22, 186, 318, 216);
const R_SAVE: (i32, i32, i32, i32) = (326, 186, 418, 216);
const TOGGLE_SPLIT: f32 = 0.40;

pub struct QuickAddArgs {
    pub anchor: Option<POINT>,
    pub theme: Option<usize>,
    pub word: Option<String>,
    pub timing: bool,
}

#[derive(Default)]
struct Dicts {
    reverse: HashMap<String, String>,
    sys: HashMap<String, Vec<String>>,
    user: HashMap<String, Vec<String>>,
}

#[derive(Clone, Copy, PartialEq)]
enum HintKind {
    Muted,
    Ok,
    Warn,
    Err,
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    None,
    Close,
    Word,
    Code,
    Direct,
    Cand,
    Link,
    Save,
}

struct Fonts {
    title: HFONT,
    label: HFONT,
    edit: HFONT,
    hint: HFONT,
    small: HFONT,
    button: HFONT,
    big: HFONT,
    icon: HFONT,
}

struct Popup {
    hwnd: HWND,
    edit_word: HWND,
    edit_code: HWND,
    pal: ThemePalette,
    fonts: Fonts,
    dicts: Option<Box<Dicts>>,
    code_overridden: bool,
    mode_overridden: bool,
    direct: bool,
    focus: u8, // 0 word, 1 code, 2 mode toggle
    hint: String,
    hint_kind: HintKind,
    saved: Option<(String, String, bool)>,
    alpha: i32,
    base_dir: PathBuf,
    data_dir: PathBuf,
    t0: Instant,
    timing: bool,
}

thread_local! {
    static POPUP: RefCell<Option<Popup>> = RefCell::new(None);
    /// Set while we change an EDIT's text programmatically (its EN_CHANGE is not a user edit).
    static SUPPRESS: Cell<bool> = Cell::new(false);
    static DPI: Cell<u32> = Cell::new(96);
    static LINK_RECT: Cell<RECT> = Cell::new(RECT::default());
    static SAVED: Cell<bool> = Cell::new(false);
    /// (text colour, background colour, background brush) for WM_CTLCOLOREDIT.
    static EDIT_COLORS: Cell<(DWORD, DWORD, HBRUSH)> = Cell::new((0, 0xFFFFFF, 0));
}

fn with_popup<R>(f: impl FnOnce(&mut Popup) -> R) -> Option<R> {
    let mut st = POPUP.with(|c| c.try_borrow_mut().ok().and_then(|mut g| g.take()))?;
    let r = f(&mut st);
    POPUP.with(|c| {
        if let Ok(mut g) = c.try_borrow_mut() {
            *g = Some(st);
        }
    });
    Some(r)
}

// ---------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------
type Rgb = (u8, u8, u8);

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

fn cref(c: Rgb) -> DWORD {
    rgb(c.0, c.1, c.2)
}

struct Colors {
    bg: Rgb,
    field: Rgb,
    field_border: Rgb,
    accent: Rgb,
    on_accent: Rgb,
    text: Rgb,
    label: Rgb,
    muted: Rgb,
    sel_bg: Rgb,
    sel_text: Rgb,
    ok: Rgb,
    warn: Rgb,
    err: Rgb,
    divider: Rgb,
}

fn colors(p: &ThemePalette) -> Colors {
    let bg = (p.bg_top.0, p.bg_top.1, p.bg_top.2);
    let bg = mix(bg, (p.bg_bottom.0, p.bg_bottom.1, p.bg_bottom.2), 0.5);
    let (white, black) = ((255, 255, 255), (0, 0, 0));
    if p.is_dark {
        let field = mix(bg, white, 0.065);
        Colors {
            bg,
            field,
            field_border: mix(bg, white, 0.13),
            accent: p.accent,
            on_accent: p.badge_text,
            text: p.text_primary,
            label: p.text_secondary,
            muted: p.text_muted,
            sel_bg: mix(field, p.accent, 0.26),
            sel_text: p.selected_text,
            ok: (110, 211, 150),
            warn: (238, 186, 92),
            err: (240, 110, 110),
            divider: mix(bg, white, 0.09),
        }
    } else {
        let field = mix(bg, black, 0.032);
        Colors {
            bg,
            field,
            field_border: mix(bg, black, 0.11),
            accent: p.accent,
            on_accent: p.badge_text,
            text: p.text_primary,
            label: p.text_secondary,
            muted: p.text_muted,
            sel_bg: mix(field, p.accent, 0.16),
            sel_text: mix(p.accent, black, 0.15),
            ok: (28, 140, 82),
            warn: (184, 112, 14),
            err: (200, 52, 52),
            divider: mix(bg, black, 0.075),
        }
    }
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------
fn px(dpi: u32, v: i32) -> i32 {
    (v * dpi as i32 + 48) / 96
}

fn lrect(dpi: u32, r: (i32, i32, i32, i32)) -> RECT {
    RECT { left: px(dpi, r.0), top: px(dpi, r.1), right: px(dpi, r.2), bottom: px(dpi, r.3) }
}

fn in_rect(r: &RECT, pt: POINT) -> bool {
    pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom
}

fn toggle_halves(dpi: u32) -> (RECT, RECT) {
    let t = lrect(dpi, R_TOGGLE);
    let mid = t.left + ((t.right - t.left) as f32 * TOGGLE_SPLIT) as i32;
    (RECT { right: mid, ..t }, RECT { left: mid, ..t })
}

fn hit_test(pt: POINT) -> Hit {
    let dpi = DPI.with(|d| d.get());
    let (direct, cand) = toggle_halves(dpi);
    if in_rect(&lrect(dpi, R_CLOSE), pt) {
        Hit::Close
    } else if in_rect(&lrect(dpi, R_WORD), pt) {
        Hit::Word
    } else if in_rect(&lrect(dpi, R_CODE), pt) {
        Hit::Code
    } else if in_rect(&direct, pt) {
        Hit::Direct
    } else if in_rect(&cand, pt) {
        Hit::Cand
    } else if in_rect(&lrect(dpi, R_SAVE), pt) {
        Hit::Save
    } else if in_rect(&LINK_RECT.with(|l| l.get()), pt) {
        Hit::Link
    } else {
        Hit::None
    }
}

fn lparam_point(l: LPARAM) -> POINT {
    POINT { x: (l & 0xFFFF) as u16 as i16 as i32, y: ((l >> 16) & 0xFFFF) as u16 as i16 as i32 }
}

// ---------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------
fn first_existing(dir: &Path, names: &[&str]) -> PathBuf {
    for n in names {
        let p = dir.join(n);
        if p.exists() {
            return p;
        }
    }
    dir.join(names[0])
}

fn load_dicts(dir: &Path) -> Dicts {
    let mut d = Dicts::default();
    if let Ok(s) = fs::read_to_string(first_existing(dir, &["heyu_reverse.tsv", "flypy_reverse.tsv"])) {
        d.reverse.reserve(12_000);
        for line in s.lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut p = line.split('\t');
            if let (Some(c), Some(k)) = (p.next(), p.next()) {
                d.reverse.insert(c.to_string(), k.to_string());
            }
        }
    }
    if let Ok(s) = fs::read_to_string(first_existing(dir, &["heyu_sys.tsv", "flypy_sys.tsv"])) {
        d.sys.reserve(72_000);
        for line in s.lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut p = line.split('\t');
            if let (Some(c), Some(w)) = (p.next(), p.next()) {
                d.sys.entry(c.to_string()).or_default().push(w.to_string());
            }
        }
    }
    if let Some(s) = heyu_core::vault::read_user_lexicon_decrypted(&dir.join("user_custom.tsv")) {
        for line in s.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let mut p = line.split('\t');
            if let (Some(c), Some(w)) = (p.next(), p.next()) {
                d.user.entry(c.trim().to_string()).or_default().push(w.trim().to_string());
            }
        }
    }
    d
}

fn read_theme_index(data_dir: &Path) -> usize {
    let txt = fs::read_to_string(data_dir.join("config.json")).unwrap_or_default();
    if let Some(i) = txt.find("\"theme_index\"") {
        let rest = &txt[i + 13..];
        if let Some(c) = rest.find(':') {
            let n: String = rest[c + 1..].trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
            return n.parse().unwrap_or(0);
        }
    }
    0
}

/// Every data directory that is kept in sync (engine's data dir first).
pub fn sync_data_dirs(base_dir: &Path, data_dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut cands = vec![data_dir.to_path_buf(), base_dir.join("data")];
    if let Some(root) = paths::appdata_root() {
        cands.push(root.join("data"));
    }
    for (i, d) in cands.into_iter().enumerate() {
        if i > 0 && !d.is_dir() {
            continue;
        }
        let key = fs::canonicalize(&d).unwrap_or_else(|_| d.clone());
        if !seen.contains(&key) {
            seen.push(key);
            out.push(d);
        }
    }
    out
}

fn escape_word(w: &str) -> String {
    w.trim().replace('\r', "").replace('\n', "\\n").replace('\t', " ")
}

fn compute_hint(p: &Popup, word: &str, code: &str) -> (String, HintKind) {
    let Some(d) = p.dicts.as_ref() else {
        return ("正在载入词库…".to_string(), HintKind::Muted);
    };
    let word_t = word.trim();
    if code.is_empty() {
        if word_t.is_empty() {
            return ("输入词句后自动生成编码，也可手动修改".to_string(), HintKind::Muted);
        }
        return ("⚠ 未能自动推导编码，请手动输入".to_string(), HintKind::Warn);
    }
    if !code.chars().all(|c| c.is_ascii_lowercase() || c == ';') {
        return ("✕ 编码只能包含字母 a-z".to_string(), HintKind::Err);
    }
    let esc = escape_word(word_t);
    let user = d.user.get(code).map(|v| v.as_slice()).unwrap_or(&[]);
    let sys = d.sys.get(code).map(|v| v.as_slice()).unwrap_or(&[]);
    if !esc.is_empty() && user.iter().any(|w| *w == esc) {
        return (format!("↺ 「{}」已在 {}，保存将置顶为第 1 位", word_t, code), HintKind::Muted);
    }
    let mut others: Vec<&str> = Vec::new();
    for w in user.iter().chain(sys.iter()) {
        if *w != esc && !others.contains(&w.as_str()) {
            others.push(w);
        }
    }
    if others.is_empty() {
        return ("✓ 编码空闲".to_string(), HintKind::Ok);
    }
    let shown: Vec<String> = others
        .iter()
        .take(3)
        .map(|w| {
            let w = w.replace("\\n", "⏎");
            if w.chars().count() > 8 {
                format!("{}…", w.chars().take(7).collect::<String>())
            } else {
                w
            }
        })
        .collect();
    let more = if others.len() > 3 { format!(" 等{}个", others.len()) } else { String::new() };
    (format!("⚠ {} 已有：{}{}（新词将排在第1位）", code, shown.join("、"), more), HintKind::Warn)
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------
unsafe fn make_font(dpi: u32, size: f32, weight: i32) -> HFONT {
    let face = to_wide("Microsoft YaHei UI");
    CreateFontW(
        -((size * dpi as f32 / 96.0).round() as i32),
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
        CLEARTYPE_QUALITY,
        DEFAULT_PITCH,
        face.as_ptr(),
    )
}

unsafe fn make_fonts(dpi: u32) -> Fonts {
    Fonts {
        title: make_font(dpi, 15.0, FW_SEMIBOLD),
        label: make_font(dpi, 12.5, FW_NORMAL),
        edit: make_font(dpi, 14.5, FW_NORMAL),
        hint: make_font(dpi, 12.0, FW_NORMAL),
        small: make_font(dpi, 11.5, FW_NORMAL),
        button: make_font(dpi, 13.0, FW_SEMIBOLD),
        big: make_font(dpi, 16.5, FW_SEMIBOLD),
        icon: make_font(dpi, 20.0, FW_BOLD),
    }
}

unsafe fn delete_fonts(f: &Fonts) {
    for h in [f.title, f.label, f.edit, f.hint, f.small, f.button, f.big, f.icon] {
        if h != 0 {
            DeleteObject(h);
        }
    }
}

unsafe fn round_rect(hdc: HDC, r: RECT, radius: i32, fill: Rgb, border: Option<Rgb>) {
    let brush = CreateSolidBrush(cref(fill));
    let pen = match border {
        Some(c) => CreatePen(PS_SOLID, 1, cref(c)),
        None => GetStockObject(NULL_PEN),
    };
    let ob = SelectObject(hdc, brush);
    let op = SelectObject(hdc, pen);
    let extra = if border.is_none() { 1 } else { 0 };
    RoundRect(hdc, r.left, r.top, r.right + extra, r.bottom + extra, radius * 2, radius * 2);
    SelectObject(hdc, ob);
    SelectObject(hdc, op);
    DeleteObject(brush);
    if border.is_some() {
        DeleteObject(pen);
    }
}

unsafe fn draw_text(hdc: HDC, font: HFONT, color: Rgb, s: &str, r: RECT, flags: u32) {
    let w: Vec<u16> = s.encode_utf16().collect();
    let mut r = r;
    SelectObject(hdc, font);
    SetTextColor(hdc, cref(color));
    DrawTextW(hdc, w.as_ptr(), w.len() as i32, &mut r, flags | DT_NOPREFIX);
}

unsafe fn text_width(hdc: HDC, font: HFONT, s: &str) -> i32 {
    let w: Vec<u16> = s.encode_utf16().collect();
    let mut r = RECT { left: 0, top: 0, right: 10_000, bottom: 1000 };
    SelectObject(hdc, font);
    DrawTextW(hdc, w.as_ptr(), w.len() as i32, &mut r, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
    r.right - r.left
}

unsafe fn paint(p: &Popup, hdc: HDC, w: i32, h: i32) -> RECT {
    let dpi = DPI.with(|d| d.get());
    let c = colors(&p.pal);
    let s = |v: i32| px(dpi, v);
    let bgb = CreateSolidBrush(cref(c.bg));
    FillRect(hdc, &RECT { left: 0, top: 0, right: w, bottom: h }, bgb);
    DeleteObject(bgb);
    SetBkMode(hdc, TRANSPARENT);
    let vc = DT_SINGLELINE | DT_VCENTER;

    if let Some((code, word, direct)) = &p.saved {
        // ---- success toast ----
        let d = s(44);
        let cx = w / 2;
        let icon = RECT { left: cx - d / 2, top: s(38), right: cx + d / 2, bottom: s(38) + d };
        round_rect(hdc, icon, d / 2, c.accent, None);
        draw_text(hdc, p.fonts.icon, c.on_accent, "✓", icon, vc | DT_CENTER);
        draw_text(hdc, p.fonts.big, c.text, "已添加到个人词库", RECT { left: 0, top: s(94), right: w, bottom: s(122) }, vc | DT_CENTER);
        let shown = if word.chars().count() > 16 { format!("{}…", word.chars().take(15).collect::<String>()) } else { word.clone() };
        let line = format!("{}  →  {}", code, shown);
        draw_text(hdc, p.fonts.edit, c.accent, &line, RECT { left: s(16), top: s(126), right: w - s(16), bottom: s(154) }, vc | DT_CENTER | DT_END_ELLIPSIS);
        let sub = if *direct { "直出 · 敲完编码自动上屏" } else { "候选首选 · 排在候选第 1 位" };
        draw_text(hdc, p.fonts.hint, c.muted, sub, RECT { left: 0, top: s(158), right: w, bottom: s(180) }, vc | DT_CENTER);
        return RECT::default();
    }

    // ---- header ----
    let ix = s(22);
    draw_text(hdc, p.fonts.title, c.accent, "✏", RECT { left: ix, top: s(14), right: ix + s(24), bottom: s(42) }, vc);
    let tx = ix + s(24);
    draw_text(hdc, p.fonts.title, c.text, "快速造词", RECT { left: tx, top: s(14), right: w, bottom: s(42) }, vc);
    let tw = text_width(hdc, p.fonts.title, "快速造词");
    draw_text(hdc, p.fonts.small, c.muted, "鹤羽音形 · 个人词库", RECT { left: tx + tw + s(10), top: s(16), right: w, bottom: s(42) }, vc);
    let close = lrect(dpi, R_CLOSE);
    draw_text(hdc, p.fonts.big, c.muted, "×", close, vc | DT_CENTER);

    // ---- fields ----
    let radius = s(8);
    let word_r = lrect(dpi, R_WORD);
    let code_r = lrect(dpi, R_CODE);
    let fb = |focused: bool| if focused { c.accent } else { c.field_border };
    round_rect(hdc, word_r, radius, c.field, Some(fb(p.focus == 0)));
    round_rect(hdc, code_r, radius, c.field, Some(fb(p.focus == 1)));
    let lab = |r: RECT| RECT { left: s(22), top: r.top, right: r.left - s(6), bottom: r.bottom };
    draw_text(hdc, p.fonts.label, c.label, "词句", lab(word_r), vc);
    draw_text(hdc, p.fonts.label, c.label, "编码", lab(code_r), vc);

    // ---- mode toggle ----
    let tog = lrect(dpi, R_TOGGLE);
    round_rect(hdc, tog, radius, c.field, Some(if p.focus == 2 { c.accent } else { c.field_border }));
    let (dr, cr) = toggle_halves(dpi);
    let inset = s(4);
    let sel = if p.direct { dr } else { cr };
    let sel = RECT { left: sel.left + inset, top: sel.top + inset, right: sel.right - inset, bottom: sel.bottom - inset };
    round_rect(hdc, sel, s(6), c.sel_bg, None);
    let (dc, cc) = if p.direct { (c.sel_text, c.label) } else { (c.label, c.sel_text) };
    draw_text(hdc, if p.direct { p.fonts.button } else { p.fonts.label }, dc, "直出", dr, vc | DT_CENTER);
    draw_text(hdc, if p.direct { p.fonts.label } else { p.fonts.button }, cc, "候选首选", cr, vc | DT_CENTER);

    // ---- hint ----
    let hc = match p.hint_kind {
        HintKind::Muted => c.muted,
        HintKind::Ok => c.ok,
        HintKind::Warn => c.warn,
        HintKind::Err => c.err,
    };
    draw_text(hdc, p.fonts.hint, hc, &p.hint, lrect(dpi, R_HINT), vc | DT_END_ELLIPSIS);

    // ---- divider + footer ----
    let divb = CreateSolidBrush(cref(c.divider));
    FillRect(hdc, &RECT { left: s(22), top: s(Y_DIVIDER), right: w - s(22), bottom: s(Y_DIVIDER) + 1.max(dpi as i32 / 96) }, divb);
    DeleteObject(divb);
    let foot = lrect(dpi, R_FOOT);
    let pre = "支持 {date} {time} {weekday} 等参数 · ";
    draw_text(hdc, p.fonts.small, c.muted, pre, foot, vc);
    let pw = text_width(hdc, p.fonts.small, pre);
    let link = "完整词库管理 ›";
    let lw = text_width(hdc, p.fonts.small, link);
    let link_r = RECT { left: foot.left + pw, top: foot.top, right: foot.left + pw + lw, bottom: foot.bottom };
    draw_text(hdc, p.fonts.small, c.accent, link, link_r, vc);
    let save = lrect(dpi, R_SAVE);
    round_rect(hdc, save, s(8), c.accent, None);
    let (bw, kw, gap) = (text_width(hdc, p.fonts.button, "保存"), text_width(hdc, p.fonts.small, "Enter"), s(6));
    let bx = save.left + ((save.right - save.left) - (bw + gap + kw)) / 2;
    draw_text(hdc, p.fonts.button, c.on_accent, "保存", RECT { left: bx, ..save }, vc);
    draw_text(hdc, p.fonts.small, mix(c.on_accent, c.accent, 0.38), "Enter", RECT { left: bx + bw + gap, top: save.top + s(1), ..save }, vc);
    link_r
}

// ---------------------------------------------------------------------------
// Behaviour
// ---------------------------------------------------------------------------
unsafe fn set_text_quiet(h: HWND, s: &str) {
    let w = to_wide(s);
    SUPPRESS.with(|f| f.set(true));
    SetWindowTextW(h, w.as_ptr());
    SUPPRESS.with(|f| f.set(false));
}

fn handles() -> Option<(HWND, HWND, HWND)> {
    with_popup(|p| (p.hwnd, p.edit_word, p.edit_code))
}

/// Recompute the auto-mode and the hint line from the current field contents.
fn refresh_hint() {
    let Some((hwnd, ew, ec)) = handles() else { return };
    let word = window_text(ew);
    let code = window_text(ec).trim().to_string();
    with_popup(|p| {
        if !p.mode_overridden {
            p.direct = code.chars().count() == 4;
        }
        let (h, k) = compute_hint(p, &word, &code);
        p.hint = h;
        p.hint_kind = k;
    });
    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
}

fn on_word_changed() {
    let Some((_, ew, ec)) = handles() else { return };
    let word = window_text(ew);
    let new_code = with_popup(|p| {
        if p.code_overridden {
            return None;
        }
        let sugg = p.dicts.as_ref().and_then(|d| suggest_phrase_code(&d.reverse, &word));
        Some(sugg.unwrap_or_default())
    })
    .flatten();
    if let Some(code) = new_code {
        if window_text(ec) != code {
            unsafe { set_text_quiet(ec, &code) };
        }
    }
    refresh_hint();
}

fn on_code_changed() {
    let Some((_, _, ec)) = handles() else { return };
    let code = window_text(ec);
    with_popup(|p| p.code_overridden = !code.trim().is_empty());
    refresh_hint();
}

fn set_mode(direct: bool) {
    let hwnd = with_popup(|p| {
        p.direct = direct;
        p.mode_overridden = true;
        p.hwnd
    });
    if let Some(h) = hwnd {
        unsafe { InvalidateRect(h, std::ptr::null(), 0) };
    }
}

fn focus_index(i: u8) {
    let Some((hwnd, ew, ec)) = handles() else { return };
    unsafe {
        match i {
            0 => {
                SetFocus(ew);
                SendMessageW(ew, EM_SETSEL, 0, -1);
            }
            1 => {
                SetFocus(ec);
                SendMessageW(ec, EM_SETSEL, 0, -1);
            }
            _ => {
                SetFocus(hwnd);
            }
        }
    }
    with_popup(|p| p.focus = i);
    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
}

fn close_popup() {
    if let Some((hwnd, _, _)) = handles() {
        unsafe { DestroyWindow(hwnd) };
    }
}

fn open_dict_manager() {
    if let Some(dir) = with_popup(|p| p.base_dir.clone()) {
        launch_studio(&dir, Some("dict"));
    }
    close_popup();
}

fn save() {
    let Some((hwnd, ew, ec)) = handles() else { return };
    let word = window_text(ew);
    let code = window_text(ec).trim().to_string();
    let word_t = word.trim().to_string();
    // Validate.
    let err: Option<(&str, u8)> = if word_t.is_empty() {
        Some(("✕ 请先输入要添加的词句", 0))
    } else if code.is_empty() {
        Some(("✕ 请输入编码", 1))
    } else if !code.chars().all(|c| c.is_ascii_lowercase() || c == ';') {
        Some(("✕ 编码只能包含字母 a-z", 1))
    } else {
        None
    };
    if let Some((msg, field)) = err {
        with_popup(|p| {
            p.hint = msg.to_string();
            p.hint_kind = HintKind::Err;
        });
        focus_index(field);
        return;
    }
    let Some((direct, base_dir, data_dir)) = with_popup(|p| (p.direct, p.base_dir.clone(), p.data_dir.clone())) else { return };
    let comment = if direct { "直出" } else { "候选首选" };
    let mut primary_ok = false;
    for (i, d) in sync_data_dirs(&base_dir, &data_dir).iter().enumerate() {
        let ok = write_user_phrase_file(&d.join("user_custom.tsv"), &code, &word_t, comment);
        if i == 0 {
            primary_ok = ok;
        }
    }
    if !primary_ok {
        with_popup(|p| {
            p.hint = format!("✕ 无法写入 {}", data_dir.join("user_custom.tsv").display());
            p.hint_kind = HintKind::Err;
        });
        unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
        return;
    }
    // Tell the running server to hot-reload right away (it also polls the file mtime).
    unsafe {
        let cls = to_wide("HeyuLayeredWindowClass");
        let title = to_wide("HeyuStatusPillWindow");
        let server = FindWindowW(cls.as_ptr(), title.as_ptr());
        if server != 0 {
            PostMessageW(server, WM_APP_RELOAD_USER, 0, 0);
        }
    }
    with_popup(|p| p.saved = Some((code.clone(), word_t.clone(), direct)));
    SAVED.with(|s| s.set(true));
    unsafe {
        ShowWindow(ew, SW_HIDE);
        ShowWindow(ec, SW_HIDE);
        SetFocus(hwnd);
        InvalidateRect(hwnd, std::ptr::null(), 0);
        SetTimer(hwnd, TIMER_HOLD, 1100, std::ptr::null());
    }
}

/// Keys handled before dispatch (Enter / Esc / Tab / mode toggle / Ctrl+A).
fn handle_key(vk: i32) -> bool {
    if SAVED.with(|s| s.get()) {
        if vk == VK_ESCAPE || vk == VK_RETURN || vk == VK_SPACE {
            close_popup();
            return true;
        }
        return false;
    }
    let ctrl = unsafe { (GetKeyState(VK_CONTROL) as u16 & 0x8000) != 0 };
    let shift = unsafe { (GetKeyState(VK_SHIFT) as u16 & 0x8000) != 0 };
    let focus = with_popup(|p| p.focus).unwrap_or(0);
    match vk {
        VK_ESCAPE => {
            close_popup();
            true
        }
        VK_RETURN => {
            save();
            true
        }
        VK_TAB => {
            let next = if shift { (focus + 2) % 3 } else { (focus + 1) % 3 };
            focus_index(next);
            true
        }
        VK_LEFT | VK_RIGHT | VK_SPACE if focus == 2 => {
            let cur = with_popup(|p| p.direct).unwrap_or(false);
            let direct = match vk {
                VK_LEFT => true,
                VK_RIGHT => false,
                _ => !cur,
            };
            set_mode(direct);
            true
        }
        0x41 if ctrl && focus < 2 => {
            if let Some((_, ew, ec)) = handles() {
                unsafe { SendMessageW(if focus == 0 { ew } else { ec }, EM_SETSEL, 0, -1) };
            }
            true
        }
        _ => false,
    }
}

unsafe fn force_foreground(hwnd: HWND) {
    if IsIconic(hwnd) != 0 {
        ShowWindow(hwnd, SW_RESTORE);
    }
    SetForegroundWindow(hwnd);
    if GetForegroundWindow() != hwnd {
        let fg = GetForegroundWindow();
        let fg_tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let me = GetCurrentThreadId();
        if fg_tid != 0 && fg_tid != me {
            AttachThreadInput(me, fg_tid, 1);
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
            AttachThreadInput(me, fg_tid, 0);
        }
    }
}

/// Bring an already-open quick-add popup to the front. Returns false if none exists.
pub fn activate_existing() -> bool {
    unsafe {
        let cls = to_wide(CLASS_NAME);
        let h = FindWindowW(cls.as_ptr(), std::ptr::null());
        if h == 0 {
            return false;
        }
        force_foreground(h);
        true
    }
}

unsafe fn layout_edits(hwnd: HWND, ew: HWND, ec: HWND, font: HFONT, dpi: u32) {
    // Height of one text line in the edit font.
    let hdc = GetDC(hwnd);
    let old = SelectObject(hdc, font);
    let probe: Vec<u16> = "国Ag".encode_utf16().collect();
    let mut sz = SIZE::default();
    GetTextExtentPoint32W(hdc, probe.as_ptr(), probe.len() as i32, &mut sz);
    SelectObject(hdc, old);
    ReleaseDC(hwnd, hdc);
    let eh = sz.cy + 2;
    for (h, r) in [(ew, R_WORD), (ec, R_CODE)] {
        let r = lrect(dpi, r);
        let pad = px(dpi, 12);
        let y = r.top + ((r.bottom - r.top) - eh) / 2;
        MoveWindow(h, r.left + pad, y, (r.right - r.left) - 2 * pad, eh, 1);
        SendMessageW(h, WM_SETFONT, font as WPARAM, 1);
        SendMessageW(h, EM_SETMARGINS, EC_LEFTMARGIN | EC_RIGHTMARGIN, 0);
    }
}

fn work_area_at(pt: POINT) -> RECT {
    unsafe {
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as DWORD, ..Default::default() };
        if hmon != 0 && GetMonitorInfoW(hmon, &mut mi) != 0 {
            mi.rcWork
        } else {
            RECT { left: 0, top: 0, right: GetSystemMetrics(SM_CXSCREEN), bottom: GetSystemMetrics(SM_CYSCREEN) }
        }
    }
}

/// Where to put the popup: under the caret if known, otherwise centred on the foreground window.
fn target_point(anchor: Option<POINT>) -> (POINT, bool) {
    if let Some(a) = anchor {
        return (a, true);
    }
    unsafe {
        let fg = GetForegroundWindow();
        let mut r = RECT::default();
        if fg != 0 && GetWindowRect(fg, &mut r) != 0 && r.right - r.left > 100 && r.bottom - r.top > 80 {
            return (POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 }, false);
        }
        let mut c = POINT::default();
        GetCursorPos(&mut c);
        let wa = work_area_at(c);
        (POINT { x: (wa.left + wa.right) / 2, y: (wa.top + wa.bottom) / 2 }, false)
    }
}

fn place(target: POINT, is_caret: bool, w: i32, h: i32, dpi: u32) -> POINT {
    let wa = work_area_at(target);
    let m = px(dpi, 8);
    let (mut x, mut y) = if is_caret {
        let below = target.y + px(dpi, 14);
        let y = if below + h > wa.bottom - m { target.y - px(dpi, 40) - h } else { below };
        (target.x - w / 2, y)
    } else {
        (target.x - w / 2, target.y - h / 2 - px(dpi, 40))
    };
    x = x.clamp(wa.left + m, (wa.right - w - m).max(wa.left + m));
    y = y.clamp(wa.top + m, (wa.bottom - h - m).max(wa.top + m));
    POINT { x, y }
}

fn log_timing(data_dir: &Path, msg: &str) {
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data_dir.join("quickadd_timing.log"))
        .and_then(|mut f| std::io::Write::write_all(&mut f, format!("{}\n", msg).as_bytes()));
}

unsafe fn apply_dwm(hwnd: HWND, pal: &ThemePalette) {
    let c = colors(pal);
    let corner: u32 = DWMWCP_ROUND;
    DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const u32 as *const _, 4);
    let border: u32 = cref(if pal.is_dark { mix(c.bg, (255, 255, 255), 0.16) } else { mix(c.bg, (0, 0, 0), 0.16) });
    DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border as *const u32 as *const _, 4);
    let dark: u32 = pal.is_dark as u32;
    DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const u32 as *const _, 4);
}

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: UINT, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc);
            let (cw, ch) = (rc.right - rc.left, rc.bottom - rc.top);
            let mem = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, cw.max(1), ch.max(1));
            let old = SelectObject(mem, bmp);
            if let Some(link) = with_popup(|p| paint(p, mem, cw, ch)) {
                LINK_RECT.with(|c| c.set(link));
                BitBlt(hdc, 0, 0, cw, ch, mem, 0, 0, SRCCOPY);
            }
            SelectObject(mem, old);
            DeleteObject(bmp);
            DeleteDC(mem);
            EndPaint(hwnd, &ps);
            return 0;
        }
        WM_CTLCOLOREDIT => {
            let (fg, bg, brush) = EDIT_COLORS.with(|c| c.get());
            SetTextColor(w as HDC, fg);
            SetBkColor(w as HDC, bg);
            return brush;
        }
        WM_COMMAND => {
            let id = w & 0xFFFF;
            let code = ((w >> 16) & 0xFFFF) as u32;
            match code {
                EN_CHANGE if !SUPPRESS.with(|s| s.get()) => {
                    if id == ID_WORD {
                        on_word_changed();
                    } else if id == ID_CODE {
                        on_code_changed();
                    }
                }
                EN_SETFOCUS => {
                    let f = if id == ID_WORD { 0 } else { 1 };
                    with_popup(|p| p.focus = f);
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                _ => {}
            }
            return 0;
        }
        WM_NCHITTEST => {
            if SAVED.with(|s| s.get()) {
                return HTCLIENT;
            }
            let mut pt = lparam_point(l);
            ScreenToClient(hwnd, &mut pt);
            return if hit_test(pt) == Hit::None { HTCAPTION } else { HTCLIENT };
        }
        WM_SETCURSOR => {
            if w as HWND == hwnd && !SAVED.with(|s| s.get()) {
                let mut pt = POINT::default();
                GetCursorPos(&mut pt);
                ScreenToClient(hwnd, &mut pt);
                let hand = matches!(hit_test(pt), Hit::Close | Hit::Direct | Hit::Cand | Hit::Link | Hit::Save);
                let ibeam = matches!(hit_test(pt), Hit::Word | Hit::Code);
                if hand || ibeam {
                    SetCursor(LoadCursorW(0, if hand { IDC_HAND } else { 32513usize as *const u16 }));
                    return 1;
                }
            }
        }
        WM_LBUTTONDOWN => {
            if SAVED.with(|s| s.get()) {
                close_popup();
                return 0;
            }
            match hit_test(lparam_point(l)) {
                Hit::Close => close_popup(),
                Hit::Word => focus_index(0),
                Hit::Code => focus_index(1),
                Hit::Direct => set_mode(true),
                Hit::Cand => set_mode(false),
                Hit::Link => open_dict_manager(),
                Hit::Save => save(),
                Hit::None => {}
            }
            return 0;
        }
        WM_ACTIVATE => {
            if (w & 0xFFFF) != 0 && !SAVED.with(|s| s.get()) {
                let f = with_popup(|p| p.focus).unwrap_or(0);
                if let Some((_, ew, ec)) = handles() {
                    SetFocus(match f {
                        0 => ew,
                        1 => ec,
                        _ => hwnd,
                    });
                }
                return 0;
            }
        }
        WM_APP_LOADED => {
            if l != 0 {
                let d: Box<Dicts> = Box::from_raw(l as *mut Dicts);
                let info = with_popup(|p| {
                    p.dicts = Some(d);
                    (p.timing, p.t0.elapsed(), p.data_dir.clone())
                });
                on_word_changed();
                if let Some((true, el, dir)) = info {
                    log_timing(&dir, &format!("dicts ready at {:.1} ms", el.as_secs_f64() * 1000.0));
                }
            }
            return 0;
        }
        WM_TIMER => {
            if w == TIMER_HOLD {
                KillTimer(hwnd, TIMER_HOLD);
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED as isize);
                SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);
                with_popup(|p| p.alpha = 255);
                SetTimer(hwnd, TIMER_FADE, 15, std::ptr::null());
            } else if w == TIMER_FADE {
                let a = with_popup(|p| {
                    p.alpha -= 28;
                    p.alpha
                })
                .unwrap_or(0);
                if a <= 0 {
                    KillTimer(hwnd, TIMER_FADE);
                    DestroyWindow(hwnd);
                } else {
                    SetLayeredWindowAttributes(hwnd, 0, a as u8, LWA_ALPHA);
                }
            }
            return 0;
        }
        WM_DPICHANGED => {
            let dpi = (w & 0xFFFF) as u32;
            DPI.with(|d| d.set(dpi.max(96)));
            let r = &*(l as *const RECT);
            SetWindowPos(hwnd, 0, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            let fonts = make_fonts(dpi);
            let edit_font = fonts.edit;
            let old = with_popup(|p| std::mem::replace(&mut p.fonts, fonts));
            if let Some(old) = old {
                delete_fonts(&old);
            }
            if let Some((_, ew, ec)) = handles() {
                layout_edits(hwnd, ew, ec, edit_font, dpi);
            }
            InvalidateRect(hwnd, std::ptr::null(), 0);
            return 0;
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, w, l)
}

pub fn run(base_dir: &Path, data_dir: &Path, args: QuickAddArgs) {
    let t0 = Instant::now();
    if activate_existing() {
        return;
    }
    unsafe {
        let theme_idx = args.theme.unwrap_or_else(|| read_theme_index(data_dir)) % THEMES.len();
        let pal = THEMES[theme_idx];
        let c = colors(&pal);
        let hinst = GetModuleHandleW(std::ptr::null());
        let cls = to_wide(CLASS_NAME);
        let icon = LoadIconW(hinst, 1usize as *const u16);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as UINT,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(popup_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: icon,
            hCursor: LoadCursorW(0, IDC_ARROW),
            hbrBackground: 0,
            lpszMenuName: std::ptr::null(),
            lpszClassName: cls.as_ptr(),
            hIconSm: icon,
        };
        RegisterClassExW(&wc);

        let (target, is_caret) = target_point(args.anchor);
        let title = to_wide("鹤羽 · 快速造词");
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            cls.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_CLIPCHILDREN,
            target.x,
            target.y,
            10,
            10,
            0,
            0,
            hinst,
            std::ptr::null(),
        );
        if hwnd == 0 {
            return;
        }
        let dpi = GetDpiForWindow(hwnd).max(96);
        DPI.with(|d| d.set(dpi));
        let (w, h) = (px(dpi, LW), px(dpi, LH));
        let pos = place(target, is_caret, w, h, dpi);
        SetWindowPos(hwnd, HWND_TOPMOST, pos.x, pos.y, w, h, SWP_NOACTIVATE);
        apply_dwm(hwnd, &pal);

        let field_brush = CreateSolidBrush(cref(c.field));
        EDIT_COLORS.with(|e| e.set((cref(c.text), cref(c.field), field_brush)));
        let fonts = make_fonts(dpi);
        let edit_cls = to_wide("EDIT");
        let empty = to_wide("");
        let ew = CreateWindowExW(
            0,
            edit_cls.as_ptr(),
            empty.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL,
            0,
            0,
            10,
            10,
            hwnd,
            ID_WORD as HMENU,
            hinst,
            std::ptr::null(),
        );
        let ec = CreateWindowExW(
            0,
            edit_cls.as_ptr(),
            empty.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL | ES_LOWERCASE,
            0,
            0,
            10,
            10,
            hwnd,
            ID_CODE as HMENU,
            hinst,
            std::ptr::null(),
        );
        SendMessageW(ew, EM_LIMITTEXT, 400, 0);
        SendMessageW(ec, EM_LIMITTEXT, 16, 0);
        layout_edits(hwnd, ew, ec, fonts.edit, dpi);

        let prefill = args.word.clone().unwrap_or_else(|| {
            clipboard_text()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty() && t.chars().count() <= WORD_PREFILL_MAX_CHARS && !t.contains(['\r', '\n', '\t']))
                .unwrap_or_default()
        });
        if !prefill.is_empty() {
            set_text_quiet(ew, &prefill);
        }

        POPUP.with(|cell| {
            *cell.borrow_mut() = Some(Popup {
                hwnd,
                edit_word: ew,
                edit_code: ec,
                pal,
                fonts,
                dicts: None,
                code_overridden: false,
                mode_overridden: false,
                direct: false,
                focus: 0,
                hint: "正在载入词库…".to_string(),
                hint_kind: HintKind::Muted,
                saved: None,
                alpha: 255,
                base_dir: base_dir.to_path_buf(),
                data_dir: data_dir.to_path_buf(),
                t0,
                timing: args.timing,
            });
        });

        // Load dictionaries off the UI thread; the window is usable immediately.
        let dir = data_dir.to_path_buf();
        let target_hwnd = hwnd;
        let _ = std::thread::Builder::new().name("heyu-quickadd-load".into()).spawn(move || {
            let raw = Box::into_raw(Box::new(load_dicts(&dir)));
            if PostMessageW(target_hwnd, WM_APP_LOADED, 0, raw as LPARAM) == 0 {
                drop(Box::from_raw(raw));
            }
        });

        ShowWindow(hwnd, SW_SHOW);
        force_foreground(hwnd);
        SetFocus(ew);
        SendMessageW(ew, EM_SETSEL, 0, -1);
        UpdateWindow(hwnd);
        if args.timing {
            log_timing(data_dir, &format!("window shown at {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0));
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            if msg.message == WM_KEYDOWN && handle_key(msg.wParam as i32) {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if let Some(f) = with_popup(|p| std::mem::replace(&mut p.fonts, make_fonts(96))) {
            delete_fonts(&f);
        }
        DeleteObject(field_brush);
    }
}
