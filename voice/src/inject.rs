//! Text insertion into the focused application via SendInput(KEYEVENTF_UNICODE).
//! Each UTF-16 code unit becomes a VK_PACKET key press; surrogate pairs are sent as two units
//! (the system recombines them into one WM_CHAR pair). The HEYU TSF text service forwards
//! VK_PACKET to heyu-ime.exe, which passes it through (no ASCII mapping exists for 0xE7).

use crate::sys::*;

pub const HEYU_MAGIC_EXTRA: usize = 0x4845_5955; // "HEYU"
const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;
const WM_GETTEXTLENGTH: u32 = 0x000E;

fn key_down(vk: i32) -> bool {
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

#[allow(dead_code)]
fn ms_since_last_input() -> u32 {
    unsafe {
        let mut lii = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut lii) != 0 {
            GetTickCount().wrapping_sub(lii.dwTime)
        } else {
            10_000
        }
    }
}

/// Wait (up to `max_ms`) until the user has released Ctrl/Alt/Shift/Win, otherwise the injected
/// characters would be combined with the still-held hotkey modifiers (Ctrl+字 → shortcuts).
pub fn wait_modifiers_released(max_ms: u32) {
    let mut waited = 0;
    while waited < max_ms && [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].iter().any(|&k| key_down(k)) {
        unsafe { Sleep(15) };
        waited += 15;
    }
}

/// Return the top-level root window (`GA_ROOT`) for a given window handle.
pub fn root_hwnd(hwnd: HWND) -> HWND {
    if hwnd.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let r = GetAncestor(hwnd, GA_ROOT);
        if r.is_null() { hwnd } else { r }
    }
}

/// Return the top-level root window (`GA_ROOT`) at screen coordinate `pt`.
pub fn root_window_from_point(pt: POINT) -> HWND {
    unsafe { root_hwnd(WindowFromPoint(pt)) }
}

/// Check if a window handle (or its `GA_ROOT` top-level ancestor) belongs to HEYU's own UI/control windows.
pub fn is_heyu_own_hwnd(hwnd: HWND) -> bool {
    if hwnd.is_null() {
        return false;
    }
    let root = root_hwnd(hwnd);
    unsafe {
        for h in [hwnd, root] {
            if h.is_null() {
                continue;
            }
            let mut cls_buf = [0u16; 128];
            let n = GetClassNameW(h, cls_buf.as_mut_ptr(), cls_buf.len() as i32);
            if n > 0 {
                let cls = String::from_utf16_lossy(&cls_buf[..n as usize]);
                if cls.contains("HEYU") || cls.contains("CustomTkinter") {
                    return true;
                }
            }
            let mut title_buf = [0u16; 256];
            let tn = GetWindowTextW(h, title_buf.as_mut_ptr(), title_buf.len() as i32);
            if tn > 0 {
                let title = String::from_utf16_lossy(&title_buf[..tn as usize]);
                if title.contains("鹤羽")
                    || title.contains("HEYU")
                    || title.contains("实时声音转写")
                    || title.contains("录音")
                    || title.contains("快速造词")
                {
                    return true;
                }
            }
        }
        false
    }
}

/// Get a concise, human-friendly title for a top-level target window (e.g. `"Luciole"`, `"Google Chrome"`, `"记事本"`).
pub fn window_short_title(hwnd: HWND) -> String {
    let root = root_hwnd(hwnd);
    if root.is_null() {
        return String::new();
    }
    unsafe {
        if IsWindow(root) == 0 {
            return String::new();
        }
        let mut title_buf = [0u16; 256];
        let tn = GetWindowTextW(root, title_buf.as_mut_ptr(), title_buf.len() as i32);
        if tn > 0 {
            let full = String::from_utf16_lossy(&title_buf[..tn as usize]).trim().to_string();
            if !full.is_empty() {
                // If title has " - AppName", keep either short full title or last segment + prefix
                let chars: Vec<char> = full.chars().collect();
                if chars.len() <= 18 {
                    return full;
                }
                if let Some(idx) = full.rfind(" - ") {
                    let app_part = full[idx + 3..].trim();
                    let doc_part: String = full[..idx].chars().take(8).collect();
                    return format!("{} · {}", doc_part, app_part);
                }
                let s: String = chars.into_iter().take(16).collect();
                return format!("{}…", s);
            }
        }
        let mut cls_buf = [0u16; 128];
        let n = GetClassNameW(root, cls_buf.as_mut_ptr(), cls_buf.len() as i32);
        if n > 0 {
            return String::from_utf16_lossy(&cls_buf[..n as usize]);
        }
        "目标窗口".to_string()
    }
}

