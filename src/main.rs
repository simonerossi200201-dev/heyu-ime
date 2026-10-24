#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! HEYU (鹤羽输入法) - Native Rust Windows Input Method Server
//! ===========================================================
//! Zero-Network Clean-Room Architecture:
//! - Default: **TSF server mode** - `heyu_tsf.dll` (a real Windows text service loaded into every
//!   app) forwards key events over a local named pipe (`\\.\pipe\heyu-ime-<session>`); this process
//!   owns the dictionaries/engine and draws the candidate HUD at the caret rect the client reports.
//! - Fallback: `--hook` keeps the legacy global `WH_KEYBOARD_LL` hook + `SendInput(KEYEVENTF_UNICODE)`.
//! - 32-Bit Per-Pixel Alpha Layered Candidate HUD (`UpdateLayeredWindow`)
//! - Draggable Floating Status Pill (`鹤羽悬浮状态胶囊`) + System Tray Menu
//! - Real-Time Hot-Reload of `config.json` and `user_custom.tsv`
//!
//! Re-entrancy rule: application state lives in a thread-local `RefCell<Option<AppState>>` and is
//! only touched through [`with_app`], which *moves the state out* of the cell for the duration of the
//! closure (no `RefCell` borrow is ever held). Anything that can pump messages or re-enter the
//! window procedure (menus, ShellExecute, SetCapture, SetWindowPos, Shell_NotifyIcon) is called
//! outside `with_app`, using values copied out beforehand.

use heyu_core::engine::{HeyuEngine, LangMode};
use heyu_core::hud::{
    status_pill_hit_test_with_engine, update_candidate_hud, update_candidate_hud_at_rect,
    update_pill_tooltip, update_status_pill_with_hover, FontSet, PillHit, PILL_W, THEMES,
};
use heyu_core::ipc::{Request, RequestKind, Response, MOD_ALT, MOD_CAPSLOCK, MOD_CTRL, MOD_LSHIFT, MOD_RSHIFT, MOD_SHIFT, MOD_WIN};
use heyu_core::paths;
use heyu_core::pipe;
use heyu_core::session::ImeSession;
use heyu_core::win32::*;
use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

mod quickadd;

const CMD_MODE_ZH: usize = 1001;
const CMD_MODE_IT: usize = 1002;
const CMD_MODE_ES: usize = 1003;
const CMD_MODE_EN: usize = 1004;
const CMD_MODE_DIRECT: usize = 1005;
const CMD_TRAD_TOGGLE: usize = 1020;
const CMD_PILL_SHOW: usize = 1021;
const CMD_PILL_HIDE: usize = 1022;
const CMD_QUICK_ADD: usize = 1030;
const CMD_VOICE: usize = 1031;
const CMD_SETTINGS: usize = 1032;
const CMD_RESTART: usize = 1033;
const CMD_EXIT: usize = 1099;

const WM_APP_REFRESH: u32 = WM_USER + 202;
const WM_APP_IPC: u32 = WM_USER + 203;
/// Posted by the quick-add popup after it wrote `user_custom.tsv`: reload the user lexicon now.
pub(crate) const WM_APP_RELOAD_USER: u32 = WM_USER + 204;
const WM_APP_SHOW_PILL: u32 = WM_USER + 205;

const HOTKEY_ID_QUICK_ADD: i32 = 1;
const VOICE_EXE: &str = "heyu-voice.exe";
const VOICE_MUTEX: &str = "Local\\HEYU_VOICE_SINGLETON";

/// Set by the tray "restart engine" command; `main` relaunches itself after the message loop ends.
static RESTART_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How long a pipe thread waits for the UI thread before answering "unavailable" (the DLL's own
/// timeout is 50 ms, so the client always gets an answer in time and passes the key through).
const IPC_UI_WAIT: Duration = Duration::from_millis(35);

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Pipe,
    Hook,
}

struct AppState {
    base_dir: PathBuf,
    engine: HeyuEngine,
    session: ImeSession,
    run_mode: RunMode,
    hhook: HHOOK,
    fonts: FontSet,
    hwnd_hud: HWND,
    hwnd_pill: HWND,
    hwnd_tip: HWND,
    caret_pos: POINT,
    pill_pos: POINT,
    pill_hover: Option<PillHit>,
    pill_press_hit: Option<PillHit>,
    pill_dragged: bool,
    tracking_pill_mouse: bool,
    dragging_pill: bool,
    dragging_hud: bool,
    drag_offset: POINT,
    drag_start_screen: POINT,
    cfg_mtime: Option<SystemTime>,
    user_dict_mtime: Option<SystemTime>,
    hud_style_mtime: Option<SystemTime>,
    preview_ticks: u8,
    last_drawn_cpm: u32,
    last_pill_sig: u64,
}

thread_local! {
    static APP: RefCell<Option<AppState>> = RefCell::new(None);
}

/// Window that receives WM_APP_IPC (status pill), readable from pipe threads.
static IPC_HWND: AtomicIsize = AtomicIsize::new(0);

/// Run `f` with exclusive access to the app state. The state is moved out of the cell while `f`
/// runs, so a re-entrant call simply gets `None` instead of a `BorrowMutError` panic.
fn with_app<R>(f: impl FnOnce(&mut AppState) -> R) -> Option<R> {
    let mut state = APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut g| g.take()))?;
    let r = f(&mut state);
    APP.with(|cell| {
        if let Ok(mut g) = cell.try_borrow_mut() {
            *g = Some(state);
        }
    });
    Some(r)
}

fn has_sys_dict(dir: &Path) -> bool {
    let d = dir.join("data");
    d.join("heyu_sys.tsv").exists() || d.join("flypy_sys.tsv").exists()
}

fn find_base_dir() -> PathBuf {
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            if has_sys_dict(dir) {
                return dir.to_path_buf();
            }
            if let Some(parent) = dir.parent().and_then(|p| p.parent()) {
                if has_sys_dict(parent) {
                    return parent.to_path_buf();
                }
            }
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn maybe_update_status_pill(state: &mut AppState) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (state.engine.mode as u8).hash(&mut h);
    state.engine.current_cpm().hash(&mut h);
    state.engine.config.input_scheme.hash(&mut h);
    state.engine.config.traditional_chinese.hash(&mut h);
    state.engine.config.chinese_punctuation.hash(&mut h);
    state.engine.config.theme_index.hash(&mut h);
    state.engine.config.show_status_pill.hash(&mut h);
    state.engine.config.hide_candidate_hud.hash(&mut h);
    state.engine.config.immersion_mode.hash(&mut h);
    state.engine.config.quick_add_hotkey.hash(&mut h);
    state.engine.config.user_name.hash(&mut h);
    state.engine.config.avatar_enabled.hash(&mut h);
    state.engine.config.avatar_size.hash(&mut h);
    state.engine.config.pill_scale.to_bits().hash(&mut h);
    state.engine.config.pill_items.hash(&mut h);
    state.engine.last_immersion_tip.hash(&mut h);
    state.pill_pos.x.hash(&mut h);
    state.pill_pos.y.hash(&mut h);
    let hover_disc: u8 = match state.pill_hover {
        None => 0,
        Some(PillHit::ModeBadge) => 1,
        Some(PillHit::DragBody) => 2,
        Some(PillHit::SchemeToggle) => 3,
        Some(PillHit::TradToggle) => 4,
        Some(PillHit::PunctToggle) => 5,
        Some(PillHit::ImmersionToggle) => 6,
        Some(PillHit::BlindToggle) => 7,
        Some(PillHit::VoiceInput) => 8,
        Some(PillHit::StudioRecorder) => 9,
        Some(PillHit::QuickAdd) => 10,
        Some(PillHit::DictPage) => 11,
        Some(PillHit::ThemeCycle) => 12,
        Some(PillHit::Settings) => 13,
    };
    hover_disc.hash(&mut h);
    let sig = h.finish();
    if sig != state.last_pill_sig {
        state.last_pill_sig = sig;
        update_status_pill_with_hover(
            state.hwnd_pill,
            &state.engine,
            &state.fonts,
            state.pill_pos,
            state.pill_hover,
        );
        update_pill_tooltip(
            state.hwnd_tip,
            &state.engine,
            &state.fonts,
            state.pill_pos,
            state.pill_hover,
        );
    }
}

