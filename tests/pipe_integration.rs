//! End-to-end tests of the TSF <-> server IPC over the *real* named pipe.
//!
//! 1. `in_process_pipe_server_*`: starts `heyu_core::pipe::start_pipe_server` inside the test
//!    process, driving a real `HeyuEngine` through the shared `ImeSession` logic, and talks to it
//!    with the very same `PipeClient` the TSF DLL uses.
//! 2. `heyu_ime_exe_*`: launches the built `heyu-ime.exe --pipe-name <unique> --data-dir <tmp>`
//!    (real server: UI thread, HUD windows, pipe-thread -> UI-thread bridge) and types over the pipe.
//!
//! All tests use a private copy of `data/` so the user's config.json is never modified.

use heyu_core::engine::HeyuEngine;
use heyu_core::ipc::{Request, RequestKind, Response, MOD_LSHIFT, MOD_SHIFT};
use heyu_core::pipe::{start_pipe_server, PipeClient};
use heyu_core::session::{ImeSession, VK_LSHIFT};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const T: Duration = Duration::from_millis(50); // same budget as the DLL

fn private_data_copy(tag: &str) -> PathBuf {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
    let dst = std::env::temp_dir().join(format!("heyu_it_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dst);
    std::fs::create_dir_all(&dst).unwrap();
    for e in std::fs::read_dir(&src).unwrap() {
        let p = e.unwrap().path();
        if p.is_file() {
            std::fs::copy(&p, dst.join(p.file_name().unwrap())).unwrap();
        }
    }
    // Deterministic start: Chinese mode, 4-code auto-commit on.
    std::fs::write(
        dst.join("config.json"),
        "{\n  \"theme_index\": 0,\n  \"show_shape_hints\": true,\n  \"auto_commit_4code\": true,\n  \"auto_clear_empty_4code\": true,\n  \"enable_ghost_tab\": true,\n  \"show_status_pill\": true,\n  \"pill_x\": -1,\n  \"pill_y\": -1,\n  \"default_mode\": \"ZH\"\n}\n",
    )
    .unwrap();
    dst
}

fn unique_pipe(tag: &str) -> String {
    format!(r"\\.\pipe\heyu-ime-test-{}-{}-{}", tag, std::process::id(), Instant::now().elapsed().as_nanos())
}

fn key(c: &mut PipeClient, ch: char) -> Response {
    let vk = ch.to_ascii_uppercase() as u16;
    let r = c.call(&Request::key(vk, ch, 0, false), T).expect("server must answer within 50 ms");
    // matching key-up, like TSF would deliver
    c.call(&Request::key(vk, ch, 0, true), T).expect("key-up answered");
    r
}

fn vk(c: &mut PipeClient, vk: u16, mods: u16, up: bool) -> Response {
    let mut req = Request::key(vk, '\0', mods, up);
    req.ch = 0;
    c.call(&req, T).expect("server must answer within 50 ms")
}

/// The scripted typing session shared by the in-process and exe tests.
fn run_typing_script(c: &mut PipeClient) {
    // Ping / focus
    let p = c.call(&Request::new(RequestKind::Ping), T).expect("ping");
    assert!(!p.unavailable && !p.ascii_mode, "server starts in Chinese mode: {:?}", p);
    c.call(&Request::new(RequestKind::FocusIn), T).expect("focus");

    // 'hedn' -> 鹤 (4-code unique auto-commit), preedit shows the code while typing
    let r = key(c, 'h');
    assert!(r.eaten);
    assert_eq!(r.preedit_text, "h");
    assert_eq!(r.preedit_caret, 1);
    assert_eq!(key(c, 'e').preedit_text, "he");
    let r = key(c, 'd');
    assert_eq!(r.preedit_text, "hed");
    assert!(r.hud_visible);
    let r = key(c, 'n');
    assert!(r.eaten);
    assert_eq!(r.commit_text, "鹤");
    assert_eq!(r.preedit_text, "");

    // 'w' + Space -> 我
    let r = key(c, 'w');
    assert!(r.eaten && r.commit_text.is_empty());
    assert_eq!(r.preedit_text, "w");
    let r = vk(c, 0x20, 0, false);
    assert!(r.eaten);
    assert_eq!(r.commit_text, "我");
    assert_eq!(r.preedit_text, "");

    // Space with empty buffer passes through (after clearing post-commit ghosts with Esc)
    vk(c, 0x1B, 0, false);
    let r = vk(c, 0x20, 0, false);
    assert!(!r.eaten, "space with no composition must reach the app");

    // Backspace edits the composition, Esc cancels it
    key(c, 'v');
    assert_eq!(key(c, 's').preedit_text, "vs");
    let r = vk(c, 0x08, 0, false);
    assert!(r.eaten);
    assert_eq!(r.preedit_text, "v");
    let r = vk(c, 0x1B, 0, false);
    assert!(r.eaten);
    assert_eq!(r.preedit_text, "");

    // Left-Shift tap -> direct mode: letters pass through untouched
    let r = vk(c, VK_LSHIFT, MOD_SHIFT | MOD_LSHIFT, false);
    assert!(!r.eaten, "shift itself is never eaten");
    let r = vk(c, VK_LSHIFT, 0, true);
    assert!(r.ascii_mode, "left-shift tap toggles to direct mode");
    let r = key(c, 'a');
    assert!(!r.eaten && r.preedit_text.is_empty());

    // Shift held while typing a letter is NOT a tap (no toggle back)
    vk(c, VK_LSHIFT, MOD_SHIFT | MOD_LSHIFT, false);
    vk(c, 0x41, MOD_SHIFT | MOD_LSHIFT, false);
    vk(c, 0x41, MOD_SHIFT | MOD_LSHIFT, true);
    let r = vk(c, VK_LSHIFT, 0, true);
    assert!(r.ascii_mode, "shift+A must not toggle");

    // Tap again -> back to Chinese
    vk(c, VK_LSHIFT, MOD_SHIFT | MOD_LSHIFT, false);
    let r = vk(c, VK_LSHIFT, 0, true);
    assert!(!r.ascii_mode);
    assert!(key(c, 'w').eaten);

    // Ctrl shortcut while composing: composition cancelled, key passes through
    let r = vk(c, 0x43, heyu_core::ipc::MOD_CTRL, false);
    assert!(!r.eaten);
    assert_eq!(r.preedit_text, "");

    // Reset (focus lost) clears everything
    key(c, 'h');
    let r = c.call(&Request::new(RequestKind::Reset), T).unwrap();
    assert!(r.preedit_text.is_empty() && !r.hud_visible);

    // SetAsciiMode explicit
    let mut req = Request::new(RequestKind::SetAsciiMode);
    req.arg = 1;
    assert!(c.call(&req, T).unwrap().ascii_mode);
    req.arg = 0;
    assert!(!c.call(&req, T).unwrap().ascii_mode);
}

fn start_in_process_server(tag: &str, delay: Duration) -> (String, PathBuf) {
    let data = private_data_copy(tag);
    let engine = HeyuEngine::new(&data);
    assert!(engine.flypy_map.len() > 60000);
    let shared = Arc::new(Mutex::new((engine, ImeSession::new())));
    let name = unique_pipe(tag);
    start_pipe_server(
        &name,
        Arc::new(move |bytes: &[u8], _pid: u32| {
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            let req = match Request::decode(bytes) {
                Ok(r) => r,
                Err(_) => return Response::unavailable().encode(),
            };
            let mut g = shared.lock().unwrap();
            let (engine, session) = &mut *g;
            session.handle(engine, &req).response.encode()
        }),
    )
    .expect("pipe server starts");
    (name, data)
}

#[test]
fn in_process_pipe_server_types_chinese() {
    let (name, data) = start_in_process_server("inproc", Duration::ZERO);
    let mut c = PipeClient::new(&name);
    run_typing_script(&mut c);

    // Latency: 200 round trips, report worst case
    let mut worst = Duration::ZERO;
    for _ in 0..200 {
        let t0 = Instant::now();
        c.call(&Request::new(RequestKind::Ping), T).unwrap();
        worst = worst.max(t0.elapsed());
    }
    println!("in-process pipe: worst ping round trip over 200 calls = {:?}", worst);
    assert!(worst < T);

    // A second, concurrent client (another app) is served too
    let mut c2 = PipeClient::new(&name);
    assert!(c2.call(&Request::new(RequestKind::Ping), T).is_some());
    let _ = std::fs::remove_dir_all(data);
}

#[test]
fn pipe_name_cannot_be_squatted() {
    let (name, data) = start_in_process_server("squat", Duration::ZERO);
    let second = start_pipe_server(&name, Arc::new(|_: &[u8], _| Vec::new()));
    assert!(second.is_err(), "FILE_FLAG_FIRST_PIPE_INSTANCE must reject a second server");
    let _ = std::fs::remove_dir_all(data);
}

#[test]
fn client_never_blocks_when_server_missing_or_slow() {
    // Missing server: fails fast, then backs off.
    let mut c = PipeClient::new(&unique_pipe("missing"));
    let t0 = Instant::now();
    assert!(c.call(&Request::new(RequestKind::Ping), T).is_none());
    assert!(c.call(&Request::new(RequestKind::Ping), T).is_none());
    assert!(t0.elapsed() < Duration::from_millis(50), "missing server must not stall typing: {:?}", t0.elapsed());

    // Slow server (200 ms per request): client gives up at ~50 ms and passes the key through.
    let (name, data) = start_in_process_server("slow", Duration::from_millis(200));
    let mut c = PipeClient::new(&name);
    let t0 = Instant::now();
    assert!(c.call(&Request::new(RequestKind::Ping), T).is_none());
    let took = t0.elapsed();
    println!("slow server: client returned after {:?}", took);
    assert!(took < Duration::from_millis(120), "timeout not honoured: {:?}", took);
    // With a generous timeout the (reconnected) client works again.
    std::thread::sleep(Duration::from_millis(450));
    assert!(c.call(&Request::new(RequestKind::Ping), Duration::from_millis(1000)).is_some());
    let _ = std::fs::remove_dir_all(data);
}

struct KillOnDrop(std::process::Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_server(name: &str, max: Duration) -> PipeClient {
    let t0 = Instant::now();
    loop {
        let mut c = PipeClient::new(name);
        if c.call(&Request::new(RequestKind::Ping), Duration::from_millis(500)).map(|r| !r.unavailable).unwrap_or(false) {
            return c;
        }
        assert!(t0.elapsed() < max, "heyu-ime.exe did not open {} within {:?}", name, max);
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn heyu_ime_exe_serves_real_pipe() {
    let exe = Path::new(env!("CARGO_BIN_EXE_heyu-ime"));
    let data = private_data_copy("exe");
    let name = unique_pipe("exe");
    let child = std::process::Command::new(exe)
        .arg("--pipe-name")
        .arg(&name)
        .arg("--data-dir")
        .arg(&data)
        .spawn()
        .expect("spawn heyu-ime.exe");
    let _guard = KillOnDrop(child);
    let mut c = wait_for_server(&name, Duration::from_secs(30));
    run_typing_script(&mut c);

    // Caret updates are accepted and positioned (HUD drawn by the real server UI thread)
    let mut req = Request::new(RequestKind::Caret);
    req.caret = Some(heyu_core::ipc::CaretRect { left: 200, top: 200, right: 202, bottom: 220 });
    assert!(c.call(&req, T).is_some());
    drop(c);
    std::thread::sleep(Duration::from_millis(100));
    let _ = std::fs::remove_dir_all(data);
}