/// Reliably bring `target_root` to foreground using `AttachThreadInput` when needed.
pub fn force_focus_window(target_root: HWND) -> bool {
    let root = root_hwnd(target_root);
    if root.is_null() {
        return false;
    }
    unsafe {
        if IsWindow(root) == 0 {
            return false;
        }
        let fg = GetForegroundWindow();
        if fg == root {
            return true;
        }
        let cur_tid = GetCurrentThreadId();
        let fg_tid = if !fg.is_null() {
            GetWindowThreadProcessId(fg, std::ptr::null_mut())
        } else {
            0
        };
        if fg_tid != 0 && fg_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, fg_tid, 1);
            let _ = SetForegroundWindow(root);
            let _ = AttachThreadInput(cur_tid, fg_tid, 0);
        } else {
            let _ = SetForegroundWindow(root);
        }
        GetForegroundWindow() == root
    }
}

/// Find the focused child control of the current foreground window.
pub fn current_focus_hwnd() -> HWND {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return std::ptr::null_mut();
        }
        let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let mut gti = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if tid != 0 && GetGUIThreadInfo(tid, &mut gti) != 0 && gti.hwndFocus != 0 {
            gti.hwndFocus as HWND
        } else {
            fg
        }
    }
}

/// In `"independent"` dual-cursor mode for standard Win32 Edit/RichEdit controls (e.g. Notepad),
/// insert `text` at the independent `voice_pos` offset and immediately restore the user's typing caret!
pub fn try_inject_independent_edit(
    text: &str,
    locked_hwnd: &mut HWND,
    voice_pos: &mut Option<u32>,
) -> Option<usize> {
    unsafe {
        let hwnd = if !locked_hwnd.is_null() {
            *locked_hwnd
        } else {
            let f = current_focus_hwnd();
            *locked_hwnd = f;
            f
        };
        if hwnd.is_null() {
            return None;
        }
        let mut cls_buf = [0u16; 64];
        let n = GetClassNameW(hwnd, cls_buf.as_mut_ptr(), cls_buf.len() as i32);
        if n <= 0 {
            return None;
        }
        let cls = String::from_utf16_lossy(&cls_buf[..n as usize]).to_ascii_lowercase();
        if !(cls == "edit" || cls.contains("richedit")) {
            return None;
        }

        let clean = text.replace("\r\n", " ").replace('\n', " ");
        let wtext = wide(&clean);
        let units = wtext.len().saturating_sub(1);
        if units == 0 {
            return Some(0);
        }

        let mut user_start: u32 = 0;
        let mut user_end: u32 = 0;
        SendMessageW(
            hwnd,
            EM_GETSEL,
            (&mut user_start as *mut u32) as WPARAM,
            (&mut user_end as *mut u32) as LPARAM,
        );
        let total_len = SendMessageW(hwnd, WM_GETTEXTLENGTH, 0, 0).max(0) as u32;
        let insert_at = voice_pos.unwrap_or(total_len).min(total_len);

        // Move selection to independent voice cursor, insert text, then restore user's typing cursor
        SendMessageW(hwnd, EM_SETSEL, insert_at as WPARAM, insert_at as LPARAM);
        SendMessageW(hwnd, EM_REPLACESEL, 1, wtext.as_ptr() as LPARAM);

        let new_voice_pos = insert_at.saturating_add(units as u32);
        *voice_pos = Some(new_voice_pos);

        // If user's typing cursor was at a different spot, restore it (adjusting offset if after insert_at)
        let adj_start = if user_start > insert_at {
            user_start.saturating_add(units as u32)
        } else {
            user_start
        };
        let adj_end = if user_end > insert_at {
            user_end.saturating_add(units as u32)
        } else {
            user_end
        };
        if user_start != insert_at || user_end != insert_at {
            SendMessageW(hwnd, EM_SETSEL, adj_start as WPARAM, adj_end as LPARAM);
        }
        Some(units)
    }
}

