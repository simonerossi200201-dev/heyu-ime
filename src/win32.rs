//! HEYU (鹤羽输入法) - Pure Zero-Dependency Win32 FFI Layer
//! ==========================================================
//! Direct bindings to Windows OS DLLs (`user32`, `gdi32`, `shell32`, `kernel32`).
//! Zero networking DLLs (`ws2_32`, `winhttp`, `wininet`) are referenced or linked.

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::c_void;

pub type HWND = isize;
pub type HINSTANCE = isize;
pub type HHOOK = isize;
pub type HDC = isize;
pub type HBITMAP = isize;
pub type HGDIOBJ = isize;
pub type HFONT = isize;
pub type HICON = isize;
pub type HCURSOR = isize;
pub type HMENU = isize;
pub type HBRUSH = isize;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type BOOL = i32;
pub type UINT = u32;
pub type DWORD = u32;
pub type WORD = u16;
pub type BYTE = u8;
pub type LONG = i32;
pub type ULONG_PTR = usize;
pub type ATOM = u16;

pub const WH_KEYBOARD_LL: i32 = 13;
pub const HC_ACTION: i32 = 0;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_SYSKEYUP: u32 = 0x0105;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_USER: u32 = 0x0400;
pub const WM_TRAYICON: u32 = WM_USER + 101;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_NCHITTEST: u32 = 0x0084;
pub const WM_MOUSEACTIVATE: u32 = 0x0021;
pub const MA_NOACTIVATE: isize = 3;

pub const WS_POPUP: u32 = 0x80000000;
pub const WS_VISIBLE: u32 = 0x10000000;
pub const WS_EX_TOPMOST: u32 = 0x00000008;
pub const WS_EX_TOOLWINDOW: u32 = 0x00000080;
pub const WS_EX_LAYERED: u32 = 0x00080000;
pub const WS_EX_NOACTIVATE: u32 = 0x08000000;
pub const WS_EX_TRANSPARENT: u32 = 0x00000020;

pub const SW_SHOWNOACTIVATE: i32 = 4;
pub const SW_HIDE: i32 = 0;
pub const HWND_TOPMOST: isize = -1;
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOACTIVATE: u32 = 0x0010;
pub const SWP_SHOWWINDOW: u32 = 0x0040;

pub const ULW_ALPHA: u32 = 0x00000002;
pub const AC_SRC_OVER: u8 = 0x00;
pub const AC_SRC_ALPHA: u8 = 0x01;
pub const DIB_RGB_COLORS: u32 = 0;
pub const BI_RGB: u32 = 0;
pub const TRANSPARENT: i32 = 1;
pub const ANTIALIASED_QUALITY: u32 = 4;
pub const CLEARTYPE_QUALITY: u32 = 5;
pub const DEFAULT_CHARSET: u32 = 1;
pub const OUT_DEFAULT_PRECIS: u32 = 0;
pub const CLIP_DEFAULT_PRECIS: u32 = 0;
pub const DEFAULT_PITCH: u32 = 0;
pub const FW_NORMAL: i32 = 400;
pub const FW_MEDIUM: i32 = 500;
#[allow(non_upper_case_globals)]
pub const FW_semibold: i32 = 600;
pub const FW_SEMIBOLD: i32 = 600;
pub const FW_BOLD: i32 = 700;
pub const SRCCOPY: u32 = 0x00CC0020;
pub const BLACKNESS: u32 = 0x00000042;

pub const INPUT_KEYBOARD: u32 = 1;
pub const KEYEVENTF_KEYUP: u32 = 0x0002;
pub const KEYEVENTF_UNICODE: u32 = 0x0004;

pub const VK_SHIFT: i32 = 0x10;
pub const VK_CONTROL: i32 = 0x11;
pub const VK_MENU: i32 = 0x12; // Alt
pub const VK_CAPITAL: i32 = 0x14; // CapsLock
pub const VK_LSHIFT: i32 = 0xA0;
pub const VK_RSHIFT: i32 = 0xA1;
pub const VK_LCONTROL: i32 = 0xA2;
pub const VK_RCONTROL: i32 = 0xA3;
pub const VK_LMENU: i32 = 0xA4;
pub const VK_RMENU: i32 = 0xA5;
pub const VK_LWIN: i32 = 0x5B;
pub const VK_RWIN: i32 = 0x5C;

pub const NIM_ADD: u32 = 0x00000000;
pub const NIM_MODIFY: u32 = 0x00000001;
pub const NIM_DELETE: u32 = 0x00000002;
pub const NIF_MESSAGE: u32 = 0x00000001;
pub const NIF_ICON: u32 = 0x00000002;
pub const NIF_TIP: u32 = 0x00000004;
pub const IDI_APPLICATION: *const u16 = 32512 as *const u16;
pub const IDC_ARROW: *const u16 = 32512 as *const u16;
pub const IDC_HAND: *const u16 = 32649 as *const u16;

pub const MF_STRING: u32 = 0x00000000;
pub const MF_SEPARATOR: u32 = 0x00000800;
pub const MF_CHECKED: u32 = 0x00000008;
pub const TPM_RIGHTBUTTON: u32 = 0x0002;
pub const TPM_BOTTOMALIGN: u32 = 0x0020;
pub const TPM_NONOTIFY: u32 = 0x0080;
pub const TPM_RETURNCMD: u32 = 0x0100;