/// Query the live Win32 caret screen rectangle of the foreground window (if it exposes one,
/// e.g. `✏ 快速造词` EDIT control, Notepad, Win32 dialogs).
fn live_win32_caret_rect() -> Option<RECT> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg == 0 {
            return None;
        }
        let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let mut gti = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as DWORD,
            ..Default::default()
        };
        if tid != 0 && GetGUIThreadInfo(tid, &mut gti) != 0 && gti.hwndCaret != 0 {
            let mut tl = POINT { x: gti.rcCaret.left, y: gti.rcCaret.top };
            let mut br = POINT {
                x: gti.rcCaret.right.max(gti.rcCaret.left + 1),
                y: gti.rcCaret.bottom.max(gti.rcCaret.top + 18),
            };
            if ClientToScreen(gti.hwndCaret, &mut tl) != 0 && ClientToScreen(gti.hwndCaret, &mut br) != 0 {
                if tl.x > 0 && br.y > 0 {
                    return Some(RECT {
                        left: tl.x,
                        top: tl.y,
                        right: br.x,
                        bottom: br.y,
                    });
                }
            }
        }
        None
    }
}

/// Check if a screen point lies inside the current foreground window rectangle.
fn rect_inside_foreground(r: RECT) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg == 0 {
            return true;
        }
        let mut wr = RECT::default();
        if GetWindowRect(fg, &mut wr) == 0 {
            return true;
        }
        r.left >= wr.left - 8 && r.left <= wr.right + 8 && r.bottom >= wr.top - 8 && r.bottom <= wr.bottom + 8
    }
}

fn refresh_ui(state: &mut AppState) {
    state.last_drawn_cpm = state.engine.current_cpm();
    if state.engine.is_hud_visible() {
        state.preview_ticks = 0;
    } else if state.preview_ticks > 0 {
        // Render a live 3-second theme/font preview right above the status pill when the user changes theme or font size!
        let theme_name = THEMES[state.engine.config.theme_index % THEMES.len()].name;
        let scale_pct = (state.engine.config.font_scale * 100.0).round() as i32;
        let saved_hide = state.engine.config.hide_candidate_hud;
        state.engine.config.hide_candidate_hud = false;
        state.engine.input_buf = "heyu".to_string();
        state.engine.page = 0;
        state.engine.candidates = vec![
            heyu_core::engine::Candidate {
                text: "鹤羽音形".to_string(),
                comment: format!("{} · {}%", theme_name, scale_pct),
                layer: "user".to_string(),
            },
            heyu_core::engine::Candidate {
                text: "时间".to_string(),
                comment: "EN time · IT tempo · ES tiempo".to_string(),
                layer: "primary".to_string(),
            },
            heyu_core::engine::Candidate {
                text: "自由".to_string(),
                comment: "IT libertà · ES libertad".to_string(),
                layer: "primary".to_string(),
            },
        ];
        let preview_pt = clamp_to_monitor(POINT {
            x: (state.pill_pos.x - 320).max(40),
            y: (state.pill_pos.y - 160).max(40),
        });
        update_candidate_hud(state.hwnd_hud, &state.engine, &state.fonts, preview_pt);
        state.engine.input_buf.clear();
        state.engine.candidates.clear();
        state.engine.config.hide_candidate_hud = saved_hide;
        maybe_update_status_pill(state);
        return;
    }

    if state.run_mode == RunMode::Pipe {
        if let Some(r) = state.session.last_caret {
            let rc = RECT {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            };
            // If `last_caret` is from a previous window before a popup (like ✏ 快速造词) opened,
            // prefer the foreground window's live Win32 caret rect so the candidate bar anchors
            // directly under the popup's input box!
            let effective_rc = if !rect_inside_foreground(rc) {
                live_win32_caret_rect().unwrap_or(rc)
            } else {
                rc
            };
            state.caret_pos = clamp_to_monitor(POINT {
                x: effective_rc.left,
                y: effective_rc.bottom + 8,
            });
            update_candidate_hud_at_rect(state.hwnd_hud, &state.engine, &state.fonts, effective_rc);
            maybe_update_status_pill(state);
            return;
        }
    }
    if state.engine.is_hud_visible() {
        if let Some(live_rc) = live_win32_caret_rect() {
            state.caret_pos = clamp_to_monitor(POINT {
                x: live_rc.left,
                y: live_rc.bottom + 8,
            });
            update_candidate_hud_at_rect(state.hwnd_hud, &state.engine, &state.fonts, live_rc);
            maybe_update_status_pill(state);
            return;
        }
        // On the 1st character of a new composition (or post-commit ghost), pass (0, 0) so
        // `query_caret_screen_position` anchors to the live Win32 caret or current mouse cursor
        // instead of staying stuck at a previous corner!
        let anchor_hint = if state.engine.input_buf.len() <= 1 {
            POINT { x: 0, y: 0 }
        } else {
            state.caret_pos
        };
        state.caret_pos = query_caret_screen_position(anchor_hint);
    }
    update_candidate_hud(state.hwnd_hud, &state.engine, &state.fonts, state.caret_pos);
    maybe_update_status_pill(state);
}

fn post_refresh(hwnd: HWND) {
    unsafe {
        // Use a 1ms coalesced timer (ID = 2) so all pending WM_APP_IPC messages (higher priority
        // than WM_TIMER in Win32 GetMessageW) are answered first with sub-millisecond latency!
        SetTimer(hwnd, 2, 1, std::ptr::null());
    }
}

/// Resolve the directory containing `heyu_studio.py`.
fn resolve_studio_dir(base_dir: &Path) -> PathBuf {
    if base_dir.join("heyu_studio.py").exists() {
        return base_dir.to_path_buf();
    }
    if let Some(dir) = env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        if dir.join("heyu_studio.py").exists() {
            return dir;
        }
    }
    base_dir.to_path_buf()
}

/// Launch the settings studio (`pythonw heyu_studio.py [--page <page>]`).
pub(crate) fn launch_studio(base_dir: &Path, page: Option<&str>) {
    unsafe {
        AllowSetForegroundWindow(ASFW_ANY);
        let sdir = resolve_studio_dir(base_dir);
        let pyw = to_wide("pythonw.exe");
        let script = sdir.join("heyu_studio.py");
        let mut params = format!("\"{}\"", script.display());
        if let Some(p) = page {
            params.push_str(&format!(" --page {}", p));
        }
        let params = to_wide(&params);
        let cwd = to_wide(&sdir.to_string_lossy());
        ShellExecuteW(0, std::ptr::null(), pyw.as_ptr(), params.as_ptr(), cwd.as_ptr(), 1);
    }
}

/// Launch a lightweight standalone mini-window (`--mini-voice` or `--mini-recorder`).
/// Fast-path: if the pre-warmed or hidden mini-window already exists, un-hide and bring it to front in <5 ms!
pub(crate) fn launch_studio_mini(base_dir: &Path, mini_flag: &str) {
    unsafe {
        AllowSetForegroundWindow(ASFW_ANY);
        let title_str = if mini_flag == "--mini-voice" {
            "鹤羽音形 · 🎙 实时声音转写"
        } else if mini_flag == "--mini-recorder" {
            "鹤羽音形 · ◉ 48kHz 录音棚"
        } else {
            ""
        };
        if !title_str.is_empty() {
            let wtitle = to_wide(title_str);
            let existing = FindWindowW(std::ptr::null(), wtitle.as_ptr());
            if existing != 0 {
                ShowWindow(existing, 9); // SW_RESTORE
                ShowWindow(existing, 5); // SW_SHOW
                SetForegroundWindow(existing);
                return;
            }
        }
        let sdir = resolve_studio_dir(base_dir);
        let pyw = to_wide("pythonw.exe");
        let script = sdir.join("heyu_studio.py");
        let params = format!("\"{}\" {}", script.display(), mini_flag);
        let params = to_wide(&params);
        let cwd = to_wide(&sdir.to_string_lossy());
        ShellExecuteW(0, std::ptr::null(), pyw.as_ptr(), params.as_ptr(), cwd.as_ptr(), 1);
    }
}