fn unicode_input(unit: u16, up: bool) -> INPUT {
    INPUT {
        type_: INPUT_KEYBOARD,
        ki: KEYBDINPUT {
            wVk: 0,
            wScan: unit,
            dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 },
            time: 0,
            dwExtraInfo: HEYU_MAGIC_EXTRA,
        },
        _pad: 0,
    }
}

/// Send a harmless unassigned key press (`wVk = 0x88` down + up) so `heyu_tsf.dll` in the focused app
/// commits any queued voice text atomically via TSF and reports the exact caret screen rectangle!
pub fn send_caret_ping() {
    let packets = [
        INPUT {
            type_: INPUT_KEYBOARD,
            ki: KEYBDINPUT {
                wVk: 0x88,
                wScan: 0,
                dwFlags: 0,
                time: 0,
                dwExtraInfo: HEYU_MAGIC_EXTRA,
            },
            _pad: 0,
        },
        INPUT {
            type_: INPUT_KEYBOARD,
            ki: KEYBDINPUT {
                wVk: 0x88,
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: HEYU_MAGIC_EXTRA,
            },
            _pad: 0,
        },
    ];
    unsafe {
        let _ = SendInput(packets.len() as u32, packets.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

/// Collapse an active text selection to its right edge by sending a single `VK_RIGHT` keystroke
/// so highlighted text is never overwritten!
pub fn send_collapse_selection_right() {
    let packets = [
        INPUT {
            type_: INPUT_KEYBOARD,
            ki: KEYBDINPUT {
                wVk: 0x27, // VK_RIGHT
                wScan: 0,
                dwFlags: 0x0001, // KEYEVENTF_EXTENDEDKEY
                time: 0,
                dwExtraInfo: HEYU_MAGIC_EXTRA,
            },
            _pad: 0,
        },
        INPUT {
            type_: INPUT_KEYBOARD,
            ki: KEYBDINPUT {
                wVk: 0x27,
                wScan: 0,
                dwFlags: 0x0001 | KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: HEYU_MAGIC_EXTRA,
            },
            _pad: 0,
        },
    ];
    unsafe {
        let _ = SendInput(packets.len() as u32, packets.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

/// Query the screen `RECT` of a top-level window `hwnd`.
pub fn window_rect(hwnd: HWND) -> Option<RECT> {
    if hwnd.is_null() {
        return None;
    }
    unsafe {
        #[link(name = "user32")]
        extern "system" {
            fn GetWindowRect(h: HWND, r: *mut RECT) -> BOOL;
        }
        if IsWindow(hwnd) == 0 {
            return None;
        }
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r) != 0 && r.right > r.left && r.bottom > r.top && r.left > -10000 && r.top > -10000 {
            Some(r)
        } else {
            None
        }
    }
}

/// Returns true if the left or right mouse button is currently held down (e.g. window drag or text selection).
pub fn is_mouse_button_down() -> bool {
    key_down(0x01) || key_down(0x02)
}

/// Commit `text` atomically via `heyu-ime.exe` -> `heyu_tsf.dll` (`ITfRange::SetText` + `GetTextExt`),
/// falling back to `type_text` if the focused application is not currently using HEYU TSF.
pub fn commit_via_tsf_or_type(text: &str) -> Result<usize, String> {
    let clean = text.replace("\r\n", " ").replace('\n', " ");
    let units_len = clean.encode_utf16().count();
    if units_len == 0 {
        return Ok(0);
    }
    let commit_path = std::env::temp_dir().join("heyu_voice_commit.utf8");
    if std::fs::write(&commit_path, clean.as_bytes()).is_ok() {
        send_caret_ping();
        for _ in 0..9 {
            unsafe { Sleep(5) };
            if !commit_path.exists() {
                return Ok(units_len);
            }
        }
        let _ = std::fs::remove_file(&commit_path);
    }
    type_text(&clean)
}

/// Returns true if the screen point `pt` belongs to one of HEYU's own floating/studio windows.
pub fn is_heyu_own_window(pt: POINT) -> bool {
    unsafe { is_heyu_own_hwnd(WindowFromPoint(pt)) }
}

/// Query `GetGUIThreadInfo` for native Win32 caret coordinates `(x_right, y_center, line_h)`.
pub fn query_gui_caret_screen_point() -> Option<(i32, i32, i32)> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return None;
        }
        let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let mut gti = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if tid != 0 && GetGUIThreadInfo(tid, &mut gti) != 0 && gti.hwndCaret != 0 {
            let r = gti.rcCaret;
            let mut tl = POINT { x: r.left, y: r.top };
            let mut br = POINT { x: r.right, y: r.bottom };
            ClientToScreen(gti.hwndCaret as HWND, &mut tl);
            ClientToScreen(gti.hwndCaret as HWND, &mut br);
            if !(tl.x == 0 && tl.y == 0 && br.x == 0 && br.y == 0) {
                let lh = (br.y - tl.y).clamp(16, 56);
                return Some((br.x.max(tl.x), (tl.y + br.y) / 2, lh));
            }
        }
        None
    }
}

/// Synthesize a fast left-click at screen coordinate `(x, y)` (tagged with `HEYU_MAGIC_EXTRA`)
/// while immediately restoring the user's physical mouse cursor position so the pointer never jumps!
pub fn click_screen_point_preserve_mouse(x: i32, y: i32) {
    unsafe {
        let mut orig = POINT::default();
        let has_orig = GetCursorPos(&mut orig) != 0;

        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1);

        let norm_x = (((x - vx) as i64 * 65535) / (vw - 1).max(1) as i64) as i32;
        let norm_y = (((y - vy) as i64 * 65535) / (vh - 1).max(1) as i64) as i32;
        let base_flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;

        let packets = [
            INPUT_MOUSE_PACKET {
                type_: INPUT_MOUSE,
                _align: 0,
                mi: MOUSEINPUT {
                    dx: norm_x,
                    dy: norm_y,
                    mouseData: 0,
                    dwFlags: base_flags | MOUSEEVENTF_LEFTDOWN,
                    time: 0,
                    dwExtraInfo: HEYU_MAGIC_EXTRA,
                },
            },
            INPUT_MOUSE_PACKET {
                type_: INPUT_MOUSE,
                _align: 0,
                mi: MOUSEINPUT {
                    dx: norm_x,
                    dy: norm_y,
                    mouseData: 0,
                    dwFlags: base_flags | MOUSEEVENTF_LEFTUP,
                    time: 0,
                    dwExtraInfo: HEYU_MAGIC_EXTRA,
                },
            },
        ];
        let _ = SendInput(
            packets.len() as u32,
            packets.as_ptr() as *const INPUT,
            std::mem::size_of::<INPUT_MOUSE_PACKET>() as i32,
        );
        if has_orig {
            let _ = SetCursorPos(orig.x, orig.y);
        }
    }
}

/// Check whether `hwnd` belongs to `Luciole` with an active native ProseMirror Dual-Cursor bridge.
pub fn is_luciole_hwnd(hwnd: HWND) -> bool {
    let root = root_hwnd(hwnd);
    if root.is_null() {
        return false;
    }
    let status_path = std::env::temp_dir().join("heyu_luciole_status.json");
    let Ok(meta) = std::fs::metadata(&status_path) else {
        return false;
    };
    let fresh = meta
        .modified()
        .ok()
        .and_then(|m| std::time::SystemTime::now().duration_since(m).ok())
        .map(|d| d.as_secs() <= 6)
        .unwrap_or(false);
    if !fresh {
        return false;
    }
    unsafe {
        let mut title_buf = [0u16; 256];
        let tn = GetWindowTextW(root, title_buf.as_mut_ptr(), title_buf.len() as i32);
        if tn > 0 {
            let title = String::from_utf16_lossy(&title_buf[..tn as usize]).to_ascii_lowercase();
            if title.contains("luciole") {
                return true;
            }
        }
        let mut win_pid: u32 = 0;
        let _ = GetWindowThreadProcessId(root, &mut win_pid);
        if win_pid != 0 {
            if let Ok(txt) = std::fs::read_to_string(&status_path) {
                if let Some(pid) = crate::config::json_num(&txt, "pid").map(|v| v as u32) {
                    if pid == win_pid {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn escape_json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

pub fn send_luciole_cmd(body: &str, seq: u64, wait_ack_ms: u32) -> bool {
    let cmd_path = std::env::temp_dir().join("heyu_luciole_cmd.json");
    let status_path = std::env::temp_dir().join("heyu_luciole_status.json");
    if std::fs::write(&cmd_path, body.as_bytes()).is_err() {
        return false;
    }
    if wait_ack_ms == 0 {
        return true;
    }
    let mut waited = 0u32;
    while waited < wait_ack_ms {
        unsafe { Sleep(5) };
        waited += 5;
        if let Ok(txt) = std::fs::read_to_string(&status_path) {
            let ack = crate::config::json_num(&txt, "ack_seq")
                .map(|v| v as u64)
                .unwrap_or(0);
            if ack >= seq {
                return true;
            }
        }
    }
    false
}

pub fn send_luciole_insert(seq: u64, text: &str, cursor_mode: &str) -> bool {
    let esc = escape_json_str(text);
    let body = format!(
        "{{\"seq\":{},\"cmd\":\"insert\",\"text\":\"{}\",\"cursor_mode\":\"{}\"}}\n",
        seq, esc, cursor_mode
    );
    send_luciole_cmd(&body, seq, 150)
}

pub fn send_luciole_anchor(seq: u64, screen_x: i32, screen_y: i32, cursor_mode: &str) -> bool {
    let body = format!(
        "{{\"seq\":{},\"cmd\":\"anchor\",\"screen_x\":{},\"screen_y\":{},\"cursor_mode\":\"{}\"}}\n",
        seq, screen_x, screen_y, cursor_mode
    );
    send_luciole_cmd(&body, seq, 80)
}

pub fn send_luciole_pin_caret(seq: u64, cursor_mode: &str) -> bool {
    let body = format!(
        "{{\"seq\":{},\"cmd\":\"pin_caret\",\"cursor_mode\":\"{}\"}}\n",
        seq, cursor_mode
    );
    send_luciole_cmd(&body, seq, 60)
}

pub fn send_luciole_sync(seq: u64, active: bool, cursor_mode: &str) {
    let body = format!(
        "{{\"seq\":{},\"cmd\":\"sync\",\"active\":{},\"cursor_mode\":\"{}\"}}\n",
        seq, active, cursor_mode
    );
    let _ = send_luciole_cmd(&body, seq, 0);
}

pub fn send_luciole_undo(seq: u64, cursor_mode: &str) -> bool {
    let body = format!(
        "{{\"seq\":{},\"cmd\":\"undo\",\"cursor_mode\":\"{}\"}}\n",
        seq, cursor_mode
    );
    send_luciole_cmd(&body, seq, 100)
}

/// Returns the number of UTF-16 units delivered.
pub fn type_text(text: &str) -> Result<usize, String> {
    let units: Vec<u16> = text.replace("\r\n", " ").replace('\n', " ").encode_utf16().collect();
    let mut sent = 0usize;
    let mut i = 0;
    const CHUNK: usize = 48;
    while i < units.len() {
        let mut end = (i + CHUNK).min(units.len());
        // never split a surrogate pair across chunks
        if end < units.len() && (0xD800..0xDC00).contains(&units[end - 1]) {
            end += 1;
        }
        let mut batch = Vec::with_capacity((end - i) * 2);
        for &u in &units[i..end] {
            batch.push(unicode_input(u, false));
            batch.push(unicode_input(u, true));
        }
        let n = unsafe { SendInput(batch.len() as u32, batch.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
        if n as usize != batch.len() {
            return Err(if n == 0 { "目标窗口拒绝输入（可能以管理员身份运行）".into() } else { "部分文字未能输入".into() });
        }
        sent += end - i;
        i = end;
        if i < units.len() {
            unsafe { Sleep(8) };
        }
    }
    // Ping TSF at the end of typing so `heyu_tsf.dll` reports the exact new caret rectangle!
    send_caret_ping();
    Ok(sent)
}

/// Send `count` Backspace keystrokes to delete the previously typed voice segment.
pub fn send_backspaces(count: usize) -> Result<usize, String> {
    if count == 0 {
        return Ok(0);
    }
    let mut batch = Vec::with_capacity(count.min(128) * 2);
    let n_keys = count.min(256);
    for _ in 0..n_keys {
        batch.push(INPUT {
            type_: INPUT_KEYBOARD,
            ki: KEYBDINPUT { wVk: VK_BACK, wScan: 0, dwFlags: 0, time: 0, dwExtraInfo: 0 },
            _pad: 0,
        });
        batch.push(INPUT {
            type_: INPUT_KEYBOARD,
            ki: KEYBDINPUT { wVk: VK_BACK, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 },
            _pad: 0,
        });
    }
    let n = unsafe { SendInput(batch.len() as u32, batch.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
    if n == 0 {
        Err("目标窗口拒绝退格操作".into())
    } else {
        Ok(n_keys)
    }
}
