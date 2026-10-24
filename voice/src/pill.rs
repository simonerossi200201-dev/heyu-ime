//! Floating "listening" pill: per-pixel-alpha layered window, never activates / steals focus,
//! click-through, DPI aware, colours taken from the active HEYU theme.

use crate::sys::*;
use heyu_core::hud::{ThemePalette, THEMES};
use std::ffi::c_void;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub enum PillState {
    Listening { label: String },
    Recognizing,
    Message { text: String, warn: bool },
}

pub struct Pill {
    hwnd: HWND,
    pub state: Option<PillState>,
    pub level: f32,
    theme: ThemePalette,
    anchor: POINT,
    anchor_is_center: bool,
    work: RECT,
    above: bool,
    scale: f32,
    t0: Instant,
    pub visible: bool,
}

const CLASS: &str = "HEYU_VOICE_PILL";

unsafe extern "system" fn pill_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    const WM_MOUSEACTIVATE: u32 = 0x0021;
    const MA_NOACTIVATE: isize = 3;
    if m == WM_MOUSEACTIVATE {
        return MA_NOACTIVATE;
    }
    DefWindowProcW(h, m, w, l)
}

fn rgb(c: (u8, u8, u8)) -> [f32; 3] {
    [c.0 as f32, c.1 as f32, c.2 as f32]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Signed distance to a rounded rectangle centred at (cx, cy) with half extents (hw, hh) and radius r.
fn sd_round_rect(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

fn sd_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (pax, pay, bax, bay) = (px - ax, py - ay, bx - ax, by - ay);
    let h = ((pax * bax + pay * bay) / (bax * bax + bay * bay).max(1e-6)).clamp(0.0, 1.0);
    let (dx, dy) = (pax - bax * h, pay - bay * h);
    (dx * dx + dy * dy).sqrt()
}

impl Pill {
    pub fn new() -> Pill {
        unsafe {
            let inst = GetModuleHandleW(std::ptr::null());
            let cls = wide(CLASS);
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(pill_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: inst,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: cls.as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                cls.as_ptr(),
                wide("HEYU Voice").as_ptr(),
                WS_POPUP,
                0,
                0,
                10,
                10,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                inst,
                std::ptr::null_mut(),
            );
            let work = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
            Pill { hwnd, state: None, level: 0.0, theme: THEMES[0], anchor: POINT::default(), anchor_is_center: true, work, above: false, scale: 1.0, t0: Instant::now(), visible: false }
        }
    }

    /// Choose the anchor near the text caret of the foreground window (or bottom-centre of its monitor).
    pub fn place_near_caret(&mut self, theme_index: usize) {
        self.theme = THEMES[theme_index % THEMES.len()];
        unsafe {
            let fg = GetForegroundWindow();
            let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
            let mut gti = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
            let mut caret: Option<RECT> = None;
            if tid != 0 && GetGUIThreadInfo(tid, &mut gti) != 0 && gti.hwndCaret != 0 {
                let r = gti.rcCaret;
                let mut tl = POINT { x: r.left, y: r.top };
                let mut br = POINT { x: r.right, y: r.bottom };
                ClientToScreen(gti.hwndCaret as HWND, &mut tl);
                ClientToScreen(gti.hwndCaret as HWND, &mut br);
                if !(tl.x == 0 && tl.y == 0 && br.x == 0 && br.y == 0) {
                    caret = Some(RECT { left: tl.x, top: tl.y, right: br.x.max(tl.x + 1), bottom: br.y.max(tl.y + 1) });
                }
            }
            let mon = match caret {
                Some(c) => MonitorFromPoint(POINT { x: c.left, y: c.bottom }, MONITOR_DEFAULTTONEAREST),
                None => MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST),
            };
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            GetMonitorInfoW(mon, &mut mi);
            let (mut dx, mut dy) = (96u32, 96u32);
            if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) != 0 {
                dx = 96;
            }
            self.scale = dx as f32 / 96.0;
            match caret {
                Some(c) => {
                    self.anchor = POINT { x: c.left + (18.0 * self.scale) as i32, y: c.bottom + (16.0 * self.scale) as i32 };
                    // flip above the caret if there is no room below
                    self.above = c.bottom + (70.0 * self.scale) as i32 > mi.rcWork.bottom;
                    if self.above {
                        self.anchor.y = c.top - (14.0 * self.scale) as i32;
                    }
                }
                None => {
                    let w = mi.rcWork;
                    self.anchor = POINT { x: (w.left + w.right) / 2, y: w.bottom - (48.0 * self.scale) as i32 };
                    self.above = true;
                }
            }
            self.anchor_is_center = caret.is_none();
            self.work = mi.rcWork;
        }
    }

    pub fn show(&mut self, st: PillState) {
        if self.state.as_ref() != Some(&st) {
            self.t0 = Instant::now();
        }
        self.state = Some(st);
        self.render();
        if !self.visible {
            unsafe {
                ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
            self.visible = true;
        }
    }

    pub fn hide(&mut self) {
        self.state = None;
        if self.visible {
            unsafe {
                ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
    }

    pub fn tick(&mut self) {
        if self.visible && self.state.is_some() {
            self.render();
        }
    }

    fn render(&mut self) {
        let Some(state) = self.state.clone() else { return };
        let s = self.scale;
        let th = self.theme;
        let t = self.t0.elapsed().as_secs_f32();
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            let mem = CreateCompatibleDC(screen);
            let font_px = (13.5 * s).round() as i32;
            let face = wide("Microsoft YaHei UI");
            let font = CreateFontW(-font_px, 0, 0, 0, 500, 0, 0, 0, 1, 0, 0, CLEARTYPE_QUALITY, 0, face.as_ptr());
            let old_font = SelectObject(mem, font);

            let (main_text, sub_text): (String, String) = match &state {
                PillState::Listening { .. } => ("聆听中".into(), String::new()),
                PillState::Recognizing => ("识别中…".into(), String::new()),
                PillState::Message { text, .. } => (text.clone(), String::new()),
            };
            let measure = |txt: &str| -> i32 {
                if txt.is_empty() {
                    return 0;
                }
                let w: Vec<u16> = txt.encode_utf16().collect();
                let mut sz = SIZE::default();
                GetTextExtentPoint32W(mem, w.as_ptr(), w.len() as i32, &mut sz);
                sz.cx
            };
            let tw_main = measure(&main_text);
            let tw_sub = measure(&sub_text);
            let pad = (12.0 * s) as i32;
            let icon_w = (14.0 * s) as i32;
            let gap = (7.0 * s) as i32;
            let right_w = match state {
                PillState::Listening { .. } => (30.0 * s) as i32 + gap,
                PillState::Recognizing => (24.0 * s) as i32 + gap,
                PillState::Message { .. } => 0,
            };
            let margin = (10.0 * s) as i32; // shadow room
            let card_h = (32.0 * s).round() as i32;
            let card_w = pad + icon_w + gap + tw_main + tw_sub + right_w + pad;
            let w = card_w + 2 * margin;
            let h = card_h + 2 * margin;

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER { biSize: 40, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() },
                bmiColors: [0],
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(mem, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            let old_bmp = SelectObject(mem, dib);
            let px = std::slice::from_raw_parts_mut(bits as *mut u32, (w * h) as usize);

            // 1) background gradient (opaque, alpha applied later)
            let top = [th.bg_top.0 as f32, th.bg_top.1 as f32, th.bg_top.2 as f32];
            let bot = [th.bg_bottom.0 as f32, th.bg_bottom.1 as f32, th.bg_bottom.2 as f32];
            for y in 0..h {
                let k = ((y - margin) as f32 / card_h as f32).clamp(0.0, 1.0);
                let c = mix(top, bot, k);
                let v = ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | (c[2] as u32);
                for x in 0..w {
                    px[(y * w + x) as usize] = v;
                }
            }
            let accent = rgb(th.accent);
            let blend = |px: &mut [u32], x: i32, y: i32, col: [f32; 3], a: f32| {
                if x < 0 || y < 0 || x >= w || y >= h || a <= 0.0 {
                    return;
                }
                let i = (y * w + x) as usize;
                let o = px[i];
                let ob = [((o >> 16) & 255) as f32, ((o >> 8) & 255) as f32, (o & 255) as f32];
                let n = mix(ob, col, a.min(1.0));
                px[i] = ((n[0] as u32) << 16) | ((n[1] as u32) << 8) | (n[2] as u32);
            };
            let cy = (margin + card_h / 2) as f32;
            let ix = (margin + pad) as f32;

            // 2) icon
            let warn = matches!(state, PillState::Message { warn: true, .. });
            let icon_col = if warn { [235.0, 92.0, 80.0] } else { accent };
            let pulse = match state {
                PillState::Listening { .. } => 0.65 + 0.35 * (t * 4.0).sin().abs(),
                _ => 1.0,
            };
            let icx = ix + icon_w as f32 / 2.0;
            for yy in (cy - 11.0 * s) as i32..=(cy + 11.0 * s) as i32 {
                for xx in ix as i32 - 2..=(ix as i32 + icon_w + 2) {
                    let (fx, fy) = (xx as f32 + 0.5, yy as f32 + 0.5);
                    let d = if matches!(state, PillState::Message { .. }) {
                        // "!" in a ring
                        let r = ((fx - icx).powi(2) + (fy - cy).powi(2)).sqrt();
                        let ring = (r - 7.0 * s).abs() - 0.9 * s;
                        let bar = sd_segment(fx, fy, icx, cy - 3.6 * s, icx, cy + 0.8 * s) - 0.95 * s;
                        let dot = ((fx - icx).powi(2) + (fy - (cy + 3.6 * s)).powi(2)).sqrt() - 1.05 * s;
                        ring.min(bar).min(dot)
                    } else {
                        // microphone: capsule + U-shaped holder + stem + base
                        let caps = sd_round_rect(fx, fy, icx, cy - 3.0 * s, 3.2 * s, 5.6 * s, 3.2 * s);
                        let rr = ((fx - icx).powi(2) + (fy - (cy - 2.0 * s)).powi(2)).sqrt();
                        let arc = if fy >= cy - 2.0 * s { (rr - 6.0 * s).abs() - 0.8 * s } else { 1e9 };
                        let stem = sd_segment(fx, fy, icx, cy + 4.0 * s, icx, cy + 7.2 * s) - 0.8 * s;
                        let base = sd_segment(fx, fy, icx - 3.2 * s, cy + 7.6 * s, icx + 3.2 * s, cy + 7.6 * s) - 0.8 * s;
                        caps.min(arc).min(stem).min(base)
                    };
                    let a = (0.5 - d).clamp(0.0, 1.0) * pulse;
                    blend(px, xx, yy, icon_col, a);
                }
            }

            // 3) right-hand animation: level bars / thinking dots
            let rx = (margin + card_w - pad - right_w + gap) as f32;
            match state {
                PillState::Listening { .. } => {
                    let weights = [0.55, 0.85, 1.0, 0.85, 0.55];
                    let bw = 3.0 * s;
                    let step = 7.0 * s;
                    for (i, wgt) in weights.iter().enumerate() {
                        let wobble = 0.75 + 0.25 * (t * 9.0 + i as f32 * 1.7).sin();
                        let lvl = (self.level * wgt * wobble).clamp(0.0, 1.0);
                        let bh = (3.0 * s + lvl * 18.0 * s) / 2.0;
                        let bx = rx + i as f32 * step + bw / 2.0;
                        for yy in (cy - bh - 2.0) as i32..=(cy + bh + 2.0) as i32 {
                            for xx in (bx - bw) as i32..=(bx + bw) as i32 {
                                let d = sd_round_rect(xx as f32 + 0.5, yy as f32 + 0.5, bx, cy, bw / 2.0, bh, bw / 2.0);
                                blend(px, xx, yy, accent, (0.5 - d).clamp(0.0, 1.0) * (0.55 + 0.45 * lvl.max(0.15)));
                            }
                        }
                    }
                }
                PillState::Recognizing => {
                    for i in 0..3 {
                        let ph = (t * 5.0 - i as f32 * 0.7).sin() * 0.5 + 0.5;
                        let r = (2.0 + 1.2 * ph) * s;
                        let dx = rx + (4.0 + i as f32 * 9.0) * s;
                        for yy in (cy - 6.0 * s) as i32..=(cy + 6.0 * s) as i32 {
                            for xx in (dx - 6.0 * s) as i32..=(dx + 6.0 * s) as i32 {
                                let d = ((xx as f32 + 0.5 - dx).powi(2) + (yy as f32 + 0.5 - cy).powi(2)).sqrt() - r;
                                blend(px, xx, yy, accent, (0.5 - d).clamp(0.0, 1.0) * (0.45 + 0.55 * ph));
                            }
                        }
                    }
                }
                _ => {}
            }

            // 4) text (GDI on the opaque background → correct antialiasing)
            SetBkMode(mem, TRANSPARENT_BK);
            let tx = margin + pad + icon_w + gap;
            let colref = |c: (u8, u8, u8)| (c.0 as u32) | ((c.1 as u32) << 8) | ((c.2 as u32) << 16);
            let draw = |txt: &str, x: i32, col: u32| {
                if txt.is_empty() {
                    return;
                }
                SetTextColor(mem, col);
                let w16: Vec<u16> = txt.encode_utf16().collect();
                let mut rc = RECT { left: x, top: margin, right: x + 4000, bottom: margin + card_h };
                DrawTextW(mem, w16.as_ptr(), w16.len() as i32, &mut rc, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_LEFT);
            };
            draw(&main_text, tx, colref(th.text_primary));
            draw(&sub_text, tx + tw_main, colref(th.text_secondary));

            // 5) per-pixel alpha: anti-aliased rounded card + hairline border + soft shadow; premultiply
            let (ccx, ccy) = ((margin as f32 + card_w as f32 / 2.0), cy);
            let (hw, hh) = (card_w as f32 / 2.0, card_h as f32 / 2.0);
            let radius = hh;
            let card_alpha = (th.bg_top.3.max(235) as f32) / 255.0;
            let shadow_strength = (th.shadow as f32 / 255.0) * 0.55;
            let border = [th.border.0 as f32, th.border.1 as f32, th.border.2 as f32];
            let border_a = th.border.3 as f32 / 255.0 * 0.6;
            let hl_a = th.highlight.3 as f32 / 255.0;
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    let d = sd_round_rect(x as f32 + 0.5, y as f32 + 0.5, ccx, ccy, hw, hh, radius);
                    let cov = (0.5 - d).clamp(0.0, 1.0);
                    let o = px[i];
                    let mut c = [((o >> 16) & 255) as f32, ((o >> 8) & 255) as f32, (o & 255) as f32];
                    // inner hairline highlight + outer border
                    let edge = (1.0 - (d + 1.0).abs()).clamp(0.0, 1.0);
                    if edge > 0.0 {
                        let toward_top = ((ccy - (y as f32)) / hh).clamp(0.0, 1.0);
                        c = mix(c, [255.0, 255.0, 255.0], edge * hl_a * (0.4 + 0.6 * toward_top));
                        c = mix(c, border, edge * border_a * 0.5);
                    }
                    let a_card = cov * card_alpha;
                    // shadow below/around the card (offset 2px down)
                    let ds = sd_round_rect(x as f32 + 0.5, y as f32 + 0.5 - 2.0 * s, ccx, ccy, hw, hh, radius);
                    let sh = if ds > 0.0 { shadow_strength * (1.0 - ds / margin as f32).clamp(0.0, 1.0).powi(2) } else { shadow_strength };
                    let a_sh = sh * (1.0 - a_card);
                    let a = (a_card + a_sh).clamp(0.0, 1.0);
                    let r = (c[0] * a_card) as u32;
                    let g = (c[1] * a_card) as u32;
                    let b = (c[2] * a_card) as u32;
                    px[i] = (((a * 255.0) as u32) << 24) | (r.min(255) << 16) | (g.min(255) << 8) | b.min(255);
                }
            }

            // position
            let mut x = if self.anchor_is_center { self.anchor.x - w / 2 } else { self.anchor.x - margin - (12.0 * s) as i32 };
            let mut y = if self.above { self.anchor.y - h + margin } else { self.anchor.y - margin };
            x = x.clamp(self.work.left - margin, (self.work.right - w + margin).max(self.work.left - margin));
            y = y.clamp(self.work.top - margin, (self.work.bottom - h + margin).max(self.work.top - margin));
            let dst = POINT { x, y };
            let size = SIZE { cx: w, cy: h };
            let src = POINT { x: 0, y: 0 };
            let bf = BLENDFUNCTION { BlendOp: AC_SRC_OVER, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA };
            UpdateLayeredWindow(self.hwnd, screen, &dst, &size, mem, &src, 0, &bf, ULW_ALPHA);

            SelectObject(mem, old_bmp);
            DeleteObject(dib);
            SelectObject(mem, old_font);
            DeleteObject(font);
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);
        }
    }
}

