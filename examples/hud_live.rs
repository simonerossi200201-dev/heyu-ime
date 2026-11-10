//! Live smoke test for the layered HUD window (incl. DWM blur-behind glass).
//!
//! `cargo run --release --example hud_live` shows the candidate window for a few
//! themes (2 s each) at screen position (220, 260) and the status pill below it.
//! Pair it with a screen grab to verify real on-screen rendering.

use heyu_core::hud::{set_hud_style_override, update_candidate_hud_at_rect, update_status_pill, FontSet, HudLayout, HudStyle};
use heyu_core::win32::*;
use heyu_core::{Candidate, HeyuEngine, LangMode};

#[allow(non_snake_case)]
#[link(name = "user32")]
extern "system" {
    fn PeekMessageW(lpMsg: *mut MSG, hWnd: HWND, min: u32, max: u32, remove: u32) -> BOOL;
}

unsafe extern "system" fn wndproc(h: HWND, m: UINT, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(h, m, w, l)
}

fn pump(ms: u64) {
    let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < end {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, 0, 0, 0, 1) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
}

fn main() {
    unsafe {
        SetProcessDPIAware();
        let hinst = GetModuleHandleW(std::ptr::null());
        let cls = to_wide("HeyuHudLiveTest");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: 0,
            hCursor: 0,
            hbrBackground: 0,
            lpszMenuName: std::ptr::null(),
            lpszClassName: cls.as_ptr(),
            hIconSm: 0,
        };
        RegisterClassExW(&wc);
        let title = to_wide("hud");
        let hud = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            cls.as_ptr(), title.as_ptr(), WS_POPUP, 300, 300, 500, 100, 0, 0, hinst, std::ptr::null(),
        );
        let pill = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            cls.as_ptr(), title.as_ptr(), WS_POPUP, 300, 300, 248, 44, 0, 0, hinst, std::ptr::null(),
        );
        let fonts = FontSet::new();
        let mut engine = HeyuEngine::new(std::env::temp_dir().join("heyu_hud_live_nonexistent"));
        engine.mode = LangMode::ZhFlypy;
        engine.input_buf = "hy".into();
        engine.candidates = ["鹤羽", "和", "话语", "黄雨", "后院", "会员", "化验"]
            .iter()
            .map(|t| Candidate { text: t.to_string(), comment: String::new(), layer: String::new() })
            .collect();
        engine.config.page_size = 5;
        engine.config.show_status_pill = true;
        let caret = RECT { left: 220, top: 236, right: 221, bottom: 258 };
        let seq: Vec<(usize, HudLayout, bool)> = std::env::args()
            .nth(1)
            .map(|s| s.split(',').filter_map(|x| x.parse().ok()).map(|i| (i, HudLayout::Horizontal, true)).collect())
            .unwrap_or_else(|| vec![(4, HudLayout::Horizontal, true), (8, HudLayout::Vertical, true), (3, HudLayout::Horizontal, true), (0, HudLayout::Horizontal, true)]);
        for (theme, layout, glass) in seq {
            engine.config.theme_index = theme;
            set_hud_style_override(Some(HudStyle { layout, glass }));
            update_candidate_hud_at_rect(hud, &engine, &fonts, caret);
            update_status_pill(pill, &engine, &fonts, POINT { x: 220, y: 520 });
            println!("theme {} glass_active={}", theme, heyu_core::hud::glass_active(hud));
            pump(2000);
        }
        ShowWindow(hud, SW_HIDE);
        ShowWindow(pill, SW_HIDE);
        DestroyWindow(hud);
        DestroyWindow(pill);
    }
}