/// Immediately toggle Microphone voice input (`WM_VOICE_TOGGLE_MIC = 0x800A`) when the user types `oio`.
fn toggle_mic_voice_immediately(data_dir: &Path) {
    unsafe {
        let cls = to_wide("HEYU_VOICE_MSG");
        let h_voice = FindWindowW(cls.as_ptr(), std::ptr::null());
        if h_voice != 0 {
            let _ = PostMessageW(h_voice, 0x8000 + 10, 0, 0);
            return;
        }
    }
    let Some(dir) = env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else { return };
    let exe = dir.join(VOICE_EXE);
    if !exe.exists() {
        return;
    }
    if !named_mutex_exists(VOICE_MUTEX) {
        if !spawn_detached(&exe, &[]) {
            log_line(data_dir, "[voice] failed to spawn heyu-voice.exe for oio macro");
            return;
        }
    }
    std::thread::spawn(move || {
        let cls = to_wide("HEYU_VOICE_MSG");
        for _ in 0..35 {
            std::thread::sleep(Duration::from_millis(45));
            let h_voice = unsafe { FindWindowW(cls.as_ptr(), std::ptr::null()) };
            if h_voice != 0 {
                unsafe {
                    let _ = PostMessageW(h_voice, 0x8000 + 10, 0, 0);
                }
                break;
            }
        }
    });
}

/// Append a line to `data/crash.log` (the server's only log file).
fn log_line(data_dir: &Path, msg: &str) {
    let line = format!("[{:?}] {}\n", SystemTime::now(), msg);
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data_dir.join("crash.log"))
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

/// Start `exe args...` fully detached (no console, not tied to our lifetime).
fn spawn_detached(exe: &Path, args: &[String]) -> bool {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    cmd.spawn().is_ok()
}

/// Minimal `"key": true|false` lookup for small JSON config files.
fn json_bool(txt: &str, key: &str) -> Option<bool> {
    let pat = format!("\"{}\"", key);
    let rest = &txt[txt.find(&pat)? + pat.len()..];
    let v = rest[rest.find(':')? + 1..].trim_start();
    if v.starts_with("true") {
        Some(true)
    } else if v.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Screen point just below the text caret of the foreground app, if it can be determined:
/// the Win32 caret first, else the TSF-reported caret if it lies inside the foreground window.
fn caret_anchor(last_caret: Option<RECT>) -> Option<POINT> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg == 0 {
            return None;
        }
        let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let mut gti = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as DWORD, ..Default::default() };
        if tid != 0 && GetGUIThreadInfo(tid, &mut gti) != 0 && gti.hwndCaret != 0 {
            let mut pt = POINT { x: gti.rcCaret.left, y: gti.rcCaret.bottom };
            if ClientToScreen(gti.hwndCaret, &mut pt) != 0 && (pt.x != 0 || pt.y != 0) {
                return Some(pt);
            }
        }
        if let Some(r) = last_caret {
            let mut wr = RECT::default();
            if (r.left | r.top | r.right | r.bottom) != 0
                && GetWindowRect(fg, &mut wr) != 0
                && r.left >= wr.left
                && r.left < wr.right
                && r.bottom >= wr.top
                && r.bottom <= wr.bottom
            {
                return Some(POINT { x: r.left, y: r.bottom });
            }
        }
        None
    }
}

/// Open the ✏ quick-add popup (separate process, so typing Chinese into it goes through the
/// normal TSF -> pipe path instead of re-entering this UI thread).
fn open_quick_add() {
    unsafe {
        // We just received WM_HOTKEY / a menu command, so we may hand foreground rights on.
        AllowSetForegroundWindow(ASFW_ANY);
    }
    if quickadd::activate_existing() {
        return;
    }
    let snap = with_app(|s| {
        let caret = s.session.last_caret.map(|r| RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom });
        (caret, s.engine.data_dir.clone())
    });
    let Some((last_caret, data_dir)) = snap else { return };
    let anchor = caret_anchor(last_caret);
    let Ok(exe) = env::current_exe() else { return };
    let mut args = vec!["--quick-add".to_string(), "--data-dir".to_string(), data_dir.to_string_lossy().to_string()];
    if let Some(a) = anchor {
        args.push("--anchor".to_string());
        args.push(format!("{},{}", a.x, a.y));
    }
    if !spawn_detached(&exe, &args) {
        log_line(&data_dir, "[quick-add] failed to start the popup process");
    }
}

unsafe fn show_balloon(hwnd: HWND, title: &str, text: &str, warn: bool) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as DWORD;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = if warn { NIIF_WARNING } else { NIIF_INFO };
    for (i, ch) in text.encode_utf16().take(255).enumerate() {
        nid.szInfo[i] = ch;
    }
    for (i, ch) in title.encode_utf16().take(63).enumerate() {
        nid.szInfoTitle[i] = ch;
    }
    Shell_NotifyIconW(NIM_MODIFY, &nid);
}

/// (Re-)register the quick-add hotkey. Empty spec = disabled. On failure: log + tray balloon.
fn register_quick_add_hotkey(hwnd: HWND, spec: &str, data_dir: &Path, notify: bool) -> bool {
    unsafe {
        UnregisterHotKey(hwnd, HOTKEY_ID_QUICK_ADD);
    }
    let spec = spec.trim();
    if spec.is_empty() {
        return true;
    }
    let Some((mods, vk)) = parse_hotkey(spec) else {
        log_line(data_dir, &format!("[quick-add] invalid quick_add_hotkey \"{}\"", spec));
        if notify {
            unsafe { show_balloon(hwnd, "快速造词快捷键无效", &format!("无法识别「{}」，示例：Ctrl+Alt+N。仍可从托盘菜单打开快速造词。", spec), true) };
        }
        return false;
    };
    let ok = unsafe { RegisterHotKey(hwnd, HOTKEY_ID_QUICK_ADD, mods | HK_MOD_NOREPEAT, vk) } != 0;
    if !ok {
        let err = unsafe { GetLastError() };
        log_line(data_dir, &format!("[quick-add] RegisterHotKey({}) failed, error {}", spec, err));
        if notify {
            unsafe {
                show_balloon(
                    hwnd,
                    "快速造词快捷键被占用",
                    &format!("{} 已被其他程序占用。可在 config.json 的 quick_add_hotkey 中更换，或从托盘菜单打开快速造词。", spec),
                    true,
                )
            };
        }
    }
    ok
}

/// Start `heyu-voice.exe` (next to this exe) if voice input is enabled and it is not running yet.
fn maybe_spawn_voice(data_dir: &Path) {
    let Some(dir) = env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else { return };
    let exe = dir.join(VOICE_EXE);
    if !exe.exists() {
        return;
    }
    let cfg = fs::read_to_string(data_dir.join("voice_config.json")).unwrap_or_default();
    if !json_bool(&cfg, "enabled").unwrap_or(false) {
        return;
    }
    if named_mutex_exists(VOICE_MUTEX) {
        return;
    }
    if !spawn_detached(&exe, &[]) {
        log_line(data_dir, "[voice] failed to start heyu-voice.exe");
    }
}

fn mode_title(m: LangMode) -> &'static str {
    match m {
        LangMode::ZhFlypy => "中文",
        LangMode::It => "Italiano",
        LangMode::Es => "Español",
        LangMode::En => "English",
        LangMode::Direct => "直通英文",
    }
}

/// Open the sleek standalone Voice & System Audio Live Transcription + Dual-Cursor Mini-Window (`--mini-voice`),
/// while ensuring `heyu-voice.exe` is running in the background.
fn trigger_voice_dictation_or_studio(base_dir: &Path, data_dir: &Path) {
    unsafe {
        let cls = to_wide("HEYU_VOICE_MSG");
        let h_voice = FindWindowW(cls.as_ptr(), std::ptr::null());
        if h_voice == 0 {
            if let Some(dir) = env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
                let exe = dir.join(VOICE_EXE);
                if exe.exists() && !named_mutex_exists(VOICE_MUTEX) {
                    if !spawn_detached(&exe, &[]) {
                        log_line(data_dir, "[voice] failed to spawn heyu-voice.exe from pill");
                    }
                }
            }
        }
    }
    launch_studio_mini(base_dir, "--mini-voice");
}

/// Values the context menu needs, copied out of the state so no borrow is held while
/// `TrackPopupMenu` runs its modal loop (which dispatches WM_APP_IPC / WM_TIMER).
struct MenuSnapshot {
    mode: LangMode,
    traditional_chinese: bool,
    show_status_pill: bool,
    quick_add_hotkey: String,
}

