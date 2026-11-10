//! HEYU (鹤羽输入法) - Local named-pipe transport (client used by the TSF DLL, server used by heyu-ime.exe)
//! ======================================================================================================
//! * Message-mode pipe, `PIPE_REJECT_REMOTE_CLIENTS`, first instance created with
//!   `FILE_FLAG_FIRST_PIPE_INSTANCE` (no name squatting).
//! * Security descriptor: SYSTEM / Administrators / the current user get full access; AppContainer
//!   processes (ALL APPLICATION PACKAGES `S-1-15-2-1`, ALL RESTRICTED APPLICATION PACKAGES
//!   `S-1-15-2-2`) get read/write *without* `FILE_CREATE_PIPE_INSTANCE`; a Low mandatory label lets
//!   low-integrity clients write. Elevated apps run as the same user and match the user ACE.
//! * The client never blocks longer than the caller-supplied timeout (overlapped I/O + CancelIoEx)
//!   and backs off reconnect attempts when the server is not running, so typing never stalls.

use crate::ipc::{pipe_name_for_session, Request, Response, MAX_MESSAGE_SIZE};
use crate::win32::*;
use std::ffi::c_void;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Access mask the client requests (must be a subset of what the SDDL grants to AppContainers).
const CLIENT_ACCESS: u32 = FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE;
/// How long to wait before trying to reconnect after the server was found missing.
const RECONNECT_BACKOFF: Duration = Duration::from_millis(400);

/// Default pipe name of the server for the current Windows session.
pub fn default_pipe_name() -> String {
    pipe_name_for_session(current_session_id())
}

// ============================================================================
// CLIENT
// ============================================================================
pub struct PipeClient {
    name: Vec<u16>,
    handle: HANDLE,
    event: HANDLE,
    retry_after: Option<Instant>,
}

// The handles are only used by the owning thread at any one time.
unsafe impl Send for PipeClient {}

impl PipeClient {
    pub fn new(name: &str) -> Self {
        Self { name: to_wide(name), handle: INVALID_HANDLE_VALUE, event: 0, retry_after: None }
    }

    pub fn for_current_session() -> Self {
        Self::new(&default_pipe_name())
    }

    pub fn is_connected(&self) -> bool {
        self.handle != INVALID_HANDLE_VALUE
    }