pub const SM_CXSCREEN: i32 = 0;
pub const SM_CYSCREEN: i32 = 1;

/// Magic tag placed in `dwExtraInfo` of `SendInput` events so our hook ignores our own injected text
pub const HEYU_MAGIC_EXTRA: usize = 0x4845_5955; // "HEYU"

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct SIZE {
    pub cx: i32,
    pub cy: i32,
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct KBDLLHOOKSTRUCT {
    pub vkCode: DWORD,
    pub scanCode: DWORD,
    pub flags: DWORD,
    pub time: DWORD,
    pub dwExtraInfo: ULONG_PTR,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct KEYBDINPUT {
    pub wVk: WORD,
    pub wScan: WORD,
    pub dwFlags: DWORD,
    pub time: DWORD,
    pub dwExtraInfo: ULONG_PTR,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union INPUT_UNION {
    pub ki: KEYBDINPUT,
    pub _pad: [u64; 4], // Ensure union size matches largest member (MOUSEINPUT = 32 bytes on x64)
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct INPUT {
    pub r#type: DWORD,
    pub u: INPUT_UNION,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct GUITHREADINFO {
    pub cbSize: DWORD,
    pub flags: DWORD,
    pub hwndActive: HWND,
    pub hwndFocus: HWND,
    pub hwndCapture: HWND,
    pub hwndMenuOwner: HWND,
    pub hwndMoveSize: HWND,
    pub hwndCaret: HWND,
    pub rcCaret: RECT,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct WNDCLASSEXW {
    pub cbSize: UINT,
    pub style: UINT,
    pub lpfnWndProc: Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: HCURSOR,
    pub hbrBackground: HBRUSH,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
    pub hIconSm: HICON,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: UINT,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: DWORD,
    pub pt: POINT,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct BLENDFUNCTION {
    pub BlendOp: BYTE,
    pub BlendFlags: BYTE,
    pub SourceConstantAlpha: BYTE,
    pub AlphaFormat: BYTE,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct BITMAPINFOHEADER {
    pub biSize: DWORD,
    pub biWidth: LONG,
    pub biHeight: LONG,
    pub biPlanes: WORD,
    pub biBitCount: WORD,
    pub biCompression: DWORD,
    pub biSizeImage: DWORD,
    pub biXPelsPerMeter: LONG,
    pub biYPelsPerMeter: LONG,
    pub biClrUsed: DWORD,
    pub biClrImportant: DWORD,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct BITMAPINFO {
    pub bmiHeader: BITMAPINFOHEADER,
    pub bmiColors: [u32; 1],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct NOTIFYICONDATAW {
    pub cbSize: DWORD,
    pub hWnd: HWND,
    pub uID: UINT,
    pub uFlags: UINT,
    pub uCallbackMessage: UINT,
    pub hIcon: HICON,
    pub szTip: [u16; 128],
    pub dwState: DWORD,
    pub dwStateMask: DWORD,
    pub szInfo: [u16; 256],
    pub uTimeoutOrVersion: UINT,
    pub szInfoTitle: [u16; 64],
    pub dwInfoFlags: DWORD,
}

#[link(name = "user32")]
extern "system" {
    pub fn GetMessageW(lpMsg: *mut MSG, hWnd: HWND, wMsgFilterMin: UINT, wMsgFilterMax: UINT) -> BOOL;
    pub fn TranslateMessage(lpMsg: *const MSG) -> BOOL;
    pub fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
    pub fn PostQuitMessage(nExitCode: i32);
    pub fn DefWindowProcW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
    pub fn RegisterClassExW(lpwcx: *const WNDCLASSEXW) -> ATOM;
    pub fn CreateWindowExW(
        dwExStyle: DWORD,
        lpClassName: *const u16,
        lpWindowName: *const u16,
        dwStyle: DWORD,
        X: i32,
        Y: i32,
        nWidth: i32,
        nHeight: i32,
        hWndParent: HWND,
        hMenu: HMENU,
        hInstance: HINSTANCE,
        lpParam: *const c_void,
    ) -> HWND;
    pub fn DestroyWindow(hWnd: HWND) -> BOOL;
    pub fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> BOOL;
    pub fn SetWindowPos(
        hWnd: HWND,
        hWndInsertAfter: HWND,
        X: i32,
        Y: i32,
        cx: i32,
        cy: i32,
        uFlags: UINT,
    ) -> BOOL;
    pub fn UpdateLayeredWindow(
        hWnd: HWND,
        hdcDst: HDC,
        pptDst: *const POINT,
        psize: *const SIZE,
        hdcSrc: HDC,
        pptSrc: *const POINT,
        crKey: DWORD,
        pblend: *const BLENDFUNCTION,
        dwFlags: DWORD,
    ) -> BOOL;
    pub fn GetDC(hWnd: HWND) -> HDC;
    pub fn ReleaseDC(hWnd: HWND, hDC: HDC) -> i32;
    pub fn SendInput(cInputs: UINT, pInputs: *const INPUT, cbSize: i32) -> UINT;
    pub fn GetAsyncKeyState(vKey: i32) -> i16;
    pub fn GetKeyState(nVirtKey: i32) -> i16;
    pub fn GetForegroundWindow() -> HWND;
    pub fn GetWindowThreadProcessId(hWnd: HWND, lpdwProcessId: *mut DWORD) -> DWORD;
    pub fn GetGUIThreadInfo(idThread: DWORD, pgui: *mut GUITHREADINFO) -> BOOL;
    pub fn ClientToScreen(hWnd: HWND, lpPoint: *mut POINT) -> BOOL;
    pub fn GetCursorPos(lpPoint: *mut POINT) -> BOOL;
    pub fn GetWindowRect(hWnd: HWND, lpRect: *mut RECT) -> BOOL;
    pub fn GetSystemMetrics(nIndex: i32) -> i32;
    pub fn SetTimer(hWnd: HWND, nIDEvent: usize, uElapse: UINT, lpTimerFunc: *const c_void) -> usize;
    pub fn LoadIconW(hInstance: HINSTANCE, lpIconName: *const u16) -> HICON;
    pub fn LoadCursorW(hInstance: HINSTANCE, lpCursorName: *const u16) -> HCURSOR;
    pub fn CreatePopupMenu() -> HMENU;
    pub fn AppendMenuW(hMenu: HMENU, uFlags: UINT, uIDNewItem: usize, lpNewItem: *const u16) -> BOOL;
    pub fn TrackPopupMenu(
        hMenu: HMENU,
        uFlags: UINT,
        x: i32,
        y: i32,
        nReserved: i32,
        hWnd: HWND,
        prcRect: *const RECT,
    ) -> BOOL;
    pub fn DestroyMenu(hMenu: HMENU) -> BOOL;
    pub fn SetForegroundWindow(hWnd: HWND) -> BOOL;
    pub fn SetCapture(hWnd: HWND) -> HWND;
    pub fn ReleaseCapture() -> BOOL;
    pub fn SetProcessDPIAware() -> BOOL;
    pub fn PostMessageW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> BOOL;
    pub fn FindWindowW(lpClassName: *const u16, lpWindowName: *const u16) -> HWND;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateCompatibleDC(hdc: HDC) -> HDC;
    pub fn DeleteDC(hdc: HDC) -> BOOL;
    pub fn CreateDIBSection(
        hdc: HDC,
        pbmi: *const BITMAPINFO,
        usage: UINT,
        ppvBits: *mut *mut c_void,
        hSection: isize,
        offset: DWORD,
    ) -> HBITMAP;
    pub fn SelectObject(hdc: HDC, h: HGDIOBJ) -> HGDIOBJ;
    pub fn DeleteObject(ho: HGDIOBJ) -> BOOL;
    pub fn CreateFontW(
        cHeight: i32,
        cWidth: i32,
        cEscapement: i32,
        cOrientation: i32,
        cWeight: i32,
        bItalic: DWORD,
        bUnderline: DWORD,
        bStrikeOut: DWORD,
        iCharSet: DWORD,
        iOutPrecision: DWORD,
        iClipPrecision: DWORD,
        iQuality: DWORD,
        iPitchAndFamily: DWORD,
        pszFaceName: *const u16,
    ) -> HFONT;
    pub fn SetBkMode(hdc: HDC, mode: i32) -> i32;
    pub fn SetTextColor(hdc: HDC, color: DWORD) -> DWORD;
    pub fn TextOutW(hdc: HDC, x: i32, y: i32, lpString: *const u16, c: i32) -> BOOL;
    pub fn GetTextExtentPoint32W(hdc: HDC, lpString: *const u16, c: i32, psizl: *mut SIZE) -> BOOL;
    pub fn BitBlt(
        hdc: HDC,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        hdcSrc: HDC,
        x1: i32,
        y1: i32,
        rop: DWORD,
    ) -> BOOL;
}

#[link(name = "shell32")]
extern "system" {
    pub fn Shell_NotifyIconW(dwMessage: DWORD, lpData: *const NOTIFYICONDATAW) -> BOOL;
    pub fn ShellExecuteW(
        hwnd: HWND,
        lpOperation: *const u16,
        lpFile: *const u16,
        lpParameters: *const u16,
        lpDirectory: *const u16,
        nShowCmd: i32,
    ) -> HINSTANCE;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(lpModuleName: *const u16) -> HINSTANCE;
    pub fn GetProcAddress(hModule: HINSTANCE, lpProcName: *const u8) -> *const c_void;
}

pub unsafe fn SetWindowsHookExW(
    idHook: i32,
    lpfn: Option<unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT>,
    hmod: HINSTANCE,
    dwThreadId: DWORD,
) -> HHOOK {
    let u32_mod = GetModuleHandleW(to_wide("user32.dll").as_ptr());
    if u32_mod == 0 {
        return 0;
    }
    let p = GetProcAddress(u32_mod, b"SetWindowsHookExW\0".as_ptr());
    if p.is_null() {
        return 0;
    }
    let f: unsafe extern "system" fn(
        i32,
        Option<unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT>,
        HINSTANCE,
        DWORD,
    ) -> HHOOK = std::mem::transmute(p);
    f(idHook, lpfn, hmod, dwThreadId)
}

pub unsafe fn UnhookWindowsHookEx(hhk: HHOOK) -> BOOL {
    let u32_mod = GetModuleHandleW(to_wide("user32.dll").as_ptr());
    if u32_mod == 0 {
        return 0;
    }
    let p = GetProcAddress(u32_mod, b"UnhookWindowsHookEx\0".as_ptr());
    if p.is_null() {
        return 0;
    }
    let f: unsafe extern "system" fn(HHOOK) -> BOOL = std::mem::transmute(p);
    f(hhk)
}

pub unsafe fn CallNextHookEx(hhk: HHOOK, nCode: i32, wParam: WPARAM, lParam: LPARAM) -> LRESULT {
    let u32_mod = GetModuleHandleW(to_wide("user32.dll").as_ptr());
    if u32_mod == 0 {
        return 0;
    }
    let p = GetProcAddress(u32_mod, b"CallNextHookEx\0".as_ptr());
    if p.is_null() {
        return 0;
    }
    let f: unsafe extern "system" fn(HHOOK, i32, WPARAM, LPARAM) -> LRESULT = std::mem::transmute(p);
    f(hhk, nCode, wParam, lParam)
}

pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Inject an arbitrary UTF-8 string into the active Windows application via `SendInput(KEYEVENTF_UNICODE)`.
/// Supports `\x08` (Backspace) prefix for accent/punctuation replacement (e.g. `??` -> `¿`).
pub fn send_unicode_text(text: &str) {
    if text.is_empty() {
        return;
    }
    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2);
    for unit in text.encode_utf16() {
        if unit == 0x0008 {
            // Backspace VK
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                u: INPUT_UNION {
                    ki: KEYBDINPUT {
                        wVk: 0x08,
                        wScan: 0,
                        dwFlags: 0,
                        time: 0,
                        dwExtraInfo: HEYU_MAGIC_EXTRA,
                    },
                },
            });
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                u: INPUT_UNION {
                    ki: KEYBDINPUT {
                        wVk: 0x08,
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: HEYU_MAGIC_EXTRA,
                    },
                },
            });
        } else {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                u: INPUT_UNION {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: unit,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: HEYU_MAGIC_EXTRA,
                    },
                },
            });
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                u: INPUT_UNION {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: unit,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: HEYU_MAGIC_EXTRA,
                    },
                },
            });
        }
    }

    unsafe {
        SendInput(
            inputs.len() as UINT,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}

/// Locate the active text cursor (caret) on screen, with intelligent fallbacks for modern apps.
pub fn query_caret_screen_position(last_pos: POINT) -> POINT {
    unsafe {
        let fg = GetForegroundWindow();
        if fg != 0 {
            let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
            if tid != 0 {
                let mut gti = GUITHREADINFO {
                    cbSize: std::mem::size_of::<GUITHREADINFO>() as DWORD,
                    ..Default::default()
                };
                if GetGUIThreadInfo(tid, &mut gti) != 0 && gti.hwndCaret != 0 {
                    if gti.rcCaret.left != 0 || gti.rcCaret.top != 0 || gti.rcCaret.bottom != 0 {
                        let mut pt = POINT {
                            x: gti.rcCaret.left + 20,
                            y: gti.rcCaret.bottom + 28,
                        };
                        if ClientToScreen(gti.hwndCaret, &mut pt) != 0 {
                            if pt.x > 10 && pt.y > 10 {
                                return clamp_to_screen(pt);
                            }
                        }
                    }
                }
            }
        }
        // For modern Chromium / Electron / Web apps where Win32 hwndCaret is 0:
        // If `last_pos` is (0, 0) (start of a new composition), anchor well below & to the right
        // of the mouse cursor (+36px right, +54px down) so it never blocks the line being read/typed!
        if last_pos.x > 20 && last_pos.y > 20 {
            return clamp_to_screen(last_pos);
        }
        let mut cursor = POINT::default();
        if GetCursorPos(&mut cursor) != 0 {
            cursor.x += 36;
            cursor.y += 54;
            return clamp_to_screen(cursor);
        }
        POINT { x: 400, y: 400 }
    }
}

pub fn clamp_to_screen(pt: POINT) -> POINT {
    unsafe {
        let sw = GetSystemMetrics(SM_CXSCREEN).max(800);
        let sh = GetSystemMetrics(SM_CYSCREEN).max(600);
        POINT {
            x: pt.x.clamp(20, (sw - 420).max(40)),
            y: pt.y.clamp(20, (sh - 85).max(40)),
        }
    }
}

/// Like [`clamp_to_screen`] but keeps the HUD on the *monitor that contains `pt`*
/// (multi-monitor setups, including monitors left of / above the primary one).
pub fn clamp_to_monitor(pt: POINT) -> POINT {
    unsafe {
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as DWORD, ..Default::default() };
        if hmon == 0 || GetMonitorInfoW(hmon, &mut mi) == 0 {
            return clamp_to_screen(pt);
        }
        let w = mi.rcWork;
        POINT {
            x: pt.x.clamp(w.left + 4, (w.right - 580).max(w.left + 24)),
            y: pt.y.clamp(w.top + 4, (w.bottom - 160).max(w.top + 24)),
        }
    }
}