fn show_context_menu(hwnd: HWND, snap: &MenuSnapshot) -> usize {
    unsafe {
        let hmenu = CreatePopupMenu();
        if hmenu == 0 {
            return 0;
        }
        let hmodes = CreatePopupMenu();
        let add = |menu: HMENU, id: usize, label: &str, flags: u32| {
            let w = to_wide(label);
            AppendMenuW(menu, MF_STRING | flags, id, w.as_ptr());
        };
        let check = |on: bool| if on { MF_CHECKED } else { 0 };

        add(hmenu, 0, &format!("鹤羽音形 · {}", mode_title(snap.mode)), MF_GRAYED);
        AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());

        add(hmodes, CMD_MODE_ZH, "中文 · 鹤羽音形", check(snap.mode == LangMode::ZhFlypy));
        add(hmodes, CMD_MODE_IT, "Italiano · 意大利语", check(snap.mode == LangMode::It));
        add(hmodes, CMD_MODE_ES, "Español · 西班牙语", check(snap.mode == LangMode::Es));
        add(hmodes, CMD_MODE_EN, "English · 英语", check(snap.mode == LangMode::En));
        add(hmodes, CMD_MODE_DIRECT, "直通英文\t左 Shift", check(snap.mode == LangMode::Direct));
        let modes_label = to_wide("输入模式");
        AppendMenuW(hmenu, MF_POPUP, hmodes as usize, modes_label.as_ptr());

        let trad_txt = if snap.traditional_chinese {
            "繁/简  当前：繁体中文输出 (点击切换简体)"
        } else {
            "简/繁  当前：简体中文输出 (点击切换繁体)"
        };
        add(hmenu, CMD_TRAD_TOGGLE, trad_txt, check(snap.traditional_chinese));

        let hk = snap.quick_add_hotkey.trim();
        add(hmenu, CMD_QUICK_ADD, &if hk.is_empty() { "✏  快速造词…".to_string() } else { format!("✏  快速造词…\t{}", hk) }, 0);
        add(hmenu, CMD_VOICE, "🎙  语音输入与双光标工作台…\tCtrl+Alt+V", 0);
        AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
        add(hmenu, CMD_PILL_SHOW, "👁  显示胶囊面板", check(snap.show_status_pill));
        add(hmenu, CMD_PILL_HIDE, "🙈  隐藏胶囊面板", check(!snap.show_status_pill));
        AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
        add(hmenu, CMD_SETTINGS, "⚙  设置中心…", 0);
        add(hmenu, CMD_RESTART, "↻  重启引擎", 0);
        add(hmenu, CMD_EXIT, "退出", 0);

        let mut pt = POINT::default();
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            hmenu,
            TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_NONOTIFY | TPM_RETURNCMD,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        ) as usize;
        DestroyMenu(hmenu); // also destroys the attached submenu
        cmd
    }
}

fn open_menu(hwnd: HWND) {
    let snap = with_app(|s| MenuSnapshot {
        mode: s.engine.mode,
        traditional_chinese: s.engine.config.traditional_chinese,
        show_status_pill: s.engine.config.show_status_pill,
        quick_add_hotkey: s.engine.config.quick_add_hotkey.clone(),
    });
    let Some(snap) = snap else { return };
    let cmd = show_context_menu(hwnd, &snap); // no state held here
    if cmd != 0 {
        handle_menu_command(cmd);
    }
}

fn install_panic_logger(log_dir: &Path) {
    let log = log_dir.join("crash.log");
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!("[{:?}] PANIC: {}\n", SystemTime::now(), info);
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .and_then(|mut f| std::io::Write::write_all(&mut f, msg.as_bytes()));
    }));
}

fn handle_menu_command(cmd: usize) {
    enum After {
        Nothing,
        Studio(PathBuf, &'static str),
        QuickAdd,
        Restart(HWND),
        Exit(HWND),
    }
    let after = with_app(|state| {
        let set_mode = |state: &mut AppState, m: LangMode| {
            state.engine.clear_composition();
            state.engine.mode = m;
            state.engine.save_config();
            refresh_ui(state);
        };
        match cmd {
            CMD_MODE_ZH => set_mode(state, LangMode::ZhFlypy),
            CMD_MODE_IT => set_mode(state, LangMode::It),
            CMD_MODE_ES => set_mode(state, LangMode::Es),
            CMD_MODE_EN => set_mode(state, LangMode::En),
            CMD_MODE_DIRECT => set_mode(state, LangMode::Direct),
            CMD_TRAD_TOGGLE => {
                state.engine.config.traditional_chinese = !state.engine.config.traditional_chinese;
                state.engine.save_config();
                refresh_ui(state);
            }
            CMD_PILL_SHOW => {
                state.engine.config.show_status_pill = true;
                state.engine.save_config();
                refresh_ui(state);
            }
            CMD_PILL_HIDE => {
                state.engine.config.show_status_pill = false;
                state.pill_hover = None;
                state.engine.save_config();
                refresh_ui(state);
            }
            CMD_QUICK_ADD => return After::QuickAdd,
            CMD_VOICE => return After::Studio(state.base_dir.clone(), "voice"),
            CMD_SETTINGS => return After::Studio(state.base_dir.clone(), "general"),
            CMD_RESTART => return After::Restart(state.hwnd_pill),
            CMD_EXIT => return After::Exit(state.hwnd_pill),
            _ => {}
        }
        After::Nothing
    });
    match after {
        Some(After::Studio(dir, page)) => launch_studio(&dir, Some(page)),
        Some(After::QuickAdd) => open_quick_add(),
        Some(After::Restart(hwnd_pill)) => unsafe {
            RESTART_REQUESTED.store(true, Ordering::Release);
            remove_tray_icon(hwnd_pill);
            PostQuitMessage(0);
        },
        Some(After::Exit(hwnd_pill)) => unsafe {
            remove_tray_icon(hwnd_pill);
            PostQuitMessage(0);
        },
        _ => {}
    }
}

// ============================================================================
// PIPE SERVER BRIDGE (pipe threads -> UI thread)
// ============================================================================
struct JobState {
    taken: bool,
    cancelled: bool,
    resp: Option<Vec<u8>>,
}

struct IpcJob {
    req: Request,
    state: Mutex<JobState>,
    cv: Condvar,
}

/// Runs on a pipe client thread. Hands the request to the UI thread (which owns the engine and
/// the HUD) and waits a bounded time for the answer.
fn pipe_handler(bytes: &[u8], client_pid: u32) -> Vec<u8> {
    let mut req = match Request::decode(bytes) {
        Ok(r) => r,
        Err(_) => return Response::unavailable().encode(),
    };
    if req.pid == 0 {
        req.pid = client_pid;
    }
    let hwnd = IPC_HWND.load(Ordering::Acquire);
    if hwnd == 0 {
        return Response::unavailable().encode();
    }
    let job = Arc::new(IpcJob {
        req,
        state: Mutex::new(JobState { taken: false, cancelled: false, resp: None }),
        cv: Condvar::new(),
    });
    let raw = Arc::into_raw(job.clone());
    if unsafe { PostMessageW(hwnd, WM_APP_IPC, 0, raw as LPARAM) } == 0 {
        unsafe { drop(Arc::from_raw(raw)) };
        return Response::unavailable().encode();
    }
    let deadline = Instant::now() + IPC_UI_WAIT;
    let hard_deadline = deadline + Duration::from_millis(250);
    let mut st = match job.state.lock() {
        Ok(g) => g,
        Err(_) => return Response::unavailable().encode(),
    };
    loop {
        if let Some(r) = st.resp.take() {
            return r;
        }
        let now = Instant::now();
        if now >= deadline {
            if !st.taken {
                // UI thread busy: give up; it will skip this job when it gets to it.
                st.cancelled = true;
                return Response::unavailable().encode();
            }
            if now >= hard_deadline {
                return Response::unavailable().encode();
            }
        }
        let wait = if now < deadline { deadline - now } else { Duration::from_millis(5) };
        st = match job.cv.wait_timeout(st, wait) {
            Ok((g, _)) => g,
            Err(_) => return Response::unavailable().encode(),
        };
    }
}

/// UI thread: process one queued IPC job.
fn process_ipc_job(lparam: LPARAM) {
    if lparam == 0 {
        return;
    }
    let job: Arc<IpcJob> = unsafe { Arc::from_raw(lparam as *const IpcJob) };
    {
        let Ok(mut st) = job.state.lock() else { return };
        if st.cancelled {
            return;
        }
        st.taken = true;
    }
    let outcome = with_app(|s| {
        // Record the TSF client's accurate caret rectangle.
        // CRITICAL: Only forward `WM_APP_TSF_CARET` (0x8000 + 20) to `heyu-voice.exe` when
        // `job.req.kind == RequestKind::Caret` (freshly measured via `ITfContextView::GetTextExt`),
        // NEVER on `RequestKind::Key` / `FocusIn` / `Ping` (which carry a cached `st.last_caret`
        // that would overwrite the user's or voice cursor when pressing keys or Shift)!
        if let Some(cr) = job.req.caret {
            if cr.left != 0 || cr.top != 0 || cr.right != 0 || cr.bottom != 0 {
                s.session.last_caret = Some(cr);
                s.caret_pos = POINT { x: cr.left, y: cr.bottom };
                if job.req.kind == RequestKind::Caret {
                    unsafe {
                        let voice_cls = to_wide("HEYU_VOICE_MSG");
                        let vhwnd = FindWindowW(voice_cls.as_ptr(), std::ptr::null());
                        if vhwnd != 0 {
                            let cx = cr.right.max(cr.left);
                            let cy = (cr.top + cr.bottom) / 2;
                            let lh = (cr.bottom - cr.top).clamp(14, 64);
                            let wp = ((cx as i32 as u32 as usize) << 32) | (cy as i32 as u32 as usize);
                            let _ = PostMessageW(vhwnd, 0x8000 + 20, wp, lh as isize);
                        }
                    }
                }
            }
        }
        // Check if this is an atomic TSF Voice Commit request (`vk == 0x88` key-down from `heyu-voice.exe`)
        if job.req.kind == RequestKind::Key && job.req.vk == 0x88 && !job.req.key_up {
            let commit_path = env::temp_dir().join("heyu_voice_commit.utf8");
            if let Ok(vtext) = fs::read_to_string(&commit_path) {
                let _ = fs::remove_file(&commit_path);
                if !vtext.is_empty() {
                    s.session.pending_voice_commit = Some(vtext);
                    return Some(s.session.handle(&mut s.engine, &job.req));
                }
            }
        }
        // When the global low-level hook is active in desktop mode, it already handles all keys
        // directly; return None (passthrough) on TSF pipe requests so keys are never double-processed
        // and the hook is never lost when switching between windows.
        if s.hhook != 0 && matches!(job.req.kind, RequestKind::Key | RequestKind::FocusIn) {
            return None;
        }
        s.run_mode = RunMode::Pipe;
        let mut out = s.session.handle(&mut s.engine, &job.req);
        if s.engine.pending_mic_toggle {
            s.engine.pending_mic_toggle = false;
            toggle_mic_voice_immediately(&s.engine.data_dir);
        }
        if s.engine.pending_undo_backspaces > 0 {
            let n = std::mem::take(&mut s.engine.pending_undo_backspaces);
            out.response.commit_text.clear();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(18));
                send_unicode_text(&"\u{8}".repeat(n));
            });
        }
        Some(out)
    })
    .flatten();
    let (bytes, refresh) = match &outcome {
        Some(o) => (o.response.encode(), o.refresh_ui),
        None => (Response::unavailable().encode(), false),
    };
    if let Ok(mut st) = job.state.lock() {
        st.resp = Some(bytes);
        job.cv.notify_all();
    }
    // Answer first, then schedule UI refresh via WM_APP_REFRESH so queued WM_APP_IPC jobs (like key-up)
    // are answered immediately without waiting on GDI rasterization.
    if refresh {
        let hwnd = IPC_HWND.load(Ordering::Acquire);
        if hwnd != 0 {
            post_refresh(hwnd);
        }
    }
}