    pub fn disconnect(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.handle);
            }
            self.handle = INVALID_HANDLE_VALUE;
        }
    }

    fn connect(&mut self) -> bool {
        if self.is_connected() {
            return true;
        }
        if let Some(t) = self.retry_after {
            if Instant::now() < t {
                return false;
            }
        }
        unsafe {
            if self.event == 0 {
                self.event = CreateEventW(std::ptr::null(), 1, 0, std::ptr::null());
                if self.event == 0 {
                    return false;
                }
            }
            for attempt in 0..2 {
                let h = CreateFileW(
                    self.name.as_ptr(),
                    CLIENT_ACCESS,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    0,
                );
                if h != INVALID_HANDLE_VALUE {
                    let mode: DWORD = PIPE_READMODE_MESSAGE;
                    if SetNamedPipeHandleState(h, &mode, std::ptr::null(), std::ptr::null()) == 0 {
                        CloseHandle(h);
                        break;
                    }
                    self.handle = h;
                    self.retry_after = None;
                    return true;
                }
                if GetLastError() == ERROR_PIPE_BUSY && attempt == 0 {
                    WaitNamedPipeW(self.name.as_ptr(), 10);
                    continue;
                }
                break;
            }
        }
        self.retry_after = Some(Instant::now() + RECONNECT_BACKOFF);
        false
    }

    /// Wait for an overlapped operation; cancels it (and waits for the cancellation) on timeout.
    unsafe fn finish_io(&self, started_ok: BOOL, ov: &mut OVERLAPPED, deadline: Instant) -> Option<u32> {
        let mut n: DWORD = 0;
        if started_ok == 0 && GetLastError() != ERROR_IO_PENDING {
            return None;
        }
        if started_ok == 0 {
            let now = Instant::now();
            let ms = deadline.saturating_duration_since(now).as_millis().max(1) as DWORD;
            if WaitForSingleObject(self.event, ms) != WAIT_OBJECT_0 {
                CancelIoEx(self.handle, ov);
                GetOverlappedResult(self.handle, ov, &mut n, 1);
                return None;
            }
        }
        if GetOverlappedResult(self.handle, ov, &mut n, 0) == 0 {
            return None;
        }
        Some(n)
    }

    fn transact_once(&mut self, req: &[u8], deadline: Instant) -> Option<Vec<u8>> {
        if !self.connect() {
            return None;
        }
        unsafe {
            let mut ov: OVERLAPPED = std::mem::zeroed();
            ov.hEvent = self.event;
            let ok = WriteFile(self.handle, req.as_ptr() as *const c_void, req.len() as DWORD, std::ptr::null_mut(), &mut ov);
            match self.finish_io(ok, &mut ov, deadline) {
                Some(n) if n as usize == req.len() => {}
                _ => {
                    self.disconnect();
                    return None;
                }
            }
            let mut buf = vec![0u8; MAX_MESSAGE_SIZE];
            let mut ov: OVERLAPPED = std::mem::zeroed();
            ov.hEvent = self.event;
            let ok = ReadFile(self.handle, buf.as_mut_ptr() as *mut c_void, buf.len() as DWORD, std::ptr::null_mut(), &mut ov);
            match self.finish_io(ok, &mut ov, deadline) {
                Some(n) => {
                    buf.truncate(n as usize);
                    Some(buf)
                }
                None => {
                    // Timed out or broken: drop the connection so a late reply can never be
                    // mistaken for the answer to the next request.
                    self.disconnect();
                    None
                }
            }
        }
    }

    /// Send raw bytes and wait (at most `timeout`) for the reply. Reconnects once if the
    /// previous connection went stale (e.g. server restarted).
    pub fn transact_raw(&mut self, req: &[u8], timeout: Duration) -> Option<Vec<u8>> {
        let deadline = Instant::now() + timeout;
        let was_connected = self.is_connected();
        if let Some(r) = self.transact_once(req, deadline) {
            return Some(r);
        }
        if was_connected && Instant::now() < deadline {
            self.retry_after = None;
            return self.transact_once(req, deadline);
        }
        None
    }

    /// Typed request/response. `None` = server unavailable (caller must pass the key through).
    pub fn call(&mut self, req: &Request, timeout: Duration) -> Option<Response> {
        let bytes = self.transact_raw(&req.encode(), timeout)?;
        Response::decode(&bytes).ok()
    }
}

impl Drop for PipeClient {
    fn drop(&mut self) {
        self.disconnect();
        if self.event != 0 {
            unsafe {
                CloseHandle(self.event);
            }
            self.event = 0;
        }
    }
}

// ============================================================================
// SERVER
// ============================================================================
pub type PipeHandler = Arc<dyn Fn(&[u8], u32) -> Vec<u8> + Send + Sync>;

struct SecurityDescriptor(*mut c_void);
unsafe impl Send for SecurityDescriptor {}
impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                LocalFree(self.0 as isize);
            }
        }
    }
}

/// String SID of the user running this process (e.g. `S-1-5-21-...`).
pub fn current_user_sid_string() -> Option<String> {
    unsafe {
        let mut token: HANDLE = 0;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut buf = vec![0u8; 256];
        let mut len: DWORD = 0;
        let ok = GetTokenInformation(token, TOKEN_USER_CLASS, buf.as_mut_ptr() as *mut c_void, buf.len() as DWORD, &mut len);
        CloseHandle(token);
        if ok == 0 {
            return None;
        }
        // TOKEN_USER { SID_AND_ATTRIBUTES { PSID Sid; DWORD Attributes } }
        let sid = *(buf.as_ptr() as *const *mut c_void);
        let mut out: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut out) == 0 || out.is_null() {
            return None;
        }
        let mut n = 0;
        while *out.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(out, n));
        LocalFree(out as isize);
        Some(s)
    }
}

