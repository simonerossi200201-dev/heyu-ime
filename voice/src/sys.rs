//! Hand-written Win32 FFI for heyu-voice (std only, no third-party crates).
//! Only local OS facilities are declared here: windowing, GDI, waveIn (winmm), registry,
//! CNG hashing (bcrypt). Nothing in this file (or crate) touches the network stack.
#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms, dead_code)]

use std::ffi::c_void;

pub type HANDLE = *mut c_void;
pub type HWND = *mut c_void;
pub type HHOOK = *mut c_void;
pub type HDC = *mut c_void;
pub type HMODULE = *mut c_void;
pub type HMONITOR = *mut c_void;
pub type HGDIOBJ = *mut c_void;
pub type HKEY = *mut c_void;
pub type BOOL = i32;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type WNDPROC = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SIZE {
    pub cx: i32,
    pub cy: i32,
}
#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt: POINT,
    pub lPrivate: u32,
}
#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: u32,
    pub style: u32,
    pub lpfnWndProc: Option<WNDPROC>,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HMODULE,
    pub hIcon: HANDLE,
    pub hCursor: HANDLE,
    pub hbrBackground: HANDLE,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
    pub hIconSm: HANDLE,
}
#[repr(C)]
#[derive(Default)]
pub struct GUITHREADINFO {
    pub cbSize: u32,
    pub flags: u32,
    pub hwndActive: usize,
    pub hwndFocus: usize,
    pub hwndCapture: usize,
    pub hwndMenuOwner: usize,
    pub hwndMoveSize: usize,
    pub hwndCaret: usize,
    pub rcCaret: RECT,
}
#[repr(C)]
#[derive(Default)]
pub struct MONITORINFO {
    pub cbSize: u32,
    pub rcMonitor: RECT,
    pub rcWork: RECT,
    pub dwFlags: u32,
}
#[repr(C)]
#[derive(Default)]
pub struct BITMAPINFOHEADER {
    pub biSize: u32,
    pub biWidth: i32,
    pub biHeight: i32,
    pub biPlanes: u16,
    pub biBitCount: u16,
    pub biCompression: u32,
    pub biSizeImage: u32,
    pub biXPelsPerMeter: i32,
    pub biYPelsPerMeter: i32,
    pub biClrUsed: u32,
    pub biClrImportant: u32,
}
#[repr(C)]
#[derive(Default)]
pub struct BITMAPINFO {
    pub bmiHeader: BITMAPINFOHEADER,
    pub bmiColors: [u32; 1],
}
#[repr(C)]
pub struct BLENDFUNCTION {
    pub BlendOp: u8,
    pub BlendFlags: u8,
    pub SourceConstantAlpha: u8,
    pub AlphaFormat: u8,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KEYBDINPUT {
    pub wVk: u16,
    pub wScan: u16,
    pub dwFlags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MOUSEINPUT {
    pub dx: i32,
    pub dy: i32,
    pub mouseData: u32,
    pub dwFlags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}
/// `INPUT` with the keyboard arm of the union; padded to the 40-byte x64 size (MOUSEINPUT is 32 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct INPUT {
    pub type_: u32,
    pub ki: KEYBDINPUT,
    pub _pad: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct INPUT_MOUSE_PACKET {
    pub type_: u32,
    pub _align: u32,
    pub mi: MOUSEINPUT,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KBDLLHOOKSTRUCT {
    pub vkCode: u32,
    pub scanCode: u32,
    pub flags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MSLLHOOKSTRUCT {
    pub pt: POINT,
    pub mouseData: u32,
    pub flags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}
#[repr(C)]
pub struct WAVEFORMATEX {
    pub wFormatTag: u16,
    pub nChannels: u16,
    pub nSamplesPerSec: u32,
    pub nAvgBytesPerSec: u32,
    pub nBlockAlign: u16,
    pub wBitsPerSample: u16,
    pub cbSize: u16,
}
#[repr(C)]
pub struct WAVEHDR {
    pub lpData: *mut u8,
    pub dwBufferLength: u32,
    pub dwBytesRecorded: u32,
    pub dwUser: usize,
    pub dwFlags: u32,
    pub dwLoops: u32,
    pub lpNext: *mut WAVEHDR,
    pub reserved: usize,
}
#[repr(C)]
pub struct SYSTEM_LOGICAL_PROCESSOR_INFORMATION {
    pub ProcessorMask: usize,
    pub Relationship: u32,
    pub _union: [u64; 2],
}

// ---- constants ----
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_HOTKEY: u32 = 0x0312;
pub const WM_APP: u32 = 0x8000;
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
pub const SW_HIDE: i32 = 0;
pub const SW_SHOWNOACTIVATE: i32 = 4;
pub const HWND_TOPMOST: HWND = -1isize as HWND;
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOACTIVATE: u32 = 0x0010;
pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;
pub const MOD_NOREPEAT: u32 = 0x4000;
pub const INPUT_MOUSE: u32 = 0;
pub const INPUT_KEYBOARD: u32 = 1;
pub const KEYEVENTF_KEYUP: u32 = 0x0002;
pub const KEYEVENTF_UNICODE: u32 = 0x0004;
pub const MOUSEEVENTF_MOVE: u32 = 0x0001;
pub const MOUSEEVENTF_LEFTDOWN: u32 = 0x0002;
pub const MOUSEEVENTF_LEFTUP: u32 = 0x0004;
pub const MOUSEEVENTF_VIRTUALDESK: u32 = 0x4000;
pub const MOUSEEVENTF_ABSOLUTE: u32 = 0x8000;
pub const WH_KEYBOARD_LL: i32 = 13;
pub const WH_MOUSE_LL: i32 = 14;
pub const HC_ACTION: i32 = 0;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const SM_XVIRTUALSCREEN: i32 = 76;
pub const SM_YVIRTUALSCREEN: i32 = 77;
pub const SM_CXVIRTUALSCREEN: i32 = 78;
pub const SM_CYVIRTUALSCREEN: i32 = 79;
pub const MONITOR_DEFAULTTONEAREST: u32 = 2;
pub const ULW_ALPHA: u32 = 2;
pub const AC_SRC_OVER: u8 = 0;
pub const AC_SRC_ALPHA: u8 = 1;
pub const DIB_RGB_COLORS: u32 = 0;
pub const TRANSPARENT_BK: i32 = 1;
pub const DT_SINGLELINE: u32 = 0x20;
pub const DT_NOPREFIX: u32 = 0x800;
pub const DT_VCENTER: u32 = 0x4;
pub const DT_LEFT: u32 = 0;
pub const ANTIALIASED_QUALITY: u32 = 4;
pub const CLEARTYPE_QUALITY: u32 = 5;
pub const ERROR_ALREADY_EXISTS: u32 = 183;
pub const WAVE_MAPPER: u32 = 0xFFFF_FFFF;
pub const WAVE_FORMAT_PCM: u16 = 1;
pub const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
pub const CALLBACK_EVENT: u32 = 0x0005_0000;
pub const WHDR_DONE: u32 = 0x0000_0001;
pub const MMSYSERR_NOERROR: u32 = 0;
pub const INFINITE: u32 = 0xFFFF_FFFF;
pub const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: u32 = 0x0000_0100;
pub const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: u32 = 0x0000_1000;
pub const HKEY_CURRENT_USER: HKEY = 0x8000_0001usize as HKEY;
pub const HKEY_LOCAL_MACHINE: HKEY = 0x8000_0002usize as HKEY;
pub const KEY_READ: u32 = 0x20019;
pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
pub const MDT_EFFECTIVE_DPI: i32 = 0;
pub const VK_SHIFT: i32 = 0x10;
pub const VK_CONTROL: i32 = 0x11;
pub const VK_MENU: i32 = 0x12;
pub const VK_BACK: u16 = 0x08;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_LWIN: i32 = 0x5B;
pub const VK_RWIN: i32 = 0x5C;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GUID {
    pub Data1: u32,
    pub Data2: u16,
    pub Data3: u16,
    pub Data4: [u8; 8],
}

#[repr(C)]
pub struct WAVEFORMATEXTENSIBLE {
    pub Format: WAVEFORMATEX,
    pub Samples: u16,
    pub dwChannelMask: u32,
    pub SubFormat: GUID,
}

/// Safely inspect a `WAVEFORMATEX` / `WAVEFORMATEXTENSIBLE` pointer returned by WASAPI `GetMixFormat`.
/// Note: in C (`mmreg.h`), `sizeof(WAVEFORMATEX)` is 18 bytes (2-byte aligned), so in `WAVEFORMATEXTENSIBLE`
/// `Samples` is at byte offset 18, `dwChannelMask` at 20, and `SubFormat` (`GUID`) at byte offset 24!
/// Reading `SubFormat.Data1` via `read_unaligned` at byte offset 24 avoids Rust's 20-byte `WAVEFORMATEX` tail-padding mismatch.
#[inline]
pub unsafe fn wfx_is_ieee_float(pwfx: *const WAVEFORMATEX) -> bool {
    if pwfx.is_null() {
        return false;
    }
    let wfx = &*pwfx;
    if wfx.wFormatTag == WAVE_FORMAT_IEEE_FLOAT {
        return true;
    }
    if wfx.wFormatTag == 0xFFFE && wfx.cbSize >= 22 {
        let sub_data1 = std::ptr::read_unaligned((pwfx as *const u8).add(24) as *const u32);
        return sub_data1 == 0x0000_0003; // KSDATAFORMAT_SUBTYPE_IEEE_FLOAT.Data1
    }
    false
}

pub const COINIT_MULTITHREADED: u32 = 0x0;
pub const CLSCTX_ALL: u32 = 0x17;
pub const AUDCLNT_SHAREMODE_SHARED: u32 = 0;
pub const AUDCLNT_STREAMFLAGS_LOOPBACK: u32 = 0x0002_0000;
pub const AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY: u32 = 0x0800_0000;
pub const AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM: u32 = 0x8000_0000;
pub const AUDCLNT_BUFFERFLAGS_SILENT: u32 = 0x2;
pub const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
pub const GA_ROOT: u32 = 2;

#[repr(C)]
pub struct LASTINPUTINFO {
    pub cbSize: u32,
    pub dwTime: u32,
}

#[repr(C)]
pub struct PROCESSENTRY32W {
    pub dwSize: u32,
    pub cntUsage: u32,
    pub th32ProcessID: u32,
    pub th32DefaultHeapID: usize,
    pub th32ModuleID: u32,
    pub cntThreads: u32,
    pub th32ParentProcessID: u32,
    pub pcPriClassBase: i32,
    pub dwFlags: u32,
    pub szExeFile: [u16; 260],
}

#[link(name = "ole32")]
extern "system" {
    pub fn CoInitializeEx(pvReserved: *mut c_void, dwCoInit: u32) -> i32;
    pub fn CoUninitialize();
    pub fn CoCreateInstance(rclsid: *const GUID, pUnkOuter: *mut c_void, dwClsContext: u32, riid: *const GUID, ppv: *mut *mut c_void) -> i32;
    pub fn CoTaskMemFree(pv: *mut c_void);
}

#[link(name = "kernel32")]
extern "system" {
    pub fn CreateMutexW(attr: *const c_void, initial_owner: BOOL, name: *const u16) -> HANDLE;
    pub fn GetLastError() -> u32;
    pub fn CloseHandle(h: HANDLE) -> BOOL;
    pub fn GetModuleHandleW(name: *const u16) -> HMODULE;
    pub fn LoadLibraryExW(name: *const u16, file: HANDLE, flags: u32) -> HMODULE;
    pub fn GetProcAddress(m: HMODULE, name: *const u8) -> *mut c_void;
    pub fn FreeLibrary(m: HMODULE) -> BOOL;
    pub fn SetDllDirectoryW(path: *const u16) -> BOOL;
    pub fn AddDllDirectory(path: *const u16) -> *mut c_void;
    pub fn CreateEventW(attr: *const c_void, manual: BOOL, initial: BOOL, name: *const u16) -> HANDLE;
    pub fn SetEvent(h: HANDLE) -> BOOL;
    pub fn WaitForSingleObject(h: HANDLE, ms: u32) -> u32;
    pub fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> HANDLE;
    pub fn Process32FirstW(snap: HANDLE, pe: *mut PROCESSENTRY32W) -> BOOL;
    pub fn Process32NextW(snap: HANDLE, pe: *mut PROCESSENTRY32W) -> BOOL;
    pub fn GetTickCount() -> u32;
    pub fn Sleep(ms: u32);
    pub fn GetLogicalProcessorInformation(buf: *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION, len: *mut u32) -> BOOL;
    pub fn AttachConsole(pid: u32) -> BOOL;
    pub fn GetCurrentThreadId() -> u32;
}

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassExW(wc: *const WNDCLASSEXW) -> u16;
    pub fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: HWND,
        menu: HANDLE,
        inst: HMODULE,
        param: *mut c_void,
    ) -> HWND;
    pub fn DefWindowProcW(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT;
    pub fn DestroyWindow(h: HWND) -> BOOL;
    pub fn GetMessageW(msg: *mut MSG, h: HWND, min: u32, max: u32) -> BOOL;
    pub fn TranslateMessage(msg: *const MSG) -> BOOL;
    pub fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    pub fn PostMessageW(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> BOOL;
    pub fn SendMessageW(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT;
    pub fn GetWindowTextW(h: HWND, buf: *mut u16, max: i32) -> i32;
    pub fn GetClassNameW(h: HWND, buf: *mut u16, max: i32) -> i32;
    pub fn PostQuitMessage(code: i32);
    pub fn FindWindowW(class: *const u16, title: *const u16) -> HWND;
    pub fn RegisterHotKey(h: HWND, id: i32, mods: u32, vk: u32) -> BOOL;
    pub fn UnregisterHotKey(h: HWND, id: i32) -> BOOL;
    pub fn SetTimer(h: HWND, id: usize, ms: u32, f: *const c_void) -> usize;
    pub fn KillTimer(h: HWND, id: usize) -> BOOL;
    pub fn ShowWindow(h: HWND, cmd: i32) -> BOOL;
    pub fn SetWindowPos(h: HWND, after: HWND, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> BOOL;
    pub fn UpdateLayeredWindow(
        h: HWND,
        hdc_dst: HDC,
        pt_dst: *const POINT,
        size: *const SIZE,
        hdc_src: HDC,
        pt_src: *const POINT,
        key: u32,
        blend: *const BLENDFUNCTION,
        flags: u32,
    ) -> BOOL;
    pub fn GetDC(h: HWND) -> HDC;
    pub fn ReleaseDC(h: HWND, dc: HDC) -> i32;
    pub fn GetForegroundWindow() -> HWND;
    pub fn GetWindowThreadProcessId(h: HWND, pid: *mut u32) -> u32;
    pub fn GetGUIThreadInfo(tid: u32, info: *mut GUITHREADINFO) -> BOOL;
    pub fn ClientToScreen(h: HWND, pt: *mut POINT) -> BOOL;
    pub fn MonitorFromWindow(h: HWND, flags: u32) -> HMONITOR;
    pub fn MonitorFromPoint(pt: POINT, flags: u32) -> HMONITOR;
    pub fn GetMonitorInfoW(m: HMONITOR, info: *mut MONITORINFO) -> BOOL;
    pub fn SendInput(n: u32, inputs: *const INPUT, cb: i32) -> u32;
    pub fn GetAsyncKeyState(vk: i32) -> i16;
    pub fn GetLastInputInfo(lii: *mut LASTINPUTINFO) -> BOOL;
    pub fn SetProcessDpiAwarenessContext(ctx: isize) -> BOOL;
    pub fn DrawTextW(dc: HDC, text: *const u16, n: i32, rc: *mut RECT, fmt: u32) -> i32;
    pub fn SetWindowsHookExW(id: i32, lpfn: unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT, hmod: HMODULE, tid: u32) -> HHOOK;
    pub fn CallNextHookEx(hhk: HHOOK, code: i32, w: WPARAM, l: LPARAM) -> LRESULT;
    pub fn UnhookWindowsHookEx(hhk: HHOOK) -> BOOL;
    pub fn GetCursorPos(pt: *mut POINT) -> BOOL;
    pub fn SetCursorPos(x: i32, y: i32) -> BOOL;
    pub fn WindowFromPoint(pt: POINT) -> HWND;
    pub fn GetClientRect(h: HWND, rc: *mut RECT) -> BOOL;
    pub fn GetSystemMetrics(idx: i32) -> i32;
    pub fn GetAncestor(h: HWND, flags: u32) -> HWND;
    pub fn IsWindow(h: HWND) -> BOOL;
    pub fn SetForegroundWindow(h: HWND) -> BOOL;
    pub fn AttachThreadInput(id_attach: u32, id_attach_to: u32, attach: BOOL) -> BOOL;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateCompatibleDC(dc: HDC) -> HDC;
    pub fn DeleteDC(dc: HDC) -> BOOL;
    pub fn CreateDIBSection(dc: HDC, bmi: *const BITMAPINFO, usage: u32, bits: *mut *mut c_void, sec: HANDLE, off: u32) -> HGDIOBJ;
    pub fn SelectObject(dc: HDC, obj: HGDIOBJ) -> HGDIOBJ;
    pub fn DeleteObject(obj: HGDIOBJ) -> BOOL;
    pub fn CreateFontW(
        h: i32,
        w: i32,
        esc: i32,
        orient: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike: u32,
        charset: u32,
        out_prec: u32,
        clip_prec: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> HGDIOBJ;
    pub fn SetTextColor(dc: HDC, c: u32) -> u32;
    pub fn SetBkMode(dc: HDC, mode: i32) -> i32;
    pub fn GetTextExtentPoint32W(dc: HDC, s: *const u16, n: i32, sz: *mut SIZE) -> BOOL;
}

#[link(name = "shcore")]
extern "system" {
    pub fn GetDpiForMonitor(m: HMONITOR, kind: i32, x: *mut u32, y: *mut u32) -> i32;
}

#[link(name = "winmm")]
extern "system" {
    pub fn waveInGetNumDevs() -> u32;
    pub fn waveInOpen(h: *mut HANDLE, dev: u32, fmt: *const WAVEFORMATEX, cb: usize, inst: usize, flags: u32) -> u32;
    pub fn waveInPrepareHeader(h: HANDLE, hdr: *mut WAVEHDR, cb: u32) -> u32;
    pub fn waveInUnprepareHeader(h: HANDLE, hdr: *mut WAVEHDR, cb: u32) -> u32;
    pub fn waveInAddBuffer(h: HANDLE, hdr: *mut WAVEHDR, cb: u32) -> u32;
    pub fn waveInStart(h: HANDLE) -> u32;
    pub fn waveInReset(h: HANDLE) -> u32;
    pub fn waveInClose(h: HANDLE) -> u32;
}

#[link(name = "advapi32")]
extern "system" {
    pub fn RegOpenKeyExW(key: HKEY, sub: *const u16, opts: u32, sam: u32, out: *mut HKEY) -> i32;
    pub fn RegQueryValueExW(key: HKEY, name: *const u16, res: *mut u32, ty: *mut u32, data: *mut u8, len: *mut u32) -> i32;
    pub fn RegCloseKey(key: HKEY) -> i32;
}

#[link(name = "bcrypt")]
extern "system" {
    pub fn BCryptOpenAlgorithmProvider(alg: *mut HANDLE, id: *const u16, imp: *const u16, flags: u32) -> i32;
    pub fn BCryptCloseAlgorithmProvider(alg: HANDLE, flags: u32) -> i32;
    pub fn BCryptCreateHash(alg: HANDLE, h: *mut HANDLE, obj: *mut u8, cb_obj: u32, secret: *const u8, cb_secret: u32, flags: u32) -> i32;
    pub fn BCryptHashData(h: HANDLE, data: *const u8, cb: u32, flags: u32) -> i32;
    pub fn BCryptFinishHash(h: HANDLE, out: *mut u8, cb: u32, flags: u32) -> i32;
    pub fn BCryptDestroyHash(h: HANDLE) -> i32;
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn wide_os(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Incremental SHA-256 via Windows CNG (hardware SHA extensions when available).
pub struct Sha256 {
    alg: HANDLE,
    h: HANDLE,
}

impl Sha256 {
    pub fn new() -> Option<Self> {
        unsafe {
            let mut alg = std::ptr::null_mut();
            if BCryptOpenAlgorithmProvider(&mut alg, wide("SHA256").as_ptr(), std::ptr::null(), 0) != 0 {
                return None;
            }
            let mut h = std::ptr::null_mut();
            if BCryptCreateHash(alg, &mut h, std::ptr::null_mut(), 0, std::ptr::null(), 0, 0) != 0 {
                BCryptCloseAlgorithmProvider(alg, 0);
                return None;
            }
            Some(Self { alg, h })
        }
    }
    pub fn update(&mut self, data: &[u8]) {
        for chunk in data.chunks(1 << 30) {
            unsafe {
                BCryptHashData(self.h, chunk.as_ptr(), chunk.len() as u32, 0);
            }
        }
    }
    pub fn finish_hex(self) -> String {
        let mut out = [0u8; 32];
        unsafe {
            BCryptFinishHash(self.h, out.as_mut_ptr(), 32, 0);
        }
        out.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

impl Drop for Sha256 {
    fn drop(&mut self) {
        unsafe {
            BCryptDestroyHash(self.h);
            BCryptCloseAlgorithmProvider(self.alg, 0);
        }
    }
}