// ============================================================================
// LOW-LEVEL KEYBOARD HOOK (`WH_KEYBOARD_LL`) - fallback before TSF registration or with `--hook`
// ============================================================================
fn key_down(vk: i32) -> bool {
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

unsafe extern "system" fn low_level_keyboard_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code == HC_ACTION && l_param != 0 {
        let kb = &*(l_param as *const KBDLLHOOKSTRUCT);

        // Ignore synthetic Unicode events injected by our own `send_unicode_text` or `heyu-voice.exe` (`VK_PACKET = 0xE7`)!
        if kb.dwExtraInfo == HEYU_MAGIC_EXTRA || kb.vkCode == 0xE7 {
            return CallNextHookEx(0, n_code, w_param, l_param);
        }

        let msg = w_param as u32;
        let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        let vk = kb.vkCode as i32;

        // Key-ups only matter for the Shift tap toggle (which also commits active preedit English).
        if is_down || (is_up && (vk == VK_LSHIFT || vk == VK_RSHIFT || vk == VK_SHIFT)) {
            let mut mods = 0u16;
            if key_down(VK_SHIFT) || key_down(VK_LSHIFT) || key_down(VK_RSHIFT) {
                mods |= MOD_SHIFT;
            }
            if key_down(VK_LSHIFT) {
                mods |= MOD_LSHIFT;
            }
            if key_down(VK_RSHIFT) {
                mods |= MOD_RSHIFT;
            }
            if key_down(VK_CONTROL) || key_down(VK_LCONTROL) || key_down(VK_RCONTROL) {
                mods |= MOD_CTRL;
            }
            if key_down(VK_MENU) || key_down(VK_LMENU) || key_down(VK_RMENU) {
                mods |= MOD_ALT;
            }
            if key_down(VK_LWIN) || key_down(VK_RWIN) {
                mods |= MOD_WIN;
            }
            if (GetKeyState(VK_CAPITAL) & 0x0001) != 0 {
                mods |= MOD_CAPSLOCK;
            }
            let mut req = Request::key(vk as u16, '\0', mods, is_up);
            req.ch = 0; // server-side US-layout mapping
            let out = with_app(|s| {
                let o = s.session.handle(&mut s.engine, &req);
                s.engine.pending_undo_backspaces = 0;
                let mic_tog = std::mem::replace(&mut s.engine.pending_mic_toggle, false);
                (o, s.hwnd_pill, mic_tog, s.engine.data_dir.clone())
            });
            if let Some((o, hwnd_pill, mic_tog, data_dir)) = out {
                if mic_tog {
                    toggle_mic_voice_immediately(&data_dir);
                }
                if o.refresh_ui {
                    post_refresh(hwnd_pill);
                }
                if !o.response.commit_text.is_empty() {
                    send_unicode_text(&o.response.commit_text);
                }
                if o.response.eaten && is_down {
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(0, n_code, w_param, l_param)
}

// ============================================================================
// WINDOW PROCEDURE FOR STATUS PILL & HUD
// ============================================================================
unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: UINT, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return MA_NOACTIVATE,
        WM_APP_IPC => {
            process_ipc_job(l_param);
            return 0;
        }
        WM_LBUTTONDOWN => {
            let mut pt = POINT::default();
            GetCursorPos(&mut pt);
            let mut wr = RECT::default();
            GetWindowRect(hwnd, &mut wr);
            let started = with_app(|state| {
                if hwnd == state.hwnd_pill {
                    state.dragging_pill = true;
                    state.pill_dragged = false;
                    state.drag_start_screen = pt;
                    state.drag_offset = POINT { x: pt.x - state.pill_pos.x, y: pt.y - state.pill_pos.y };
                    let hit = status_pill_hit_test_with_engine(state.pill_pos, pt.x, &state.engine);
                    state.pill_press_hit = Some(hit);
                    if state.pill_hover != Some(hit) {
                        state.pill_hover = Some(hit);
                        maybe_update_status_pill(state);
                    }
                    true
                } else if hwnd == state.hwnd_hud {
                    state.dragging_hud = true;
                    state.drag_start_screen = pt;
                    state.drag_offset = POINT { x: pt.x - wr.left, y: pt.y - wr.top };
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
            if started {
                SetCapture(hwnd);
                return 0;
            }
        }
        WM_MOUSEMOVE => {
            let mut pt = POINT::default();
            GetCursorPos(&mut pt);
            let mut need_track_leave = false;
            let new_pos = with_app(|state| {
                let dx = (pt.x - state.drag_start_screen.x).abs();
                let dy = (pt.y - state.drag_start_screen.y).abs();
                if state.dragging_pill && hwnd == state.hwnd_pill {
                    if !state.pill_dragged {
                        let thresh = match state.pill_press_hit {
                            Some(PillHit::DragBody) | None => 4,
                            Some(_) => 9,
                        };
                        if dx > thresh || dy > thresh {
                            state.pill_dragged = true;
                        }
                    }
                    if state.pill_dragged {
                        let p = POINT { x: pt.x - state.drag_offset.x, y: pt.y - state.drag_offset.y };
                        state.pill_pos = p;
                        state.pill_hover = None;
                        maybe_update_status_pill(state);
                        Some(p)
                    } else {
                        None
                    }
                } else if state.dragging_hud && hwnd == state.hwnd_hud && (dx > 4 || dy > 4) {
                    let p = POINT { x: pt.x - state.drag_offset.x, y: pt.y - state.drag_offset.y };
                    Some(p)
                } else if hwnd == state.hwnd_pill && !state.dragging_pill {
                    if !state.tracking_pill_mouse {
                        state.tracking_pill_mouse = true;
                        need_track_leave = true;
                    }
                    let hit = status_pill_hit_test_with_engine(state.pill_pos, pt.x, &state.engine);
                    if state.pill_hover != Some(hit) {
                        state.pill_hover = Some(hit);
                        maybe_update_status_pill(state);
                    }
                    None
                } else {
                    None
                }
            })
            .flatten();
            if need_track_leave {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as DWORD,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut tme);
            }
            if let Some(p) = new_pos {
                SetWindowPos(hwnd, HWND_TOPMOST, p.x, p.y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
                return 0;
            }
        }
        WM_MOUSELEAVE => {
            with_app(|state| {
                if hwnd == state.hwnd_pill {
                    state.tracking_pill_mouse = false;
                    if state.pill_hover.is_some() {
                        state.pill_hover = None;
                        maybe_update_status_pill(state);
                    }
                }
            });
            return 0;
        }
        WM_LBUTTONUP => {
            let mut pt = POINT::default();
            GetCursorPos(&mut pt);
            enum PillAction {
                None,
                DragFinished,
                CycleMode,
                CycleScheme,
                ToggleTrad,
                TogglePunct,
                CycleImmersion,
                ToggleBlind,
                VoiceDictation(PathBuf, PathBuf),
                StudioVoice(PathBuf),
                QuickAdd,
                OpenDict(PathBuf),
                CycleTheme,
                OpenSettings(PathBuf),
            }
            let action = with_app(|state| {
                if state.dragging_hud && hwnd == state.hwnd_hud {
                    state.dragging_hud = false;
                    let moved = (pt.x - state.drag_start_screen.x).abs() > 5 || (pt.y - state.drag_start_screen.y).abs() > 5;
                    if moved {
                        let p = POINT { x: pt.x - state.drag_offset.x, y: pt.y - state.drag_offset.y };
                        state.engine.config.hud_x = p.x.max(20);
                        state.engine.config.hud_y = p.y.max(20);
                        state.engine.save_config();
                    }
                    return PillAction::DragFinished;
                }
                if !(state.dragging_pill && hwnd == state.hwnd_pill) {
                    return PillAction::None;
                }
                state.dragging_pill = false;
                let was_dragged = std::mem::replace(&mut state.pill_dragged, false);
                let press_hit = state.pill_press_hit.take();
                if was_dragged {
                    state.engine.config.pill_x = state.pill_pos.x;
                    state.engine.config.pill_y = state.pill_pos.y;
                    state.engine.save_config();
                    return PillAction::DragFinished;
                }
                let up_hit = status_pill_hit_test_with_engine(state.pill_pos, pt.x, &state.engine);
                let target_hit = if up_hit != PillHit::DragBody {
                    up_hit
                } else {
                    press_hit.unwrap_or(PillHit::DragBody)
                };
                match target_hit {
                    PillHit::ModeBadge => PillAction::CycleMode,
                    PillHit::DragBody => PillAction::DragFinished,
                    PillHit::SchemeToggle => PillAction::CycleScheme,
                    PillHit::TradToggle => PillAction::ToggleTrad,
                    PillHit::PunctToggle => PillAction::TogglePunct,
                    PillHit::ImmersionToggle => PillAction::CycleImmersion,
                    PillHit::BlindToggle => PillAction::ToggleBlind,
                    PillHit::VoiceInput => PillAction::VoiceDictation(
                        state.base_dir.clone(),
                        state.engine.data_dir.clone(),
                    ),
                    PillHit::StudioRecorder => PillAction::StudioVoice(state.base_dir.clone()),
                    PillHit::QuickAdd => PillAction::QuickAdd,
                    PillHit::DictPage => PillAction::OpenDict(state.base_dir.clone()),
                    PillHit::ThemeCycle => PillAction::CycleTheme,
                    PillHit::Settings => PillAction::OpenSettings(state.base_dir.clone()),
                }
            })
            .unwrap_or(PillAction::None);
            if !matches!(action, PillAction::None) {
                ReleaseCapture();
            }
            match action {
                PillAction::CycleMode => {
                    with_app(|state| {
                        state.engine.cycle_language_mode();
                        refresh_ui(state);
                    });
                }
                PillAction::CycleScheme => {
                    with_app(|state| {
                        state.engine.clear_composition();
                        let next_sc = match state.engine.config.input_scheme.as_str() {
                            "quanpin" => "shuangpin",
                            "shuangpin" => "yinxing",
                            _ => "quanpin",
                        };
                        if next_sc == "quanpin" && state.engine.config.page_size == 5 {
                            state.engine.config.page_size = 10;
                        }
                        state.engine.config.input_scheme = next_sc.to_string();
                        state.engine.save_config();
                        refresh_ui(state);
                    });
                }
                PillAction::ToggleTrad => {
                    with_app(|state| {
                        state.engine.config.traditional_chinese = !state.engine.config.traditional_chinese;
                        state.engine.save_config();
                        refresh_ui(state);
                    });
                }
                PillAction::TogglePunct => {
                    with_app(|state| {
                        state.engine.config.chinese_punctuation = !state.engine.config.chinese_punctuation;
                        state.engine.save_config();
                        refresh_ui(state);
                    });
                }
                PillAction::CycleImmersion => {
                    with_app(|state| {
                        let next_imm = match state.engine.config.immersion_mode.to_ascii_uppercase().as_str() {
                            "OFF" => "ALL",
                            "ALL" => "EN",
                            "EN" => "IT",
                            "IT" => "ES",
                            _ => "OFF",
                        };
                        state.engine.config.immersion_mode = next_imm.to_string();
                        state.engine.save_config();
                        refresh_ui(state);
                    });
                }
                PillAction::ToggleBlind => {
                    with_app(|state| {
                        state.engine.config.hide_candidate_hud = !state.engine.config.hide_candidate_hud;
                        state.engine.save_config();
                        refresh_ui(state);
                    });
                }
                PillAction::VoiceDictation(base_dir, data_dir) => {
                    trigger_voice_dictation_or_studio(&base_dir, &data_dir);
                }
                PillAction::StudioVoice(base_dir) => {
                    launch_studio_mini(&base_dir, "--mini-recorder");
                }
                PillAction::QuickAdd => {
                    open_quick_add();
                }
                PillAction::OpenDict(base_dir) => {
                    launch_studio(&base_dir, Some("dict"));
                }
                PillAction::CycleTheme => {
                    with_app(|state| {
                        state.engine.config.theme_index = (state.engine.config.theme_index + 1) % THEMES.len();
                        state.engine.save_config();
                        state.preview_ticks = 6;
                        refresh_ui(state);
                    });
                }
                PillAction::OpenSettings(base_dir) => {
                    launch_studio(&base_dir, None);
                }
                PillAction::DragFinished | PillAction::None => {}
            }
            return 0;
        }
        WM_RBUTTONUP => {
            open_menu(hwnd);
            return 0;
        }
        WM_TRAYICON => {
            let mouse_msg = (l_param as u32) & 0xFFFF;
            if mouse_msg == WM_RBUTTONUP {
                open_menu(hwnd);
            } else if mouse_msg == WM_LBUTTONDBLCLK || mouse_msg == WM_LBUTTONUP {
                if let Some(dir) = with_app(|s| s.base_dir.clone()) {
                    launch_studio(&dir, None);
                }
            }
            return 0;
        }
        WM_HOTKEY => {
            if w_param as i32 == HOTKEY_ID_QUICK_ADD {
                open_quick_add();
            }
            return 0;
        }
        WM_APP_RELOAD_USER => {
            with_app(|state| {
                state.user_dict_mtime = file_mtime(&state.engine.data_dir.join("user_custom.tsv"));
                state.engine.load_user_custom();
            });
            return 0;
        }
        WM_APP_SHOW_PILL => {
            let base_dir = with_app(|state| {
                state.engine.config.show_status_pill = true;
                state.engine.save_config();
                state.last_pill_sig = 0;
                refresh_ui(state);
                state.base_dir.clone()
            });
            let hinst = GetModuleHandleW(std::ptr::null());
            add_tray_icon(hwnd, hinst);
            if let Some(dir) = base_dir {
                launch_studio(&dir, None);
            }
            return 0;
        }
        WM_TIMER => {
            if w_param == 2 {
                KillTimer(hwnd, 2);
                with_app(refresh_ui);
                return 0;
            }
            // Check config.json, user_custom.tsv & hud_style.json mtime every 600ms for instant hot-reload
            let rehotkey = with_app(|state| {
                let mut rehotkey = None;
                let cfg_p = state.engine.data_dir.join("config.json");
                let u_p = state.engine.data_dir.join("user_custom.tsv");
                let hs_p = state.engine.data_dir.join("hud_style.json");
                let new_cfg_m = file_mtime(&cfg_p);
                let new_u_m = file_mtime(&u_p);
                let new_hs_m = file_mtime(&hs_p);
                let mut changed = false;
                let mut style_or_theme_changed = false;
                if new_cfg_m != state.cfg_mtime && new_cfg_m.is_some() {
                    let prev_theme = state.engine.config.theme_index;
                    let prev_scale = state.engine.config.font_scale;
                    let prev_imm = state.engine.config.immersion_mode.clone();
                    let prev_hotkey = state.engine.config.quick_add_hotkey.clone();
                    state.cfg_mtime = new_cfg_m;
                    state.engine.load_config();
                    if state.engine.config.theme_index != prev_theme
                        || (state.engine.config.font_scale - prev_scale).abs() > 0.01
                        || state.engine.config.immersion_mode != prev_imm
                    {
                        style_or_theme_changed = true;
                    }
                    if state.engine.config.quick_add_hotkey != prev_hotkey {
                        rehotkey = Some((state.engine.config.quick_add_hotkey.clone(), state.engine.data_dir.clone()));
                    }
                    state.engine.prune_and_save_history();
                    changed = true;
                }
                static HIST_TICK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                if HIST_TICK.fetch_add(1, Ordering::Relaxed) % 20 == 0 {
                    state.engine.prune_and_save_history();
                }
                if new_u_m != state.user_dict_mtime && new_u_m.is_some() {
                    state.user_dict_mtime = new_u_m;
                    state.engine.load_user_custom();
                    changed = true;
                }
                if new_hs_m != state.hud_style_mtime {
                    state.hud_style_mtime = new_hs_m;
                    style_or_theme_changed = true;
                    changed = true;
                }
                if style_or_theme_changed && !state.engine.is_hud_visible() && !state.engine.config.hide_candidate_hud {
                    state.preview_ticks = 6;
                } else if state.preview_ticks > 0 {
                    state.preview_ticks -= 1;
                    if state.preview_ticks == 0 {
                        changed = true;
                    }
                }
                if state.engine.current_cpm() != state.last_drawn_cpm {
                    state.engine.save_telemetry();
                    changed = true;
                }
                if changed {
                    refresh_ui(state);
                }
                rehotkey
            })
            .flatten();
            if let Some((spec, data_dir)) = rehotkey {
                register_quick_add_hotkey(hwnd, &spec, &data_dir, true);
            }
            return 0;
        }
        WM_APP_REFRESH => {
            with_app(refresh_ui);
            return 0;
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, w_param, l_param)
}

unsafe fn load_app_icon(hinstance: HINSTANCE) -> HICON {
    // Resource ID 1 is embedded from assets/heyu.rc (`1 ICON "heyu.ico"`).
    let ico = LoadIconW(hinstance, 1usize as *const u16);
    if ico != 0 {
        ico
    } else {
        LoadIconW(0, IDI_APPLICATION)
    }
}

unsafe fn enable_per_monitor_dpi() {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetProcAddress(hModule: HINSTANCE, lpProcName: *const u8) -> *const std::ffi::c_void;
    }
    let user32 = GetModuleHandleW(to_wide("user32.dll").as_ptr());
    if user32 != 0 {
        let p = GetProcAddress(user32, b"SetProcessDpiAwarenessContext\0".as_ptr());
        if !p.is_null() {
            let set_ctx: unsafe extern "system" fn(isize) -> BOOL = std::mem::transmute(p);
            // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4
            if set_ctx(-4) != 0 {
                return;
            }
        }
    }
    SetProcessDPIAware();
}

unsafe fn add_tray_icon(hwnd: HWND, hinstance: HINSTANCE) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as DWORD;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAYICON;
    nid.hIcon = load_app_icon(hinstance);
    let tip = to_wide("鹤羽音形 (HEYU YinXing) · 净室离线 · 中/意/西/英 极速引擎");
    for (i, &ch) in tip.iter().take(127).enumerate() {
        nid.szTip[i] = ch;
    }
    Shell_NotifyIconW(NIM_ADD, &nid);
}

unsafe fn remove_tray_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as DWORD;
    nid.hWnd = hwnd;
    nid.uID = 1;
    Shell_NotifyIconW(NIM_DELETE, &nid);
}

fn run_cli_benchmark(data_dir: &Path) {
    let t0 = Instant::now();
    let mut engine = HeyuEngine::new(data_dir);
    let load_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = Instant::now();
    let test_codes = ["hedn", "vsuo", "ybww", "abjb", "adbl", "w", "wg", "ui", "vi", "he"];
    for _ in 0..1000 {
        for code in &test_codes {
            for ch in code.chars() {
                engine.process_key(ch, false);
            }
            engine.clear_composition();
        }
    }
    let lookup_us = (t1.elapsed().as_secs_f64() * 1_000_000.0) / 10_000.0;

    println!("================================================================");
    println!("HEYU YinXing (鹤羽音形) Native Rust Engine Benchmark & Verification");
    println!("================================================================");
    println!("  Data Directory           : {}", data_dir.display());
    println!("  Dictionary Load Time     : {:.2} ms", load_ms);
    println!("  Heyu Codes Loaded        : {}", engine.flypy_map.len());
    println!("  Reverse Char Encodings   : {}", engine.reverse_map.len());
    println!("  Shuangpin Shape Hints    : {}", engine.sp2_hints.len());
    println!("  English Lexicon Keys     : {}", engine.poly_en.len());
    println!("  Italian Lexicon Keys     : {}", engine.poly_it.len());
    println!("  Spanish Lexicon Keys     : {}", engine.poly_es.len());
    println!("  Avg Keystroke Latency    : {:.2} µs (microseconds!)", lookup_us);
    println!("  Pipe Name (TSF mode)     : {}", pipe::default_pipe_name());
    println!("================================================================");
}

struct CliArgs {
    hook: bool,
    benchmark: bool,
    pipe_name: Option<String>,
    data_dir: Option<PathBuf>,
    quick_add: bool,
    anchor: Option<POINT>,
    theme: Option<usize>,
    word: Option<String>,
    timing: bool,
    wait_pid: Option<u32>,
}

fn parse_args() -> CliArgs {
    let args: Vec<String> = env::args().collect();
    let mut cli = CliArgs {
        hook: false,
        benchmark: false,
        pipe_name: None,
        data_dir: None,
        quick_add: false,
        anchor: None,
        theme: None,
        word: None,
        timing: false,
        wait_pid: None,
    };
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--hook" => cli.hook = true,
            "--benchmark" | "--verify" => cli.benchmark = true,
            "--pipe-name" if i + 1 < args.len() => {
                i += 1;
                cli.pipe_name = Some(args[i].clone());
            }
            "--data-dir" if i + 1 < args.len() => {
                i += 1;
                cli.data_dir = Some(PathBuf::from(&args[i]));
            }
            "--quick-add" => cli.quick_add = true,
            "--timing" => cli.timing = true,
            "--anchor" if i + 1 < args.len() => {
                i += 1;
                let mut it = args[i].split(',').map(|v| v.trim().parse::<i32>());
                if let (Some(Ok(x)), Some(Ok(y))) = (it.next(), it.next()) {
                    cli.anchor = Some(POINT { x, y });
                }
            }
            "--theme" if i + 1 < args.len() => {
                i += 1;
                cli.theme = args[i].parse().ok();
            }
            "--word" if i + 1 < args.len() => {
                i += 1;
                cli.word = Some(args[i].clone());
            }
            "--wait-pid" if i + 1 < args.len() => {
                i += 1;
                cli.wait_pid = args[i].parse().ok();
            }
            _ => {}
        }
        i += 1;
    }
    cli
}

/// Relaunch this server (tray "↻ 重启引擎"): same arguments, and the child first waits for us to exit.
fn relaunch_self() {
    let Ok(exe) = env::current_exe() else { return };
    let mut args: Vec<String> = Vec::new();
    let mut it = env::args().skip(1);
    while let Some(a) = it.next() {
        if a == "--wait-pid" {
            it.next();
            continue;
        }
        args.push(a);
    }
    args.push("--wait-pid".to_string());
    args.push(std::process::id().to_string());
    spawn_detached(&exe, &args);
}

fn main() {
    let base_dir = find_base_dir();
    let cli = parse_args();
    if let Some(pid) = cli.wait_pid {
        wait_for_process_exit(pid, 8000);
    }
    let data_dir = paths::resolve_data_dir(&base_dir, cli.data_dir.as_deref());
    let _ = fs::create_dir_all(&data_dir);
    install_panic_logger(&data_dir);
    if cli.benchmark {
        run_cli_benchmark(&data_dir);
        return;
    }
    if cli.quick_add {
        unsafe { enable_per_monitor_dpi() };
        quickadd::run(
            &base_dir,
            &data_dir,
            quickadd::QuickAddArgs { anchor: cli.anchor, theme: cli.theme, word: cli.word.clone(), timing: cli.timing },
        );
        return;
    }
    // A custom pipe name means a private (test) instance: no single-instance check, no tray icon, no hook.
    let private_instance = cli.pipe_name.is_some();
    // Always keep the keyboard hook active initially so typing works immediately in all open windows
    // even if Windows taskbar is still on Microsoft Pinyin or ENG; as soon as `heyu_tsf.dll` sends
    // a key/focus event over the Named Pipe, `process_ipc_job` automatically unhooks `hhook`!
    let use_hook_initially = !private_instance;
    let run_mode = if use_hook_initially { RunMode::Hook } else { RunMode::Pipe };

    unsafe {
        enable_per_monitor_dpi();

        // Single-instance check: if window class "HeyuStatusPillWindow" already exists, exit cleanly
        let class_name = to_wide("HeyuLayeredWindowClass");
        let pill_title = to_wide("HeyuStatusPillWindow");
        if !private_instance {
            let existing = FindWindowW(class_name.as_ptr(), pill_title.as_ptr());
            if existing != 0 {
                PostMessageW(existing, WM_APP_SHOW_PILL, 0, 0);
                return;
            }
        }

        let hinstance = GetModuleHandleW(std::ptr::null());
        let app_icon = load_app_icon(hinstance);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as UINT,
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: app_icon,
            hCursor: LoadCursorW(0, IDC_HAND),
            hbrBackground: 0,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: app_icon,
        };
        RegisterClassExW(&wc);

        let hud_title = to_wide("HeyuCandidateHudWindow");
        let hwnd_hud = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            hud_title.as_ptr(),
            WS_POPUP,
            300,
            300,
            500,
            100,
            0,
            0,
            hinstance,
            std::ptr::null(),
        );

        let sw = GetSystemMetrics(SM_CXSCREEN).max(1280);
        let sh = GetSystemMetrics(SM_CYSCREEN).max(720);

        let engine = HeyuEngine::new(&data_dir);
        let default_px = if engine.config.pill_x > 10 { engine.config.pill_x } else { sw - 440 };
        let default_py = if engine.config.pill_y > 10 { engine.config.pill_y } else { sh - 110 };

        let hwnd_pill = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            pill_title.as_ptr(),
            WS_POPUP | WS_VISIBLE,
            default_px,
            default_py,
            PILL_W,
            44,
            0,
            0,
            hinstance,
            std::ptr::null(),
        );

        let tip_title = to_wide("HeyuPillTooltipWindow");
        let hwnd_tip = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            class_name.as_ptr(),
            tip_title.as_ptr(),
            WS_POPUP,
            default_px,
            default_py - 40,
            260,
            36,
            0,
            0,
            hinstance,
            std::ptr::null(),
        );

        let hhook = if use_hook_initially {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), hinstance, 0)
        } else {
            0
        };

        let cfg_mtime = file_mtime(&data_dir.join("config.json"));
        let user_dict_mtime = file_mtime(&data_dir.join("user_custom.tsv"));
        let hud_style_mtime = file_mtime(&data_dir.join("hud_style.json"));

        let mut app_state = AppState {
            base_dir: base_dir.clone(),
            engine,
            session: ImeSession::new(),
            run_mode,
            hhook,
            fonts: FontSet::new(),
            hwnd_hud,
            hwnd_pill,
            hwnd_tip,
            caret_pos: POINT { x: 0, y: 0 },
            pill_pos: POINT { x: default_px, y: default_py },
            pill_hover: None,
            pill_press_hit: None,
            pill_dragged: false,
            tracking_pill_mouse: false,
            dragging_pill: false,
            dragging_hud: false,
            drag_offset: POINT::default(),
            drag_start_screen: POINT::default(),
            cfg_mtime,
            user_dict_mtime,
            hud_style_mtime,
            preview_ticks: 0,
            last_drawn_cpm: 0,
            last_pill_sig: 0,
        };

        app_state.engine.save_telemetry();
        refresh_ui(&mut app_state);
        let quick_add_hotkey = app_state.engine.config.quick_add_hotkey.clone();
        if !private_instance {
            add_tray_icon(hwnd_pill, hinstance);
        }
        SetTimer(hwnd_pill, 1, 600, std::ptr::null());

        APP.with(|cell| {
            *cell.borrow_mut() = Some(app_state);
        });

        if !private_instance {
            register_quick_add_hotkey(hwnd_pill, &quick_add_hotkey, &data_dir, true);
            maybe_spawn_voice(&data_dir);
        }

        // Always start the Named Pipe server (unless `--hook` was explicitly requested alone)
        // so TSF clients can connect at any time!
        if !cli.hook || cli.pipe_name.is_some() {
            IPC_HWND.store(hwnd_pill, Ordering::Release);
            let name = cli.pipe_name.clone().unwrap_or_else(pipe::default_pipe_name);
            if let Err(e) = pipe::start_pipe_server(&name, Arc::new(pipe_handler)) {
                let msg = format!("[{:?}] pipe server failed to start on {}: {}\n", SystemTime::now(), name, e);
                let _ = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(data_dir.join("crash.log"))
                    .and_then(|mut f| std::io::Write::write_all(&mut f, msg.as_bytes()));
                if private_instance {
                    return;
                }
            }
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        IPC_HWND.store(0, Ordering::Release);
        UnregisterHotKey(hwnd_pill, HOTKEY_ID_QUICK_ADD);
        if let Some(h) = with_app(|s| s.hhook) {
            if h != 0 {
                UnhookWindowsHookEx(h);
            }
        }
        if !private_instance {
            remove_tray_icon(hwnd_pill);
        }
        if RESTART_REQUESTED.load(Ordering::Acquire) && !private_instance {
            DestroyWindow(hwnd_hud);
            DestroyWindow(hwnd_pill);
            DestroyWindow(hwnd_tip);
            relaunch_self();
        }
    }
}