/// SDDL used for the server pipe (see module docs).
pub fn pipe_sddl() -> String {
    let user = current_user_sid_string().unwrap_or_else(|| "IU".to_string());
    // 0x0012019B = READ_CONTROL | SYNCHRONIZE | FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_EA |
    //              FILE_WRITE_EA | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES  (no CREATE_PIPE_INSTANCE)
    format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{user})(A;;0x0012019B;;;AC)(A;;0x0012019B;;;S-1-15-2-2)S:(ML;;NW;;;LW)"
    )
}

fn build_security_descriptor() -> SecurityDescriptor {
    let sddl = to_wide(&pipe_sddl());
    let mut psd: *mut c_void = std::ptr::null_mut();
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut psd, std::ptr::null_mut()) == 0 {
            psd = std::ptr::null_mut();
        }
    }
    SecurityDescriptor(psd)
}

fn create_instance(name: &[u16], sd: &SecurityDescriptor, first: bool) -> HANDLE {
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as DWORD,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let open_mode = PIPE_ACCESS_DUPLEX | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
    unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            MAX_MESSAGE_SIZE as DWORD,
            MAX_MESSAGE_SIZE as DWORD,
            0,
            if sd.0.is_null() { std::ptr::null() } else { &sa },
        )
    }
}

fn serve_client(h: HANDLE, handler: PipeHandler) {
    let mut client_pid: DWORD = 0;
    unsafe {
        GetNamedPipeClientProcessId(h, &mut client_pid);
    }
    let mut buf = vec![0u8; MAX_MESSAGE_SIZE];
    loop {
        let mut n: DWORD = 0;
        let ok = unsafe { ReadFile(h, buf.as_mut_ptr() as *mut c_void, buf.len() as DWORD, &mut n, std::ptr::null_mut()) };
        if ok == 0 {
            break; // client gone, or oversized message (ERROR_MORE_DATA) -> drop connection
        }
        let req = &buf[..n as usize];
        let resp = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(req, client_pid)))
            .unwrap_or_else(|_| Response::unavailable().encode());
        let mut w: DWORD = 0;
        let ok = unsafe { WriteFile(h, resp.as_ptr() as *const c_void, resp.len() as DWORD, &mut w, std::ptr::null_mut()) };
        if ok == 0 {
            break;
        }
    }
    unsafe {
        DisconnectNamedPipe(h);
        CloseHandle(h);
    }
}

/// Start the pipe server on background threads (one listener + one thread per connected client).
/// `handler(request_bytes, client_pid) -> response_bytes` is called on the client's thread.
/// Fails if the pipe name is already owned by another process.
pub fn start_pipe_server(name: &str, handler: PipeHandler) -> std::io::Result<()> {
    let wname = to_wide(name);
    let sd = build_security_descriptor();
    let first = create_instance(&wname, &sd, true);
    if first == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32));
    }
    std::thread::Builder::new().name("heyu-pipe-listener".into()).spawn(move || {
        let sd = sd; // keep the security descriptor alive for the listener's lifetime
        let mut pending = first;
        loop {
            let ok = unsafe { ConnectNamedPipe(pending, std::ptr::null_mut()) };
            let connected = ok != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
            if connected {
                let h = pending;
                let hd = handler.clone();
                let spawned = std::thread::Builder::new().name("heyu-pipe-client".into()).spawn(move || serve_client(h, hd));
                if spawned.is_err() {
                    unsafe {
                        DisconnectNamedPipe(h);
                        CloseHandle(h);
                    }
                }
            } else {
                unsafe {
                    CloseHandle(pending);
                }
            }
            loop {
                pending = create_instance(&wname, &sd, false);
                if pending != INVALID_HANDLE_VALUE {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    })?;
    Ok(())
}