/// Floating Dual-Cursor Voice Caret Marker (`┃🎙`):
/// A tiny, 100% click-through emerald-green caret bar + micro badge shown at the independent
/// Voice Cursor position in any external app (`Luciole`, `Word`, `Chrome`, `Notepad`, etc.).
pub struct VoiceCursorMark {
    hwnd: HWND,
    pub visible: bool,
    pub pos: Option<(i32, i32, i32)>, // (x, y_center, line_h)
}

const CURSOR_MARK_CLASS: &str = "HEYU_VOICE_CURSOR_MARK";

impl VoiceCursorMark {
    pub fn new() -> Self {
        unsafe {
            let inst = GetModuleHandleW(std::ptr::null());
            let cls = wide(CURSOR_MARK_CLASS);
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(pill_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: inst,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: cls.as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                cls.as_ptr(),
                wide("HEYU Voice Cursor").as_ptr(),
                WS_POPUP,
                0,
                0,
                10,
                10,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                inst,
                std::ptr::null_mut(),
            );
            Self {
                hwnd,
                visible: false,
                pos: None,
            }
        }
    }

    pub fn show_at(&mut self, x: i32, y_center: i32, line_h: i32) {
        let lh = line_h.clamp(18, 48);
        if self.visible && self.pos == Some((x, y_center, lh)) {
            return;
        }
        self.pos = Some((x, y_center, lh));
        unsafe {
            let mon = MonitorFromPoint(POINT { x, y: y_center }, MONITOR_DEFAULTTONEAREST);
            let (mut dx, mut dy) = (96u32, 96u32);
            if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) != 0 {
                dx = 96;
            }
            let s = (dx as f32 / 96.0).max(1.0);
            let w = (38.0 * s).round() as i32;
            let h = (lh as f32 + 18.0 * s).round() as i32;

            let screen = GetDC(std::ptr::null_mut());
            let mem = CreateCompatibleDC(screen);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: 40,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                bmiColors: [0],
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(mem, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            let old_bmp = SelectObject(mem, dib);
            let px = std::slice::from_raw_parts_mut(bits as *mut u32, (w * h) as usize);
            for p in px.iter_mut() {
                *p = 0;
            }

            // Draw glowing emerald vertical caret bar at x_bar = 4*s
            let x_bar = (4.0 * s).round();
            let y_top = (12.0 * s).round();
            let y_bot = y_top + lh as f32;
            let emerald = [16.0f32, 185.0, 129.0]; // #10B981

            for yy in 0..h {
                for xx in 0..w {
                    let (fx, fy) = (xx as f32 + 0.5, yy as f32 + 0.5);
                    let d_bar = sd_segment(fx, fy, x_bar, y_top, x_bar, y_bot) - 1.35 * s;
                    // Small top flag capsule `[🎙]` above/right of the caret bar
                    let flag_cx = x_bar + 12.0 * s;
                    let flag_cy = (7.0 * s).round();
                    let d_flag = sd_round_rect(fx, fy, flag_cx, flag_cy, 11.0 * s, 6.0 * s, 5.0 * s);
                    let d = d_bar.min(d_flag);
                    if d < 3.0 * s {
                        let core = (0.5 - d).clamp(0.0, 1.0);
                        let glow = (1.0 - (d.max(0.0) / (3.0 * s))).clamp(0.0, 1.0).powi(2) * 0.35;
                        let a = (core * 0.92 + glow * (1.0 - core)).clamp(0.0, 1.0);
                        let r = (emerald[0] * a) as u32;
                        let g = (emerald[1] * a) as u32;
                        let b = (emerald[2] * a) as u32;
                        px[(yy * w + xx) as usize] = (((a * 255.0) as u32) << 24) | (r << 16) | (g << 8) | b;
                    }
                }
            }

            // Draw tiny white "语" label inside the top flag capsule
            let font_px = (9.5 * s).round() as i32;
            let face = wide("Microsoft YaHei UI");
            let font = CreateFontW(-font_px, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, CLEARTYPE_QUALITY, 0, face.as_ptr());
            let old_font = SelectObject(mem, font);
            SetBkMode(mem, TRANSPARENT_BK);
            SetTextColor(mem, 0x00FF_FFFF);
            let txt = wide("语");
            let mut rc = RECT {
                left: (x_bar + 6.0 * s) as i32,
                top: 0,
                right: w,
                bottom: (14.0 * s) as i32,
            };
            DrawTextW(mem, txt.as_ptr(), (txt.len() - 1) as i32, &mut rc, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_LEFT);
            // Fix GDI cleared alpha on white text pixels inside flag capsule
            for yy in 0..(14.0 * s) as i32 {
                for xx in (x_bar as i32)..w {
                    let idx = (yy * w + xx) as usize;
                    let p = px[idx];
                    let a = (p >> 24) & 0xFF;
                    let r = (p >> 16) & 0xFF;
                    if a == 0 && r > 180 {
                        px[idx] = 0xF5FF_FFFF;
                    }
                }
            }

            let dst = POINT {
                x: x - (x_bar as i32),
                y: y_center - (y_top as i32) - lh / 2,
            };
            let size = SIZE { cx: w, cy: h };
            let src = POINT { x: 0, y: 0 };
            let bf = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA,
            };
            UpdateLayeredWindow(self.hwnd, screen, &dst, &size, mem, &src, 0, &bf, ULW_ALPHA);
            if !self.visible {
                ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                self.visible = true;
            }

            SelectObject(mem, old_bmp);
            DeleteObject(dib);
            SelectObject(mem, old_font);
            DeleteObject(font);
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);
        }
    }

    pub fn hide(&mut self) {
        self.pos = None;
        if self.visible {
            unsafe {
                ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
    }
}