// ============================================================================
// NAMED PIPES / OVERLAPPED I/O / SECURITY (local IPC only - no network APIs)
// ============================================================================
pub type HANDLE = isize;
pub type HMONITOR = isize;
pub const INVALID_HANDLE_VALUE: HANDLE = -1;

pub const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
pub const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
pub const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
pub const PIPE_TYPE_MESSAGE: u32 = 0x0000_0004;
pub const PIPE_READMODE_MESSAGE: u32 = 0x0000_0002;
pub const PIPE_WAIT: u32 = 0x0000_0000;
pub const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x0000_0008;
pub const PIPE_UNLIMITED_INSTANCES: u32 = 255;
pub const OPEN_EXISTING: u32 = 3;
pub const SECURITY_SQOS_PRESENT: u32 = 0x0010_0000;
pub const SECURITY_IDENTIFICATION: u32 = 1 << 16;
pub const FILE_READ_DATA: u32 = 0x0001;
pub const FILE_WRITE_DATA: u32 = 0x0002;
pub const FILE_READ_ATTRIBUTES: u32 = 0x0080;
pub const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
pub const SYNCHRONIZE: u32 = 0x0010_0000;
pub const TOKEN_QUERY: u32 = 0x0008;
pub const TOKEN_USER_CLASS: u32 = 1; // TokenUser
pub const SDDL_REVISION_1: u32 = 1;
pub const WAIT_OBJECT_0: u32 = 0;
pub const WAIT_TIMEOUT: u32 = 0x102;
pub const ERROR_IO_PENDING: u32 = 997;
pub const ERROR_PIPE_BUSY: u32 = 231;
pub const ERROR_PIPE_CONNECTED: u32 = 535;
pub const ERROR_MORE_DATA: u32 = 234;
pub const ERROR_ACCESS_DENIED: u32 = 5;
pub const MONITOR_DEFAULTTONEAREST: u32 = 2;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct OVERLAPPED {
    pub Internal: usize,
    pub InternalHigh: usize,
    pub Offset: DWORD,
    pub OffsetHigh: DWORD,
    pub hEvent: HANDLE,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SECURITY_ATTRIBUTES {
    pub nLength: DWORD,
    pub lpSecurityDescriptor: *mut c_void,
    pub bInheritHandle: BOOL,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct MONITORINFO {
    pub cbSize: DWORD,
    pub rcMonitor: RECT,
    pub rcWork: RECT,
    pub dwFlags: DWORD,
}

#[link(name = "kernel32")]
extern "system" {
    pub fn CreateNamedPipeW(
        lpName: *const u16,
        dwOpenMode: DWORD,
        dwPipeMode: DWORD,
        nMaxInstances: DWORD,
        nOutBufferSize: DWORD,
        nInBufferSize: DWORD,
        nDefaultTimeOut: DWORD,
        lpSecurityAttributes: *const SECURITY_ATTRIBUTES,
    ) -> HANDLE;
    pub fn ConnectNamedPipe(hNamedPipe: HANDLE, lpOverlapped: *mut OVERLAPPED) -> BOOL;
    pub fn DisconnectNamedPipe(hNamedPipe: HANDLE) -> BOOL;
    pub fn GetNamedPipeClientProcessId(Pipe: HANDLE, ClientProcessId: *mut DWORD) -> BOOL;
    pub fn CreateFileW(
        lpFileName: *const u16,
        dwDesiredAccess: DWORD,
        dwShareMode: DWORD,
        lpSecurityAttributes: *const SECURITY_ATTRIBUTES,
        dwCreationDisposition: DWORD,
        dwFlagsAndAttributes: DWORD,
        hTemplateFile: HANDLE,
    ) -> HANDLE;
    pub fn ReadFile(
        hFile: HANDLE,
        lpBuffer: *mut c_void,
        nNumberOfBytesToRead: DWORD,
        lpNumberOfBytesRead: *mut DWORD,
        lpOverlapped: *mut OVERLAPPED,
    ) -> BOOL;
    pub fn WriteFile(
        hFile: HANDLE,
        lpBuffer: *const c_void,
        nNumberOfBytesToWrite: DWORD,
        lpNumberOfBytesWritten: *mut DWORD,
        lpOverlapped: *mut OVERLAPPED,
    ) -> BOOL;
    pub fn FlushFileBuffers(hFile: HANDLE) -> BOOL;
    pub fn CloseHandle(hObject: HANDLE) -> BOOL;
    pub fn CreateEventW(lpEventAttributes: *const SECURITY_ATTRIBUTES, bManualReset: BOOL, bInitialState: BOOL, lpName: *const u16) -> HANDLE;
    pub fn ResetEvent(hEvent: HANDLE) -> BOOL;
    pub fn WaitForSingleObject(hHandle: HANDLE, dwMilliseconds: DWORD) -> DWORD;
    pub fn GetOverlappedResult(hFile: HANDLE, lpOverlapped: *const OVERLAPPED, lpNumberOfBytesTransferred: *mut DWORD, bWait: BOOL) -> BOOL;
    pub fn CancelIoEx(hFile: HANDLE, lpOverlapped: *const OVERLAPPED) -> BOOL;
    pub fn GetLastError() -> DWORD;
    pub fn SetNamedPipeHandleState(hNamedPipe: HANDLE, lpMode: *const DWORD, lpMaxCollectionCount: *const DWORD, lpCollectDataTimeout: *const DWORD) -> BOOL;
    pub fn WaitNamedPipeW(lpNamedPipeName: *const u16, nTimeOut: DWORD) -> BOOL;
    pub fn ProcessIdToSessionId(dwProcessId: DWORD, pSessionId: *mut DWORD) -> BOOL;
    pub fn GetCurrentProcessId() -> DWORD;
    pub fn GetCurrentProcess() -> HANDLE;
    pub fn LocalFree(hMem: isize) -> isize;
}

#[link(name = "advapi32")]
extern "system" {
    pub fn OpenProcessToken(ProcessHandle: HANDLE, DesiredAccess: DWORD, TokenHandle: *mut HANDLE) -> BOOL;
    pub fn GetTokenInformation(
        TokenHandle: HANDLE,
        TokenInformationClass: u32,
        TokenInformation: *mut c_void,
        TokenInformationLength: DWORD,
        ReturnLength: *mut DWORD,
    ) -> BOOL;
    pub fn ConvertSidToStringSidW(Sid: *mut c_void, StringSid: *mut *mut u16) -> BOOL;
    pub fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        StringSecurityDescriptor: *const u16,
        StringSDRevision: DWORD,
        SecurityDescriptor: *mut *mut c_void,
        SecurityDescriptorSize: *mut DWORD,
    ) -> BOOL;
}

#[link(name = "user32")]
extern "system" {
    pub fn MonitorFromPoint(pt: POINT, dwFlags: DWORD) -> HMONITOR;
    pub fn GetMonitorInfoW(hMonitor: HMONITOR, lpmi: *mut MONITORINFO) -> BOOL;
}

/// Windows session id of the current process (0 on failure).
pub fn current_session_id() -> u32 {
    unsafe {
        let mut sid: DWORD = 0;
        if ProcessIdToSessionId(GetCurrentProcessId(), &mut sid) != 0 {
            sid
        } else {
            0
        }
    }
}

// ============================================================================
// QUICK-ADD POPUP / HOTKEYS / MENUS / PROCESS HELPERS
// ============================================================================
pub const WM_SETFOCUS: u32 = 0x0007;
pub const WM_KILLFOCUS: u32 = 0x0008;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_ACTIVATE: u32 = 0x0006;
pub const WM_SETCURSOR: u32 = 0x0020;
pub const WM_SETFONT: u32 = 0x0030;
pub const WM_CHAR: u32 = 0x0102;
pub const WM_CTLCOLOREDIT: u32 = 0x0133;
pub const WM_HOTKEY: u32 = 0x0312;
pub const WM_DPICHANGED: u32 = 0x02E0;
pub const WM_MOUSELEAVE: u32 = 0x02A3;
pub const TME_LEAVE: u32 = 0x0000_0002;
pub const HTCLIENT: isize = 1;
pub const HTCAPTION: isize = 2;

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct TRACKMOUSEEVENT {
    pub cbSize: DWORD,
    pub dwFlags: DWORD,
    pub hwndTrack: HWND,
    pub dwHoverTime: DWORD,
}

pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_TABSTOP: u32 = 0x0001_0000;
pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
pub const ES_AUTOHSCROLL: u32 = 0x0080;
pub const CS_DROPSHADOW: u32 = 0x0002_0000;
pub const GWL_EXSTYLE: i32 = -20;
pub const LWA_ALPHA: u32 = 0x2;
pub const SW_SHOW: i32 = 5;
pub const SW_SHOWNORMAL: i32 = 1;
pub const SW_RESTORE: i32 = 9;
pub const SWP_NOZORDER: u32 = 0x0004;

pub const EM_SETSEL: u32 = 0x00B1;
pub const EM_LIMITTEXT: u32 = 0x00C5;
pub const EM_SETMARGINS: u32 = 0x00D3;
pub const EC_LEFTMARGIN: usize = 0x1;
pub const EC_RIGHTMARGIN: usize = 0x2;
pub const EN_SETFOCUS: u32 = 0x0100;
pub const EN_KILLFOCUS: u32 = 0x0200;
pub const EN_CHANGE: u32 = 0x0300;

pub const VK_BACK: i32 = 0x08;
pub const VK_TAB: i32 = 0x09;
pub const VK_RETURN: i32 = 0x0D;
pub const VK_ESCAPE: i32 = 0x1B;
pub const VK_SPACE: i32 = 0x20;
pub const VK_LEFT: i32 = 0x25;
pub const VK_RIGHT: i32 = 0x27;

pub const DT_CENTER: u32 = 0x0001;
pub const DT_RIGHT: u32 = 0x0002;
pub const DT_VCENTER: u32 = 0x0004;
pub const DT_SINGLELINE: u32 = 0x0020;
pub const DT_CALCRECT: u32 = 0x0400;
pub const DT_NOPREFIX: u32 = 0x0800;
pub const DT_END_ELLIPSIS: u32 = 0x8000;
pub const PS_SOLID: i32 = 0;
pub const NULL_PEN: i32 = 8;
pub const NULL_BRUSH: i32 = 5;
pub const OPAQUE: i32 = 2;

pub const CF_UNICODETEXT: u32 = 13;

pub const HK_MOD_ALT: u32 = 0x0001;
pub const HK_MOD_CONTROL: u32 = 0x0002;
pub const HK_MOD_SHIFT: u32 = 0x0004;
pub const HK_MOD_WIN: u32 = 0x0008;
pub const HK_MOD_NOREPEAT: u32 = 0x4000;

pub const MF_GRAYED: u32 = 0x0000_0001;
pub const MF_DISABLED: u32 = 0x0000_0002;
pub const MF_POPUP: u32 = 0x0000_0010;
pub const MF_DEFAULT: u32 = 0x0000_1000;

pub const NIF_INFO: u32 = 0x0000_0010;
pub const NIIF_INFO: u32 = 0x0000_0001;
pub const NIIF_WARNING: u32 = 0x0000_0002;

pub const ASFW_ANY: u32 = 0xFFFF_FFFF;
pub const MUTEX_SYNCHRONIZE: u32 = 0x0010_0000;

pub const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
pub const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
pub const DWMWA_BORDER_COLOR: u32 = 34;
pub const DWMWCP_ROUND: u32 = 2;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct PAINTSTRUCT {
    pub hdc: HDC,
    pub fErase: BOOL,
    pub rcPaint: RECT,
    pub fRestore: BOOL,
    pub fIncUpdate: BOOL,
    pub rgbReserved: [u8; 32],
}

#[link(name = "user32")]
extern "system" {
    pub fn RegisterHotKey(hWnd: HWND, id: i32, fsModifiers: UINT, vk: UINT) -> BOOL;
    pub fn UnregisterHotKey(hWnd: HWND, id: i32) -> BOOL;
    pub fn SetFocus(hWnd: HWND) -> HWND;
    pub fn GetFocus() -> HWND;
    pub fn SendMessageW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
    pub fn SetWindowTextW(hWnd: HWND, lpString: *const u16) -> BOOL;
    pub fn GetWindowTextW(hWnd: HWND, lpString: *mut u16, nMaxCount: i32) -> i32;
    pub fn GetWindowTextLengthW(hWnd: HWND) -> i32;
    pub fn InvalidateRect(hWnd: HWND, lpRect: *const RECT, bErase: BOOL) -> BOOL;
    pub fn UpdateWindow(hWnd: HWND) -> BOOL;
    pub fn BeginPaint(hWnd: HWND, lpPaint: *mut PAINTSTRUCT) -> HDC;
    pub fn EndPaint(hWnd: HWND, lpPaint: *const PAINTSTRUCT) -> BOOL;
    pub fn GetClientRect(hWnd: HWND, lpRect: *mut RECT) -> BOOL;
    pub fn FillRect(hDC: HDC, lprc: *const RECT, hbr: HBRUSH) -> i32;
    pub fn DrawTextW(hdc: HDC, lpchText: *const u16, cchText: i32, lprc: *mut RECT, format: UINT) -> i32;
    pub fn MoveWindow(hWnd: HWND, X: i32, Y: i32, nWidth: i32, nHeight: i32, bRepaint: BOOL) -> BOOL;
    pub fn SetWindowLongPtrW(hWnd: HWND, nIndex: i32, dwNewLong: isize) -> isize;
    pub fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> isize;
    pub fn SetLayeredWindowAttributes(hwnd: HWND, crKey: DWORD, bAlpha: BYTE, dwFlags: DWORD) -> BOOL;
    pub fn KillTimer(hWnd: HWND, uIDEvent: usize) -> BOOL;
    pub fn IsWindow(hWnd: HWND) -> BOOL;
    pub fn IsIconic(hWnd: HWND) -> BOOL;
    pub fn BringWindowToTop(hWnd: HWND) -> BOOL;
    pub fn AttachThreadInput(idAttach: DWORD, idAttachTo: DWORD, fAttach: BOOL) -> BOOL;
    pub fn AllowSetForegroundWindow(dwProcessId: DWORD) -> BOOL;
    pub fn GetDpiForWindow(hwnd: HWND) -> UINT;
    pub fn SetCursor(hCursor: HCURSOR) -> HCURSOR;
    pub fn ScreenToClient(hWnd: HWND, lpPoint: *mut POINT) -> BOOL;
    pub fn TrackMouseEvent(lpEventTrack: *mut TRACKMOUSEEVENT) -> BOOL;
    pub fn OpenClipboard(hWndNewOwner: HWND) -> BOOL;
    pub fn CloseClipboard() -> BOOL;
    pub fn GetClipboardData(uFormat: UINT) -> HANDLE;
    pub fn IsClipboardFormatAvailable(format: UINT) -> BOOL;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateSolidBrush(color: DWORD) -> HBRUSH;
    pub fn CreatePen(iStyle: i32, cWidth: i32, color: DWORD) -> HGDIOBJ;
    pub fn RoundRect(hdc: HDC, left: i32, top: i32, right: i32, bottom: i32, width: i32, height: i32) -> BOOL;
    pub fn SetBkColor(hdc: HDC, color: DWORD) -> DWORD;
    pub fn GetStockObject(i: i32) -> HGDIOBJ;
    pub fn CreateCompatibleBitmap(hdc: HDC, cx: i32, cy: i32) -> HBITMAP;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn OpenMutexW(dwDesiredAccess: DWORD, bInheritHandle: BOOL, lpName: *const u16) -> HANDLE;
    pub fn OpenProcess(dwDesiredAccess: DWORD, bInheritHandle: BOOL, dwProcessId: DWORD) -> HANDLE;
    pub fn GlobalLock(hMem: HANDLE) -> *mut c_void;
    pub fn GlobalUnlock(hMem: HANDLE) -> BOOL;
    pub fn GetCurrentThreadId() -> DWORD;
}

#[link(name = "dwmapi")]
extern "system" {
    pub fn DwmSetWindowAttribute(hwnd: HWND, dwAttribute: DWORD, pvAttribute: *const c_void, cbAttribute: DWORD) -> i32;
}

/// `RGB()` macro (COLORREF = 0x00BBGGRR).
pub const fn rgb(r: u8, g: u8, b: u8) -> DWORD {
    (r as DWORD) | ((g as DWORD) << 8) | ((b as DWORD) << 16)
}

/// Read the clipboard as Unicode text (`None` if empty / not text / locked by another app).
pub fn clipboard_text() -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 || OpenClipboard(0) == 0 {
            return None;
        }
        let mut out = None;
        let h = GetClipboardData(CF_UNICODETEXT);
        if h != 0 {
            let p = GlobalLock(h) as *const u16;
            if !p.is_null() {
                let mut len = 0usize;
                while len < 1 << 20 && *p.add(len) != 0 {
                    len += 1;
                }
                out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)));
                GlobalUnlock(h);
            }
        }
        CloseClipboard();
        out
    }
}

/// Text of a window / EDIT control.
pub fn window_text(hwnd: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(hwnd);
        if n <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..got.max(0) as usize])
    }
}

/// True if a named mutex exists (another process holds the singleton).
pub fn named_mutex_exists(name: &str) -> bool {
    unsafe {
        let w = to_wide(name);
        let h = OpenMutexW(MUTEX_SYNCHRONIZE, 0, w.as_ptr());
        if h != 0 {
            CloseHandle(h);
            true
        } else {
            false
        }
    }
}

/// Wait (bounded) until process `pid` has exited. Used by "restart engine".
pub fn wait_for_process_exit(pid: u32, timeout_ms: u32) {
    unsafe {
        let h = OpenProcess(SYNCHRONIZE, 0, pid);
        if h != 0 {
            WaitForSingleObject(h, timeout_ms);
            CloseHandle(h);
        }
    }
}

/// Parse a hotkey string like `"Ctrl+Alt+N"`, `"Ctrl+Shift+F9"` into
/// `(RegisterHotKey modifiers, virtual-key)`. Returns `None` for an empty / invalid string.
pub fn parse_hotkey(spec: &str) -> Option<(u32, u32)> {
    let mut mods = 0u32;
    let mut vk = 0u32;
    for part in spec.split('+').map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let up = part.to_ascii_uppercase();
        match up.as_str() {
            "CTRL" | "CONTROL" => mods |= HK_MOD_CONTROL,
            "ALT" => mods |= HK_MOD_ALT,
            "SHIFT" => mods |= HK_MOD_SHIFT,
            "WIN" | "SUPER" | "META" => mods |= HK_MOD_WIN,
            _ => {
                if vk != 0 {
                    return None;
                }
                let b = up.as_bytes();
                vk = if b.len() == 1 && (b[0].is_ascii_uppercase() || b[0].is_ascii_digit()) {
                    b[0] as u32
                } else if b.len() >= 2 && b[0] == b'F' && up[1..].parse::<u32>().map(|n| (1..=24).contains(&n)).unwrap_or(false) {
                    0x70 + up[1..].parse::<u32>().ok()? - 1
                } else {
                    match up.as_str() {
                        "SPACE" => 0x20,
                        "INSERT" | "INS" => 0x2D,
                        "HOME" => 0x24,
                        "END" => 0x23,
                        "PAGEUP" => 0x21,
                        "PAGEDOWN" => 0x22,
                        "`" | "~" => 0xC0,
                        ";" => 0xBA,
                        "=" => 0xBB,
                        "," => 0xBC,
                        "-" => 0xBD,
                        "." => 0xBE,
                        "/" => 0xBF,
                        _ => return None,
                    }
                };
            }
        }
    }
    if vk == 0 || mods == 0 {
        return None;
    }
    Some((mods, vk))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_parsing() {
        assert_eq!(parse_hotkey("Ctrl+Alt+N"), Some((HK_MOD_CONTROL | HK_MOD_ALT, 'N' as u32)));
        assert_eq!(parse_hotkey("ctrl + shift + f9"), Some((HK_MOD_CONTROL | HK_MOD_SHIFT, 0x78)));
        assert_eq!(parse_hotkey("Win+Space"), Some((HK_MOD_WIN, 0x20)));
        assert_eq!(parse_hotkey("N"), None);
        assert_eq!(parse_hotkey(""), None);
        assert_eq!(parse_hotkey("Ctrl+A+B"), None);
        assert_eq!(parse_hotkey("Ctrl+F25"), None);
    }
}
