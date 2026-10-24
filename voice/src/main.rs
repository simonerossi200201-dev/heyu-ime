//! HEYU (鹤羽音形) · heyu-voice.exe — fully offline voice input.
//!
//! * Resident, windowless (no console), single instance (`Local\HEYU_VOICE_SINGLETON`).
//! * Global hotkey (default Ctrl+Alt+V) toggles recording; Esc cancels; energy-VAD auto-stop.
//! * Microphone audio is captured with winmm waveIn into RAM only, recognised locally by
//!   whisper.cpp (OpenAI Whisper weights), and typed into the focused app with SendInput.
//! * No network API is imported by this binary (verified by tools/security_audit.py), and
//!   install.ps1 adds outbound firewall block rules for it and for the whisper runtime.
//!
//! Debug / test entry points (no microphone needed):
//!   heyu-voice.exe --transcribe-file x.wav [--model NAME] [--lang zh|en|it|es|auto] [--cpu]
//!                  [--cli] [--repeat N] [--type] [--delay-ms N]
//!   heyu-voice.exe --fake-mic x.wav        (resident mode; the WAV replaces the microphone)
//!   heyu-voice.exe --quit                  (ask the running instance to exit)
#![windows_subsystem = "windows"]

mod asr;
mod audio;
mod config;
mod corrector;
mod inject;
mod pill;
mod recorder;
mod sys;
mod uia;

use config::{Dirs, VoiceConfig};
use pill::{Pill, PillState, VoiceCursorMark};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Instant, SystemTime};
use sys::*;

const MSG_CLASS: &str = "HEYU_VOICE_MSG";
const MUTEX_NAME: &str = "Local\\HEYU_VOICE_SINGLETON";
const WM_ASR_RESULT: u32 = WM_APP + 1;
const WM_VOICE_TOGGLE_MIC: u32 = WM_APP + 10;
const WM_VOICE_TOGGLE_SYSTEM: u32 = WM_APP + 11;
const WM_VOICE_STOP: u32 = WM_APP + 12;
const WM_VOICE_START_MIC: u32 = WM_APP + 13;
const WM_VOICE_START_SYSTEM: u32 = WM_APP + 14;
const WM_VOICE_TOGGLE_BOTH: u32 = WM_APP + 15;
const WM_VOICE_START_BOTH: u32 = WM_APP + 16;
const WM_APP_TSF_CARET: u32 = WM_APP + 20;
const WM_APP_ANCHOR_VOICE_CURSOR: u32 = WM_APP + 21;
const WM_APP_USER_MOUSE_UP: u32 = WM_APP + 22;
const WM_APP_USER_NEWLINE_KEY: u32 = WM_APP + 23;
const WM_APP_USER_MOUSE_DOWN: u32 = WM_APP + 24;
const HK_MAIN: i32 = 1;
const HK_ESC: i32 = 2;
const HK_LOOPBACK: i32 = 3;
const HK_PIN_CURSOR: i32 = 4;
const HK_UNDO_VOICE: i32 = 5;
const TIMER_TICK: usize = 1;
const TIMER_CFG: usize = 2;

static VOICE_HWND_ATOMIC: AtomicUsize = AtomicUsize::new(0);
static SWALLOW_NEXT_LBUTTONUP: AtomicBool = AtomicBool::new(false);
static INJECTING_VOICE: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------------------------
// ASR worker thread
// ---------------------------------------------------------------------------------------------

enum Job {
    Load { runtime: PathBuf, model: String, gpu: bool },
    Transcribe {
        session: u64,
        final_chunk: bool,
        pcm: Vec<i16>,
        lang: String,
        prompt: Option<String>,
        smart_correction: bool,
        simplified_chinese: bool,
        session_ctx: corrector::SessionContext,
        runtime: PathBuf,
        model: String,
        gpu: bool,
    },
    Unload,
}

struct AsrResult {
    session: u64,
    final_chunk: bool,
    text: Result<String, String>,
    lang: String,
    ms: u128,
}

fn asr_worker(rx: Receiver<Job>, hwnd: usize, verbose: bool) {
    let mut engine: Option<asr::Whisper> = None;
    let mut key: Option<(PathBuf, String, bool)> = None;
    let mut ffi_broken = false;
    let threads = asr::physical_cores();
    let ensure = |engine: &mut Option<asr::Whisper>, key: &mut Option<(PathBuf, String, bool)>, ffi_broken: &mut bool, runtime: &Path, model: &str, gpu: bool| -> Result<(), String> {
        let k = (runtime.to_path_buf(), model.to_string(), gpu);
        if engine.is_some() && key.as_ref() == Some(&k) {
            return Ok(());
        }
        *engine = None;
        *key = Some(k);
        if *ffi_broken || !runtime.join("whisper.dll").is_file() {
            return Err("FFI_UNAVAILABLE".into());
        }
        match asr::Whisper::load(runtime, model, gpu, verbose) {
            Ok(mut w) => {
                // warm-up: allocates compute buffers / CUDA graphs so the first real utterance is fast
                let _ = w.transcribe(&vec![0.0f32; 16000], "en", None, threads);
                *engine = Some(w);
                Ok(())
            }
            Err(e) => {
                if e == "LAYOUT_MISMATCH" || e.starts_with("whisper.dll 缺少") || e.starts_with("无法加载") {
                    *ffi_broken = true;
                }
                Err(e)
            }
        }
    };
    while let Ok(job) = rx.recv() {
        match job {
            Job::Load { runtime, model, gpu } => {
                let _ = ensure(&mut engine, &mut key, &mut ffi_broken, &runtime, &model, gpu);
            }
            Job::Unload => {
                engine = None;
                key = None;
            }
            Job::Transcribe {
                session,
                final_chunk,
                mut pcm,
                lang,
                prompt,
                smart_correction,
                simplified_chinese,
                session_ctx,
                runtime,
                model,
                gpu,
            } => {
                let t0 = Instant::now();
                let text = match ensure(&mut engine, &mut key, &mut ffi_broken, &runtime, &model, gpu) {
                    Ok(()) => {
                        let f = asr::pcm_to_f32(&pcm);
                        engine.as_mut().unwrap().transcribe(&f, &lang, prompt.as_deref(), threads)
                    }
                    Err(e) if e.starts_with("MODEL_MISSING") || e.contains("SHA-256") || e.contains("大小不符") => Err(e),
                    Err(_) => asr::transcribe_cli(&runtime, &model, &pcm, &lang, prompt.as_deref(), threads, gpu),
                };
                pcm.iter_mut().for_each(|s| *s = 0);
                let text = text.map(|t| {
                    asr::postprocess_full(
                        &t,
                        &lang,
                        prompt.as_deref(),
                        smart_correction,
                        simplified_chinese,
                        final_chunk,
                        Some(&session_ctx),
                    )
                });
                let res = Box::new(AsrResult { session, final_chunk, text, lang, ms: t0.elapsed().as_millis() });
                unsafe {
                    let raw_ptr = Box::into_raw(res);
                    if PostMessageW(hwnd as HWND, WM_ASR_RESULT, 0, raw_ptr as LPARAM) == 0 {
                        // window gone; reclaim Box to prevent memory leak
                        drop(Box::from_raw(raw_ptr));
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Resident app
// ---------------------------------------------------------------------------------------------

#[derive(PartialEq, Clone, Copy)]
enum Phase {
    Idle,
    Recording,
    Recognizing,
}

struct App {
    hwnd: HWND,
    dirs: Dirs,
    cfg: VoiceConfig,
    cfg_mtime: Option<SystemTime>,
    pill: Pill,
    phase: Phase,
    session_id: u64,
    stream_seq: u64,
    pending_chunks: u32,
    committed_chunks: u32,
    last_typed_units: usize,
    last_committed_char: Option<char>,
    last_cmd_seq: u64,
    last_text: String,
    active_source: String,
    target_root_hwnd: HWND,
    target_title: String,
    target_selected_at: Option<Instant>,
    target_locked: bool,
    target_win_rect: Option<RECT>,
    mouse_down_pt: Option<(i32, i32)>,
    mouse_down_win_rect: Option<RECT>,
    user_has_selection: bool,
    last_user_key_at: Option<Instant>,
    pending_texts: std::collections::VecDeque<String>,
    cursor_mark: VoiceCursorMark,
    uia: uia::UiaVoiceAnchor,
    edit_hwnd: HWND,
    voice_char_offset: Option<u32>,
    luciole_seq: u64,
    voice_cursor_pt: Option<(i32, i32)>,
    voice_cursor_ready: bool,
    voice_line_h: i32,
    user_cursor_pt: Option<(i32, i32)>,
    user_moved_away: bool,
    last_tsf_caret: Option<(i32, i32, i32, Instant)>,
    mark_visible_until: Option<Instant>,
    last_status_write: Instant,
    cached_hotwords: Vec<String>,
    cached_replace_rules: String,
    last_active_at: Instant,
    model_unloaded_for_idle: bool,
    session_ctx: corrector::SessionContext,
    capture: Option<audio::Capture>,
    vad: audio::Vad,
    rec_start: Instant,
    chunk_start: Instant,
    rec_lang: String,
    tx: Sender<Job>,
    hotkey_registered: bool,
    loopback_registered: bool,
    esc_registered: bool,
    message_until: Option<Instant>,
    fake_mic: Option<PathBuf>,
    gpu_forced_off: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn lang_label(l: &str) -> &'static str {
    match l {
        "zh" => "中文",
        "en" => "English",
        "it" => "Italiano",
        "es" => "Español",
        _ => "自动识别",
    }
}

/// "Ctrl+Alt+V" → (MOD_*, VK)
fn parse_hotkey(s: &str) -> Option<(u32, u32)> {
    let mut mods = 0u32;
    let mut vk = 0u32;
    for part in s.split('+').map(|p| p.trim().to_ascii_uppercase()) {
        match part.as_str() {
            "CTRL" | "CONTROL" => mods |= MOD_CONTROL,
            "ALT" => mods |= MOD_ALT,
            "SHIFT" => mods |= MOD_SHIFT,
            "WIN" | "WINDOWS" | "META" => mods |= MOD_WIN,
            "SPACE" => vk = 0x20,
            "ENTER" | "RETURN" => vk = 0x0D,
            "TAB" => vk = 0x09,
            "INSERT" | "INS" => vk = 0x2D,
            "PAUSE" => vk = 0x13,
            "CAPSLOCK" => vk = 0x14,
            "SCROLLLOCK" => vk = 0x91,
            "`" | "BACKQUOTE" | "GRAVE" => vk = 0xC0,
            p if p.len() == 1 && p.chars().all(|c| c.is_ascii_alphanumeric()) => vk = p.as_bytes()[0] as u32,
            p if p.starts_with('F') && p[1..].parse::<u32>().map(|n| (1..=24).contains(&n)).unwrap_or(false) => {
                vk = 0x70 + p[1..].parse::<u32>().unwrap() - 1
            }
            _ => return None,
        }
    }
    if vk == 0 {
        None
    } else {
        Some((mods, vk))
    }
}

impl App {
    fn use_gpu(&self, runtime: &Path) -> bool {
        if self.gpu_forced_off {
            return false;
        }
        match self.cfg.use_gpu.as_str() {
            "true" => true,
            "false" => false,
            _ => asr::runtime_has_cuda(runtime),
        }
    }

    /// Hardware-adaptive model selection: on CPU-only laptops (`!use_gpu`), if the user selected
    /// the heavy 32-decoder-layer `large-v3-q5_0` and `large-v3-turbo-q5_0` (4-decoder-layer turbo)
    /// is installed locally, automatically use `large-v3-turbo-q5_0` for 3.5x faster real-time response.
    fn effective_model(&self, runtime: &Path) -> String {
        if !self.use_gpu(runtime) && self.cfg.model == "large-v3-q5_0" {
            if asr::model_path("large-v3-turbo-q5_0").is_file() {
                return "large-v3-turbo-q5_0".to_string();
            }
        }
        self.cfg.model.clone()
    }

    fn register_hotkey(&mut self) {
        unsafe {
            if self.hotkey_registered {
                UnregisterHotKey(self.hwnd, HK_MAIN);
                self.hotkey_registered = false;
            }
            if self.loopback_registered {
                UnregisterHotKey(self.hwnd, HK_LOOPBACK);
                self.loopback_registered = false;
            }
            if !self.cfg.enabled {
                return;
            }
            match parse_hotkey(&self.cfg.hotkey) {
                Some((m, vk)) => {
                    if RegisterHotKey(self.hwnd, HK_MAIN, m | MOD_NOREPEAT, vk) != 0 {
                        self.hotkey_registered = true;
                    } else {
                        self.message(format!("快捷键 {} 已被其他程序占用 · 请在设置-语音输入中更换", self.cfg.hotkey), true, 4000);
                    }
                }
                None => self.message(format!("无法识别的快捷键：{}", self.cfg.hotkey), true, 4000),
            }
            if let Some((m, vk)) = parse_hotkey(&self.cfg.loopback_hotkey) {
                if RegisterHotKey(self.hwnd, HK_LOOPBACK, m | MOD_NOREPEAT, vk) != 0 {
                    self.loopback_registered = true;
                }
            }
            let _ = UnregisterHotKey(self.hwnd, HK_PIN_CURSOR);
            let _ = RegisterHotKey(self.hwnd, HK_PIN_CURSOR, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'G' as u32);
            let _ = UnregisterHotKey(self.hwnd, HK_UNDO_VOICE);
            let _ = RegisterHotKey(self.hwnd, HK_UNDO_VOICE, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'Z' as u32);
        }
    }

    fn next_luciole_seq(&mut self) -> u64 {
        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.luciole_seq = (self.luciole_seq + 1).max(now_ms);
        self.luciole_seq
    }

    fn query_real_caret_pt(&self) -> Option<(i32, i32, i32)> {
        if let Some((cx, cy, lh, ts)) = self.last_tsf_caret {
            if ts.elapsed().as_secs() < 3 {
                return Some((cx, cy, lh));
            }
        }
        if let Some(pt) = self.uia.query_caret_screen_point() {
            return Some(pt);
        }
        inject::query_gui_caret_screen_point()
    }

    fn on_undo_voice_hotkey(&mut self) {
        if inject::is_luciole_hwnd(self.target_root_hwnd) {
            let seq = self.next_luciole_seq();
            let mode = self.cfg.cursor_mode.clone();
            if inject::send_luciole_undo(seq, &mode) {
                self.last_typed_units = 0;
                self.message("↩ 已撤回上一句语音输入".into(), false, 1600);
                return;
            }
        }
        if self.last_typed_units > 0 {
            inject::wait_modifiers_released(600);
            INJECTING_VOICE.store(true, Ordering::SeqCst);
            let _ = inject::send_backspaces(self.last_typed_units);
            INJECTING_VOICE.store(false, Ordering::SeqCst);
            self.last_typed_units = 0;
            self.message("↩ 已撤回上一句语音输入".into(), false, 1600);
        }
    }

    fn set_esc(&mut self, on: bool) {
        unsafe {
            if on && !self.esc_registered {
                // In continuous_mode, keep bare Esc 100% free for simultaneous typing/IME/editor use;
                // bind Shift+Esc as emergency cancel instead!
                let mods = if self.cfg.continuous_mode { MOD_SHIFT | MOD_NOREPEAT } else { MOD_NOREPEAT };
                self.esc_registered = RegisterHotKey(self.hwnd, HK_ESC, mods, VK_ESCAPE) != 0;
            } else if !on && self.esc_registered {
                UnregisterHotKey(self.hwnd, HK_ESC);
                self.esc_registered = false;
            }
        }
    }

    fn start_tick(&self) {
        unsafe {
            SetTimer(self.hwnd, TIMER_TICK, 33, std::ptr::null());
        }
    }

    fn message(&mut self, text: String, warn: bool, ms: u64) {
        if !self.pill.visible {
            self.pill.place_near_caret(self.dirs.theme_index());
        }
        self.pill.show(PillState::Message { text, warn });
        self.message_until = Some(Instant::now() + std::time::Duration::from_millis(ms));
        self.start_tick();
    }

    fn preload(&self) {
        if !self.cfg.enabled {
            return;
        }
        if let Some(rt) = self.dirs.runtime_dir() {
            let model = self.effective_model(&rt);
            if asr::model_path(&model).is_file() {
                let _ = self.tx.send(Job::Load { gpu: self.use_gpu(&rt), runtime: rt, model });
            }
        }
    }

    fn resolve_lang(&self) -> String {
        match self.cfg.language.as_str() {
            "follow" => self.dirs.ime_language().to_string(),
            l => l.to_string(),
        }
    }

    fn listening_pill_label(&self) -> String {
        let lang_lbl = match self.rec_lang.as_str() {
            "zh" => "译为中文",
            "en" => "译为英语",
            "it" => "译为意语",
            "es" => "译为西语",
            _ => "自动检测(原声)",
        };
        let src_tag = if self.active_source == "system" {
            if self.cfg.target_pid > 0 {
                "指定软件内录 · "
            } else {
                "仅电脑声音 · "
            }
        } else if self.active_source == "both" {
            "麦+电脑混合 · "
        } else {
            "仅麦克风 · "
        };
        let dual_tag = if self.cfg.cursor_mode == "independent" { " [双光标]" } else { "" };
        if self.cfg.continuous_mode {
            if self.committed_chunks > 0 {
                format!("{}{}{} · 持续聆听 (已转录 {} 段 · 再按结束)", src_tag, lang_lbl, dual_tag, self.committed_chunks)
            } else {
                format!("{}{}{} · 持续聆听 (边听边打字互不干扰)", src_tag, lang_lbl, dual_tag)
            }
        } else {
            format!("{}{}{}", src_tag, lang_lbl, dual_tag)
        }
    }

    fn on_pin_cursor_hotkey(&mut self) {
        let fg = unsafe { GetForegroundWindow() };
        if !fg.is_null() && !inject::is_heyu_own_hwnd(fg) {
            let root = inject::root_hwnd(fg);
            self.target_root_hwnd = root;
            self.target_title = inject::window_short_title(root);
            self.target_selected_at = Some(Instant::now());
            self.target_locked = true;
            self.edit_hwnd = std::ptr::null_mut();
            self.voice_char_offset = None;
        }
        if self.cfg.cursor_mode != "independent" {
            self.cfg.cursor_mode = "independent".into();
            let _ = self.dirs.save_config(&self.cfg);
            self.cfg_mtime = self.dirs.config_mtime();
        }
        if inject::is_luciole_hwnd(self.target_root_hwnd) {
            self.cursor_mark.hide();
            self.voice_cursor_ready = true;
            self.user_moved_away = false;
            let seq = self.next_luciole_seq();
            let _ = inject::send_luciole_pin_caret(seq, "independent");
        } else if let Some((cx, cy, lh)) = self.uia.anchor_at_current_caret().or_else(|| self.query_real_caret_pt()) {
            self.voice_cursor_pt = Some((cx, cy));
            self.voice_cursor_ready = true;
            self.voice_line_h = lh;
            self.user_moved_away = false;
            self.cursor_mark.show_at(cx, cy, lh);
            self.mark_visible_until = Some(Instant::now() + std::time::Duration::from_secs(5));
        }
        self.publish_live_status();
        self.message("🔀 已锁定当前目标窗口并定位语音光标（Ctrl+Alt+左键可随时重新定点）".into(), false, 2400);
    }

    fn on_anchor_voice_cursor(&mut self, click_x: i32, click_y: i32) {
        let pt = POINT { x: click_x, y: click_y };
        if inject::is_heyu_own_window(pt) {
            return;
        }
        // Automatically enable Dual-Cursor mode ("independent") when the user presses Ctrl+Alt+LeftClick
        if self.cfg.cursor_mode != "independent" {
            self.cfg.cursor_mode = "independent".into();
            let _ = self.dirs.save_config(&self.cfg);
            self.cfg_mtime = self.dirs.config_mtime();
        }
        let click_root = inject::root_window_from_point(pt);
        if !click_root.is_null() {
            self.target_root_hwnd = click_root;
            self.target_title = inject::window_short_title(click_root);
            self.target_selected_at = Some(Instant::now());
            self.target_locked = true;
            self.target_win_rect = inject::window_rect(click_root);
            self.edit_hwnd = std::ptr::null_mut();
            self.voice_char_offset = None;
        }
        self.voice_cursor_ready = true;
        self.user_has_selection = false;

        if inject::is_luciole_hwnd(click_root) {
            // Native ProseMirror Second Cursor inside Luciole: zero mouse click simulation,
            // user's primary cursor stays 100% untouched!
            self.cursor_mark.hide();
            self.voice_cursor_pt = Some((click_x, click_y));
            let seq = self.next_luciole_seq();
            let _ = inject::send_luciole_anchor(seq, click_x, click_y, "independent");
        } else if let Some((cx, cy, lh)) = self.uia.anchor_at_screen_point(click_x, click_y) {
            // OS-level UIAutomation RangeFromPoint anchored the exact character offset without clicking!
            self.voice_cursor_pt = Some((cx, cy));
            self.voice_line_h = lh;
            self.user_moved_away = true;
            self.cursor_mark.show_at(cx, cy, lh);
            self.mark_visible_until = Some(Instant::now() + std::time::Duration::from_secs(6));
        } else {
            self.voice_cursor_pt = Some((click_x, click_y));
            self.user_cursor_pt = Some((click_x, click_y));
            self.user_moved_away = false;
            inject::wait_modifiers_released(600);
            INJECTING_VOICE.store(true, Ordering::SeqCst);
            inject::click_screen_point_preserve_mouse(click_x, click_y);
            unsafe { Sleep(14) };
            inject::send_caret_ping();
            INJECTING_VOICE.store(false, Ordering::SeqCst);
            if let Some((cx, cy, lh)) = self.uia.anchor_at_current_caret().or_else(|| self.query_real_caret_pt()) {
                self.voice_cursor_pt = Some((cx, cy));
                self.voice_line_h = lh;
            }
            let (vx, vy) = self.voice_cursor_pt.unwrap_or((click_x, click_y));
            self.cursor_mark.show_at(vx, vy, self.voice_line_h);
            self.mark_visible_until = Some(Instant::now() + std::time::Duration::from_secs(6));
        }

        self.publish_live_status();
        self.start_tick();
        let t_name = if self.target_title.is_empty() { "当前窗口" } else { &self.target_title };
        self.message(format!("🔒 语音光标已定点并锁定「{}」· 您可自由去别处编辑或切换软件", t_name), false, 2600);
    }

    /// Keep `voice_cursor_pt`, `user_cursor_pt`, and the green `┃语` indicator locked to `target_root_hwnd`
    /// whenever the user drags or moves the target application window across the screen!
    fn sync_target_window_move(&mut self) {
        if self.cfg.cursor_mode != "independent" || inject::is_luciole_hwnd(self.target_root_hwnd) {
            if self.cursor_mark.visible {
                self.cursor_mark.hide();
            }
            if self.cfg.cursor_mode != "independent" {
                self.mark_visible_until = None;
            }
        }
        let Some(wr) = inject::window_rect(self.target_root_hwnd) else {
            self.target_win_rect = None;
            return;
        };
        if let Some(prev) = self.target_win_rect {
            let dx = wr.left - prev.left;
            let dy = wr.top - prev.top;
            if dx != 0 || dy != 0 {
                if let Some((ux, uy)) = self.user_cursor_pt {
                    self.user_cursor_pt = Some((ux + dx, uy + dy));
                }
                if let Some((rx, ry, rlh)) = self.uia.voice_range_screen_point() {
                    self.voice_cursor_pt = Some((rx, ry));
                    self.voice_line_h = rlh;
                    if self.cursor_mark.visible && self.cfg.cursor_mode == "independent" && !inject::is_luciole_hwnd(self.target_root_hwnd) {
                        self.cursor_mark.show_at(rx, ry, rlh);
                    }
                } else if let Some((vx, vy)) = self.voice_cursor_pt {
                    let (nvx, nvy) = (vx + dx, vy + dy);
                    self.voice_cursor_pt = Some((nvx, nvy));
                    if self.cursor_mark.visible && self.cfg.cursor_mode == "independent" && !inject::is_luciole_hwnd(self.target_root_hwnd) {
                        self.cursor_mark.show_at(nvx, nvy, self.voice_line_h);
                    }
                }
            }
        }
        self.target_win_rect = Some(wr);
    }

    fn on_user_mouse_down(&mut self, down_x: i32, down_y: i32) {
        if INJECTING_VOICE.load(Ordering::SeqCst) {
            return;
        }
        let pt = POINT { x: down_x, y: down_y };
        if inject::is_heyu_own_window(pt) {
            self.mouse_down_pt = None;
            self.mouse_down_win_rect = None;
            return;
        }
        let click_root = inject::root_window_from_point(pt);
        self.mouse_down_pt = Some((down_x, down_y));
        self.mouse_down_win_rect = inject::window_rect(click_root);
    }

    fn on_user_mouse_up(&mut self, up_x: i32, up_y: i32) {
        if INJECTING_VOICE.load(Ordering::SeqCst) {
            return;
        }
        let pt = POINT { x: up_x, y: up_y };
        if inject::is_heyu_own_window(pt) {
            return;
        }
        let click_root = inject::root_window_from_point(pt);
        if click_root.is_null() {
            return;
        }

        let (down_x, down_y) = self.mouse_down_pt.take().unwrap_or((up_x, up_y));
        let down_wr = self.mouse_down_win_rect.take();
        let up_wr = inject::window_rect(click_root);

        // 1. Check if the window itself was dragged/moved or resized, or if the click started in the window title bar!
        if let (Some(dwr), Some(uwr)) = (down_wr, up_wr) {
            let win_moved_or_resized =
                uwr.left != dwr.left || uwr.top != dwr.top || uwr.right != dwr.right || uwr.bottom != dwr.bottom;
            let in_title_bar = down_y < dwr.top + 48;
            if win_moved_or_resized || in_title_bar {
                if click_root == self.target_root_hwnd {
                    self.sync_target_window_move();
                }
                // Dragging a window or clicking its title bar NEVER moves the document caret!
                return;
            }
        }

        // 2. Check if target window is already locked (after 10s, manual lock, or active independent voice cursor)
        let target_alive = !self.target_root_hwnd.is_null()
            && unsafe { IsWindow(self.target_root_hwnd) != 0 };
        let keep_existing_target = target_alive
            && (self.target_locked
                || (self.cfg.cursor_mode == "independent" && self.phase == Phase::Recording && self.voice_cursor_ready));

        if keep_existing_target && click_root != self.target_root_hwnd {
            // Target window has an active Voice Cursor or is locked: clicking in other apps
            // must NEVER hijack the voice target or move the Green Voice Cursor!
            return;
        }

        // 3. Check if the user dragged the mouse inside the document to SELECT / HIGHLIGHT text!
        let drag_dist_x = (up_x - down_x).abs();
        let drag_dist_y = (up_y - down_y).abs();
        if drag_dist_x > 6 || drag_dist_y > 6 {
            self.user_has_selection = true;
            if click_root == self.target_root_hwnd && self.cfg.cursor_mode == "independent" {
                self.user_moved_away = true;
            }
            return;
        }

        // 4. Normal single click inside the document client area (`drag_dist <= 6`):
        // CRITICAL: A normal left-click (`!ctrl_down && !alt_down`) ONLY moves the user's primary system cursor!
        // It must NEVER move the Green Voice Cursor (`voice_cursor_pt` / `uia.voice_range` / `cursor_mark`)!
        self.user_has_selection = false;
        self.user_cursor_pt = Some((up_x, up_y));

        if click_root != self.target_root_hwnd {
            self.target_root_hwnd = click_root;
            self.target_title = inject::window_short_title(click_root);
            self.target_selected_at = Some(Instant::now());
            self.target_locked = false;
            self.target_win_rect = up_wr;
            self.edit_hwnd = std::ptr::null_mut();
            self.voice_char_offset = None;
            self.uia.clear_voice_range();
            self.voice_cursor_pt = None;
            self.voice_cursor_ready = false;
            self.user_moved_away = false;
            self.cursor_mark.hide();
        }

        // User clicked inside the active target application window (`click_root == self.target_root_hwnd`)
        self.sync_target_window_move();
        if self.target_selected_at.is_none() {
            self.target_selected_at = Some(Instant::now());
        }
        if self.cfg.cursor_mode == "independent" {
            if !self.voice_cursor_ready {
                // First click inside the target window: anchor the Green Voice Cursor (`┃语`) at this initial caret once!
                unsafe { Sleep(18) };
                self.voice_cursor_ready = true;
                self.user_moved_away = false;
                if inject::is_luciole_hwnd(click_root) {
                    self.cursor_mark.hide();
                    self.voice_cursor_pt = Some((up_x, up_y));
                    let seq = self.next_luciole_seq();
                    let _ = inject::send_luciole_pin_caret(seq, "independent");
                } else if let Some((cx, cy, lh)) = self.uia.anchor_at_current_caret().or_else(|| self.uia.anchor_at_screen_point(up_x, up_y)) {
                    self.voice_cursor_pt = Some((cx, cy));
                    self.voice_line_h = lh;
                    if self.phase == Phase::Recording {
                        self.cursor_mark.show_at(cx, cy, lh);
                    }
                } else {
                    self.voice_cursor_pt = Some((up_x, up_y));
                    if self.phase == Phase::Recording {
                        self.cursor_mark.show_at(up_x, up_y, self.voice_line_h);
                    }
                }
            } else {
                // Subsequent normal left-clicks NEVER move the Green Voice Cursor!
                // Only `Ctrl + Alt + 左键` or `Ctrl + Alt + G` moves the Green Voice Cursor.
                self.user_moved_away = true;
            }
        } else {
            self.voice_cursor_pt = Some((up_x, up_y));
            self.voice_cursor_ready = true;
            self.user_moved_away = false;
            self.cursor_mark.hide();
        }
        self.publish_live_status();
    }

    fn on_user_newline_key(&mut self, vk: u32) {
        if INJECTING_VOICE.load(Ordering::SeqCst) {
            return;
        }
        // Record physical key press timestamp so voice flushing pauses cleanly while the user is typing!
        if !matches!(vk, 0x10..=0x12 | 0xA0..=0xA5 | 0x88 | 0xE7) {
            self.last_user_key_at = Some(Instant::now());
            self.start_tick();
        }
        // Check if the user selected text with Shift+Arrow or Ctrl+A
        let shift_down = unsafe { (GetAsyncKeyState(VK_SHIFT) as u16 & 0x8000) != 0 };
        let ctrl_down = unsafe { (GetAsyncKeyState(VK_CONTROL) as u16 & 0x8000) != 0 };
        if (shift_down && (0x21..=0x28).contains(&vk)) || (ctrl_down && vk == 'A' as u32) {
            self.user_has_selection = true;
        } else if (0x30..=0x5A).contains(&vk) || matches!(vk, 0x08 | 0x0D | 0x20) {
            self.user_has_selection = false;
        }

        if self.cfg.cursor_mode != "independent" || !self.user_moved_away || inject::is_luciole_hwnd(self.target_root_hwnd) {
            return;
        }
        if let Some((rx, ry, rlh)) = self.uia.voice_range_screen_point() {
            self.voice_cursor_pt = Some((rx, ry));
            self.voice_line_h = rlh;
            if self.cursor_mark.visible {
                self.cursor_mark.show_at(rx, ry, rlh);
            }
            return;
        }
        let Some((vx, mut vy)) = self.voice_cursor_pt else { return };
        let (ux, mut uy) = self.user_cursor_pt.unwrap_or((vx, vy));
        let lh = self.voice_line_h.clamp(18, 48);

        if vk == 0x0D {
            if uy <= vy + lh / 2 {
                vy += lh;
                self.voice_cursor_pt = Some((vx, vy));
                if self.cursor_mark.visible && self.cfg.cursor_mode == "independent" {
                    self.cursor_mark.show_at(vx, vy, lh);
                }
            }
            uy += lh;
            self.user_cursor_pt = Some((ux, uy));
        }
    }

    fn on_tsf_caret(&mut self, cx: i32, cy: i32, lh: i32) {
        let lh = lh.clamp(18, 52);
        self.voice_line_h = lh;
        self.last_tsf_caret = Some((cx, cy, lh, Instant::now()));
        if self.cfg.cursor_mode != "independent" || inject::is_luciole_hwnd(self.target_root_hwnd) {
            self.voice_cursor_pt = Some((cx, cy));
            self.cursor_mark.hide();
            return;
        }
        if INJECTING_VOICE.load(Ordering::SeqCst) || !self.user_moved_away {
            self.voice_cursor_pt = Some((cx, cy));
            if self.phase == Phase::Recording || self.mark_visible_until.is_some() {
                self.cursor_mark.show_at(cx, cy, lh);
            }
        } else {
            // User is editing elsewhere: track the user's live caret position!
            self.user_cursor_pt = Some((cx, cy));
        }
    }

    /// Safely dequeue any pending `WM_APP_TSF_CARET` messages directly without calling `DispatchMessageW`
    /// so we never re-enter `APP` (`RefCell`) while `on_result` is already borrowing `APP` mutably.
    fn poll_tsf_caret_messages(&mut self) {
        unsafe {
            const PM_REMOVE: u32 = 1;
            #[link(name = "user32")]
            extern "system" {
                fn PeekMessageW(msg: *mut MSG, h: HWND, a: u32, b: u32, rm: u32) -> BOOL;
            }
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, self.hwnd, WM_APP_TSF_CARET, WM_APP_TSF_CARET, PM_REMOVE) != 0 {
                let cx = ((msg.wParam as u64) >> 32) as u32 as i32;
                let cy = (msg.wParam as u32) as i32;
                let lh = (msg.lParam as i32).clamp(16, 56);
                self.on_tsf_caret(cx, cy, lh);
            }
        }
    }

    /// When Microphone voice dictation is triggered (via `oiu`, `Ctrl+Alt+V`, `Ctrl+Shift+V`, or `toggle_mic`),
    /// always activate Single-Cursor Follow mode (`cursor_mode = "follow"`), hide the Dual-Cursor mark,
    /// and sync `voice_config.json` so the UI immediately reflects Single-Cursor Follow!
    fn force_single_cursor_for_mic(&mut self) {
        let changed = self.cfg.cursor_mode != "follow" || self.cfg.audio_source != "mic";
        self.cfg.cursor_mode = "follow".to_string();
        self.cfg.audio_source = "mic".to_string();
        self.target_locked = false;
        self.user_moved_away = false;
        self.voice_cursor_ready = false;
        self.voice_cursor_pt = None;
        self.edit_hwnd = std::ptr::null_mut();
        self.voice_char_offset = None;
        self.uia.clear_voice_range();
        self.cursor_mark.hide();
        if changed {
            self.dirs.save_config(&self.cfg);
            self.cfg_mtime = self.dirs.config_mtime();
        }
    }

    /// When System / Both audio transcription is triggered (via `Ctrl+Alt+B`, `start_system`, `start_both`, etc.),
    /// automatically activate Dual-Cursor Co-Editing mode (`cursor_mode = "independent"`) and sync `voice_config.json`.
    fn force_dual_cursor_for_system(&mut self, source: &str) {
        let changed = self.cfg.cursor_mode != "independent" || self.cfg.audio_source != source;
        self.cfg.cursor_mode = "independent".to_string();
        self.cfg.audio_source = source.to_string();
        if changed {
            self.dirs.save_config(&self.cfg);
            self.cfg_mtime = self.dirs.config_mtime();
        }
    }

    fn on_hotkey(&mut self) {
        self.force_single_cursor_for_mic();
        self.on_hotkey_with_source("mic");
    }

    fn on_loopback_hotkey(&mut self) {
        self.force_dual_cursor_for_system("system");
        self.on_hotkey_with_source("system");
    }

    fn on_hotkey_with_source(&mut self, source: &str) {
        if source == "mic" {
            self.force_single_cursor_for_mic();
        } else if source == "system" || source == "both" {
            self.force_dual_cursor_for_system(source);
        }
        match self.phase {
            Phase::Idle => self.start_recording_source(source),
            Phase::Recording => {
                if self.active_source != source {
                    self.cancel();
                    self.start_recording_source(source);
                } else {
                    self.stop_recording();
                }
            }
            Phase::Recognizing => {}
        }
    }

    fn start_recording_source(&mut self, source: &str) {
        if !self.cfg.enabled {
            return;
        }
        if source == "mic" {
            self.force_single_cursor_for_mic();
        } else if source == "system" || source == "both" {
            self.force_dual_cursor_for_system(source);
        }
        self.pill.place_near_caret(self.dirs.theme_index());
        if self.dirs.runtime_dir().is_none() {
            self.message("未安装语音运行库 · 请在设置-语音输入中下载".into(), true, 3000);
            return;
        }
        if !asr::model_path(&self.cfg.model).is_file() {
            self.message("未安装语音模型 · 请在设置-语音输入中下载".into(), true, 3000);
            return;
        }
        let cap = match (&self.fake_mic, source) {
            (Some(p), _) => audio::Capture::start_fake(p).map_err(|e| e),
            (None, "system") => audio::Capture::start_process_loopback(self.cfg.target_pid),
            (None, "both") => audio::Capture::start_both(self.cfg.target_pid),
            (None, _) => audio::Capture::start_mic().map_err(|e| match e {
                audio::MicError::Denied => "麦克风被系统隐私设置禁止 · 设置 › 隐私和安全性 › 麦克风 › 打开“允许桌面应用访问麦克风”".to_string(),
                audio::MicError::NoDevice => "未检测到麦克风 · 请连接麦克风后重试".to_string(),
                audio::MicError::Open(code) => format!("无法打开麦克风（错误 {}）· 可能被其他程序独占", code),
            }),
        };
        match cap {
            Ok(c) => {
                self.session_id += 1;
                self.pending_chunks = 0;
                self.committed_chunks = 0;
                self.last_committed_char = None;
                self.active_source = source.to_string();
                self.cached_hotwords = self.dirs.load_custom_hotwords();
                self.cached_replace_rules = self.dirs.load_custom_replace_rules(&self.cfg.replace_rules);
                self.last_active_at = Instant::now();
                self.model_unloaded_for_idle = false;
                self.session_ctx.clear();

                // Check current foreground window; if it is one of HEYU's own mini-windows, restore focus
                // to the most recent external target window so typing goes directly into the user's app!
                let fg = unsafe { GetForegroundWindow() };
                if !fg.is_null() && !inject::is_heyu_own_hwnd(fg) {
                    let root = inject::root_hwnd(fg);
                    if root != self.target_root_hwnd {
                        self.target_root_hwnd = root;
                        self.target_title = inject::window_short_title(root);
                        self.target_locked = false;
                        self.edit_hwnd = std::ptr::null_mut();
                        self.voice_char_offset = None;
                        self.uia.clear_voice_range();
                        self.voice_cursor_pt = None;
                        self.voice_cursor_ready = false;
                        self.user_moved_away = false;
                    }
                    self.target_selected_at = Some(Instant::now());
                } else if !self.target_root_hwnd.is_null() && unsafe { IsWindow(self.target_root_hwnd) != 0 } {
                    let _ = inject::force_focus_window(self.target_root_hwnd);
                    if self.target_selected_at.is_none() {
                        self.target_selected_at = Some(Instant::now());
                    }
                } else {
                    self.target_selected_at = None;
                    self.target_locked = false;
                    self.edit_hwnd = std::ptr::null_mut();
                    self.voice_char_offset = None;
                    self.uia.clear_voice_range();
                    self.voice_cursor_pt = None;
                    self.voice_cursor_ready = false;
                    self.user_moved_away = false;
                }

                self.target_win_rect = inject::window_rect(self.target_root_hwnd);
                self.user_has_selection = false;
                self.pending_texts.clear();

                if inject::is_luciole_hwnd(self.target_root_hwnd) {
                    // Luciole renders a real inline DOM Green Voice Cursor inside ProseMirror!
                    self.cursor_mark.hide();
                    self.voice_cursor_ready = true;
                    let seq = self.next_luciole_seq();
                    let mode = self.cfg.cursor_mode.clone();
                    inject::send_luciole_sync(seq, true, &mode);
                } else if self.cfg.cursor_mode == "independent" {
                    if !self.voice_cursor_ready || self.voice_cursor_pt.is_none() {
                        if let Some((cx, cy, lh)) = self.uia.anchor_at_current_caret().or_else(|| self.query_real_caret_pt()) {
                            self.voice_cursor_pt = Some((cx, cy));
                            self.voice_line_h = lh;
                            self.voice_cursor_ready = true;
                            self.user_moved_away = false;
                        } else if let Some((ux, uy)) = self.user_cursor_pt {
                            self.voice_cursor_pt = Some((ux, uy));
                            self.voice_cursor_ready = true;
                            self.user_moved_away = false;
                        }
                    }
                    if let Some((vx, vy)) = self.voice_cursor_pt {
                        self.cursor_mark.show_at(vx, vy, self.voice_line_h);
                    }
                } else {
                    self.cursor_mark.hide();
                }
                self.capture = Some(c);
                self.vad = audio::Vad::new();
                self.rec_start = Instant::now();
                self.chunk_start = Instant::now();
                self.rec_lang = self.resolve_lang();
                self.phase = Phase::Recording;
                self.message_until = None;
                self.set_esc(true);
                self.pill.level = 0.0;
                let lbl = self.listening_pill_label();
                self.pill.show(PillState::Listening { label: lbl });
                self.publish_live_status();
                self.start_tick();
                self.preload();
            }
            Err(msg) => self.message(msg, true, 6000),
        }
    }

    fn publish_live_status(&mut self) {
        self.last_status_write = Instant::now();
        let phase_s = match self.phase {
            Phase::Idle => "idle",
            Phase::Recording => "recording",
            Phase::Recognizing => "recognizing",
        };
        let elapsed = if self.phase == Phase::Idle {
            0.0
        } else {
            self.rec_start.elapsed().as_secs_f32()
        };
        let lock_rem_s = if self.target_locked {
            0
        } else if let Some(sel_at) = self.target_selected_at {
            10u32.saturating_sub(sel_at.elapsed().as_secs() as u32)
        } else {
            10
        };
        self.dirs.write_live_status(
            phase_s,
            &self.active_source,
            self.vad.level,
            self.committed_chunks,
            elapsed,
            &self.last_text,
            &self.target_title,
            self.target_locked,
            lock_rem_s,
        );
    }

    fn flush_current_chunk(&mut self, smart_valley: bool) {
        let Some(cap) = &self.capture else { return };
        let mut pcm = if smart_valley {
            cap.drain_samples_smart(28_800) // search last 1.8s for lowest-energy silence valley
        } else {
            cap.drain_samples_smart(8_000) // search last 500ms for exact silence center
        };
        self.vad = audio::Vad::new();
        self.chunk_start = Instant::now();
        let has_vocal = audio::preprocess_and_verify_human_speech(&pcm);
        if !has_vocal {
            pcm.iter_mut().for_each(|s| *s = 0);
            return;
        }
        // NOTE: Do NOT second-trim `pcm` here! `drain_samples_smart` already truncated `pcm` at the exact
        // silence valley (`best_pos + 160`), and `pcm_to_f32` appends clean 350ms zero padding.
        // Second-trimming was clipping final syllables (`应该明白` -> `应该明`, `喜怒哀乐` -> `喜`).
        let Some(runtime) = self.dirs.runtime_dir() else { return };
        let model = self.effective_model(&runtime);
        let prompt = asr::build_prompt_with_context(
            &self.rec_lang,
            self.cfg.simplified_chinese,
            &self.cached_hotwords,
            self.session_ctx.recent_prompt_tail(),
        );
        self.pending_chunks += 1;
        let _ = self.tx.send(Job::Transcribe {
            session: self.session_id,
            final_chunk: false,
            pcm,
            lang: self.rec_lang.clone(),
            prompt,
            smart_correction: self.cfg.smart_correction,
            simplified_chinese: self.cfg.simplified_chinese,
            session_ctx: self.session_ctx.clone(),
            gpu: self.use_gpu(&runtime),
            runtime,
            model,
        });
    }

    fn stop_recording(&mut self) {
        let Some(cap) = self.capture.take() else {
            self.publish_live_status();
            return;
        };
        let mut pcm = cap.finish();
        // final VAD + vocal-formant pass over remaining samples (rejects breath puffs)
        let mut v = audio::Vad::new();
        v.update(&pcm);
        let has_vocal = audio::preprocess_and_verify_human_speech(&pcm);
        if !has_vocal {
            pcm.iter_mut().for_each(|s| *s = 0);
            if self.pending_chunks > 0 {
                self.phase = Phase::Recognizing;
                self.pill.show(PillState::Recognizing);
            } else {
                self.phase = Phase::Idle;
                self.set_esc(false);
                if self.committed_chunks == 0 {
                    self.message("未检测到语音".into(), false, 1500);
                } else {
                    self.pill.hide();
                }
            }
            self.publish_live_status();
            return;
        }
        // drop trailing silence beyond 900 ms
        let trail = (v.silence_ms.saturating_sub(900) as usize) * 16;
        if trail > 0 && trail < pcm.len() {
            pcm.truncate(pcm.len() - trail);
        }
        let Some(runtime) = self.dirs.runtime_dir() else {
            self.phase = Phase::Idle;
            self.set_esc(false);
            self.publish_live_status();
            self.message("未安装语音运行库 · 请在设置-语音输入中下载".into(), true, 3000);
            return;
        };
        let model = self.effective_model(&runtime);
        self.pending_chunks += 1;
        self.phase = Phase::Recognizing;
        self.pill.show(PillState::Recognizing);
        self.publish_live_status();
        let prompt = asr::build_prompt_with_context(
            &self.rec_lang,
            self.cfg.simplified_chinese,
            &self.cached_hotwords,
            self.session_ctx.recent_prompt_tail(),
        );
        let _ = self.tx.send(Job::Transcribe {
            session: self.session_id,
            final_chunk: true,
            pcm,
            lang: self.rec_lang.clone(),
            prompt,
            smart_correction: self.cfg.smart_correction,
            simplified_chinese: self.cfg.simplified_chinese,
            session_ctx: self.session_ctx.clone(),
            gpu: self.use_gpu(&runtime),
            runtime,
            model,
        });
    }

    fn cancel(&mut self) {
        match self.phase {
            Phase::Recording => {
                self.capture = None; // Drop wipes the samples
                self.session_id += 1;
            }
            Phase::Recognizing => {
                self.session_id += 1; // result will be ignored
            }
            Phase::Idle => {}
        }
        self.pending_chunks = 0;
        self.pending_texts.clear();
        self.phase = Phase::Idle;
        self.set_esc(false);
        self.pill.hide();
        self.cursor_mark.hide();
        if inject::is_luciole_hwnd(self.target_root_hwnd) {
            let seq = self.next_luciole_seq();
            let mode = self.cfg.cursor_mode.clone();
            inject::send_luciole_sync(seq, false, &mode);
        }
        self.publish_live_status();
    }

    fn on_tick(&mut self) {
        self.sync_target_window_move();
        if !self.pending_texts.is_empty() {
            self.try_flush_pending_texts();
        }
        if (self.cfg.cursor_mode != "independent" || inject::is_luciole_hwnd(self.target_root_hwnd)) && self.cursor_mark.visible {
            self.cursor_mark.hide();
        }
        if self.phase == Phase::Recording {
            if !self.target_locked && !(self.cfg.cursor_mode == "independent" && self.voice_cursor_ready) {
                let fg = unsafe { inject::root_hwnd(GetForegroundWindow()) };
                if !fg.is_null() && !inject::is_heyu_own_hwnd(fg) {
                    if fg != self.target_root_hwnd {
                        self.target_root_hwnd = fg;
                        self.target_title = inject::window_short_title(fg);
                        self.target_selected_at = Some(Instant::now());
                        self.target_win_rect = inject::window_rect(fg);
                    } else if self.target_selected_at.is_none() {
                        self.target_selected_at = Some(Instant::now());
                    }
                }
            }
            // Check 10-second target window auto-lock!
            // In System Audio mode (`active_source == "system"` / `"both"`) or Dual-Cursor mode (`cursor_mode == "independent"`),
            // lock onto the target window after 10s so background video transcription never jumps when the user switches apps.
            // In Microphone Single-Cursor Follow mode (`oiu`), always follow the user's active cursor!
            let allow_auto_lock = self.active_source == "system" || self.active_source == "both" || self.cfg.cursor_mode == "independent";
            if allow_auto_lock && !self.target_locked && !self.target_root_hwnd.is_null() {
                if unsafe { IsWindow(self.target_root_hwnd) != 0 } {
                    if let Some(sel_at) = self.target_selected_at {
                        if sel_at.elapsed().as_secs() >= 10 {
                            self.target_locked = true;
                            self.publish_live_status();
                        }
                    }
                } else {
                    self.target_root_hwnd = std::ptr::null_mut();
                    self.target_title.clear();
                    self.target_selected_at = None;
                }
            }

            if let Some(cap) = &self.capture {
                if let Ok(g) = cap.samples.lock() {
                    self.vad.update(&g);
                }
                let failed = cap.failed.load(std::sync::atomic::Ordering::SeqCst);
                self.pill.level = self.vad.level;
                let total_el = self.rec_start.elapsed().as_millis() as u64;
                let chunk_el = self.chunk_start.elapsed().as_millis() as u64;
                let auto = self.cfg.auto_stop_silence_ms as u64;
                if failed != 0 {
                    self.capture = None;
                    self.phase = Phase::Idle;
                    self.set_esc(false);
                    self.publish_live_status();
                    self.message(format!("麦克风启动失败（错误 {}）", failed), true, 4000);
                    return;
                }
                // Throttle disk status writes to ~4 Hz (240 ms) so we never spam the filesystem
                if self.last_status_write.elapsed().as_millis() >= 240 {
                    self.publish_live_status();
                }
                let max_total_ms = if self.active_source == "system" || self.active_source == "both" || self.cfg.continuous_mode {
                    (self.cfg.max_record_s as u64).max(7200) * 1000
                } else {
                    (self.cfg.max_record_s as u64).max(10) * 1000
                };
                if total_el >= max_total_ms {
                    self.stop_recording();
                } else if self.active_source == "system" || self.active_source == "both" {
                    // System audio or Mic+System mixed (videos/lectures/meetings):
                    // Only split at genuine prosodic breath pauses (`>= 480ms` after 3.8s, `>= 360ms` after 7.5s,
                    // or `>= 280ms` after 13.0s) with a 21.0-second ceiling (Whisper's native window is 30s).
                    // Never split on 150-240ms stop-consonant closures, which previously severed compound words
                    // (`交往` -> `交往，网`, `耳闻` -> `耳闻，文`, `明白` -> `明`, `方方面面` -> `方法，方面面`)!
                    let pause_ready = self.vad.speech_started
                        && ((chunk_el >= 3_800 && self.vad.silence_ms >= 480)
                            || (chunk_el >= 7_500 && self.vad.silence_ms >= 360)
                            || (chunk_el >= 13_000 && self.vad.silence_ms >= 280));
                    let chunk_full = self.vad.speech_started && chunk_el >= 21_000;
                    if pause_ready || chunk_full {
                        self.flush_current_chunk(!pause_ready);
                    }
                } else if self.cfg.continuous_mode {
                    // In continuous mic mode: keep phrase context whole (`>= 1.8s` + `>= 560ms` silence,
                    // `>= 3.8s` + `>= 440ms` silence, or `>= 780ms` end-of-sentence pause) so short phrases
                    // are never split mid-word while still responding at real-time typing speed!
                    let pause_ready = self.vad.speech_started
                        && ((chunk_el >= 1_800 && self.vad.silence_ms >= 560)
                            || (chunk_el >= 3_800 && self.vad.silence_ms >= 440)
                            || (self.vad.silence_ms >= 780));
                    let chunk_full = self.vad.speech_started && chunk_el >= 16_000;
                    if pause_ready || chunk_full {
                        self.flush_current_chunk(!pause_ready);
                    }
                } else {
                    let eff_auto = if auto > 0 { auto.min(680) } else { 0 };
                    if eff_auto > 0 && self.vad.speech_started && self.vad.silence_ms as u64 >= eff_auto {
                        self.stop_recording();
                    } else if auto > 0 && !self.vad.speech_started && total_el > 8000 {
                        self.capture = None;
                        self.phase = Phase::Idle;
                        self.set_esc(false);
                        self.publish_live_status();
                        self.message("未检测到语音".into(), false, 1500);
                        return;
                    }
                }
            }
        }
        if let Some(until) = self.message_until {
            if Instant::now() >= until {
                self.message_until = None;
                if self.phase == Phase::Idle {
                    self.pill.hide();
                }
            }
        }
        if let Some(m_until) = self.mark_visible_until {
            if Instant::now() >= m_until && self.phase == Phase::Idle {
                self.mark_visible_until = None;
                self.cursor_mark.hide();
            }
        } else if self.phase == Phase::Idle && self.cursor_mark.visible {
            self.cursor_mark.hide();
        }
        self.pill.tick();
        if !self.pill.visible
            && !self.cursor_mark.visible
            && self.phase == Phase::Idle
            && self.pending_texts.is_empty()
        {
            unsafe {
                KillTimer(self.hwnd, TIMER_TICK);
            }
        }
    }

    fn try_flush_pending_texts(&mut self) {
        if self.pending_texts.is_empty() {
            return;
        }
        // Helper to concatenate adjacent streaming segments cleanly:
        // when a Latin word/punctuation ends `batch_text` and a Latin word starts `seg`, insert a space!
        fn append_streaming_segment(acc: &mut String, seg: &str) {
            if seg.is_empty() {
                return;
            }
            if let (Some(prev_ch), Some(next_ch)) = (acc.chars().last(), seg.chars().next()) {
                let prev_latin_or_punct = prev_ch.is_ascii_alphanumeric()
                    || matches!(prev_ch, '.' | ',' | '?' | '!' | ';' | ':' | ')' | '"' | '\'')
                    || (prev_ch.is_alphabetic() && (prev_ch as u32) < 0x2000);
                let next_latin = next_ch.is_ascii_alphanumeric()
                    || matches!(next_ch, '¿' | '¡')
                    || (next_ch.is_alphabetic() && (next_ch as u32) < 0x2000);
                if prev_latin_or_punct && next_latin && !acc.ends_with(' ') && !seg.starts_with(' ') {
                    acc.push(' ');
                }
            }
            acc.push_str(seg);
        }

        // Tier 1: If target window is Luciole with native ProseMirror Dual-Cursor bridge,
        // we can stream directly into the ProseMirror document transaction at any time (even while user types or drags!)
        if inject::is_luciole_hwnd(self.target_root_hwnd) {
            self.cursor_mark.hide();
            let mut batch_text = String::new();
            while let Some(seg) = self.pending_texts.pop_front() {
                append_streaming_segment(&mut batch_text, &seg);
            }
            if batch_text.is_empty() {
                return;
            }
            if self.committed_chunks > 0 {
                if let (Some(prev_ch), Some(next_ch)) = (self.last_committed_char, batch_text.chars().next()) {
                    let prev_latin_or_punct = prev_ch.is_ascii_alphanumeric()
                        || matches!(prev_ch, '.' | ',' | '?' | '!' | ';' | ':' | ')' | '"' | '\'')
                        || (prev_ch.is_alphabetic() && (prev_ch as u32) < 0x2000);
                    let next_latin = next_ch.is_ascii_alphanumeric()
                        || matches!(next_ch, '¿' | '¡')
                        || (next_ch.is_alphabetic() && (next_ch as u32) < 0x2000);
                    if prev_latin_or_punct && next_latin && !batch_text.starts_with(' ') {
                        batch_text.insert(0, ' ');
                    }
                }
            }
            let seq = self.next_luciole_seq();
            let mode = self.cfg.cursor_mode.clone();
            if inject::send_luciole_insert(seq, &batch_text, &mode) {
                self.last_committed_char = batch_text.chars().last();
                self.last_typed_units = batch_text.encode_utf16().count();
                self.committed_chunks += 1;
                self.dirs.append_voice_history(&batch_text);
                self.publish_live_status();
                if self.phase == Phase::Recording {
                    self.pill.place_near_caret(self.dirs.theme_index());
                    let lbl = self.listening_pill_label();
                    self.pill.show(PillState::Listening { label: lbl });
                }
                return;
            }
        }

        // Wait cleanly if the user is holding the mouse button (e.g. dragging the window or selecting text)
        if inject::is_mouse_button_down() {
            return;
        }
        // Wait cleanly if the user is actively typing on the keyboard (within the last 520 ms)
        if self
            .last_user_key_at
            .map(|t| t.elapsed().as_millis() < 520)
            .unwrap_or(false)
        {
            return;
        }

        self.sync_target_window_move();

        let mut batch_text = String::new();
        while let Some(seg) = self.pending_texts.pop_front() {
            append_streaming_segment(&mut batch_text, &seg);
        }
        if batch_text.is_empty() {
            return;
        }
        if self.committed_chunks > 0 {
            if let (Some(prev_ch), Some(next_ch)) = (self.last_committed_char, batch_text.chars().next()) {
                let prev_latin_or_punct = prev_ch.is_ascii_alphanumeric()
                    || matches!(prev_ch, '.' | ',' | '?' | '!' | ';' | ':' | ')' | '"' | '\'')
                    || (prev_ch.is_alphabetic() && (prev_ch as u32) < 0x2000);
                let next_latin = next_ch.is_ascii_alphanumeric()
                    || matches!(next_ch, '¿' | '¡')
                    || (next_ch.is_alphabetic() && (next_ch as u32) < 0x2000);
                if prev_latin_or_punct && next_latin && !batch_text.starts_with(' ') {
                    batch_text.insert(0, ' ');
                }
            }
        }

        inject::wait_modifiers_released(400);

        // 1. Check current foreground window vs our target window (`self.target_root_hwnd`)
        let cur_fg = unsafe { inject::root_hwnd(GetForegroundWindow()) };
        let target_alive =
            !self.target_root_hwnd.is_null() && unsafe { IsWindow(self.target_root_hwnd) != 0 };
        let keep_target = target_alive
            && (self.target_locked || (self.cfg.cursor_mode == "independent" && self.voice_cursor_ready));

        let mut switched_from_external: Option<HWND> = None;
        if inject::is_heyu_own_hwnd(cur_fg) {
            if target_alive {
                let _ = inject::force_focus_window(self.target_root_hwnd);
                unsafe { Sleep(14) };
            }
        } else if keep_target && cur_fg != self.target_root_hwnd {
            // Target window is locked or has an anchored Voice Cursor and the user switched to another app
            switched_from_external = Some(cur_fg);
            let _ = inject::force_focus_window(self.target_root_hwnd);
            unsafe { Sleep(16) };
        } else if !cur_fg.is_null() && !inject::is_heyu_own_hwnd(cur_fg) {
            if cur_fg != self.target_root_hwnd {
                self.target_root_hwnd = cur_fg;
                self.target_title = inject::window_short_title(cur_fg);
                self.target_selected_at = Some(Instant::now());
                self.target_locked = false;
                self.target_win_rect = inject::window_rect(cur_fg);
                self.edit_hwnd = std::ptr::null_mut();
                self.voice_char_offset = None;
                self.uia.clear_voice_range();
            } else if self.target_selected_at.is_none() {
                self.target_selected_at = Some(Instant::now());
            }
        }

        self.sync_target_window_move();

        // 2. Deliver `batch_text` in either Dual-Cursor (`independent`) or Single-Cursor (`follow`) mode!
        let injected_res = if self.cfg.cursor_mode == "independent" {
            // Tier 2A: Native Win32 Edit/RichEdit control (`EM_GETSEL` / `EM_SETSEL` / `EM_REPLACESEL`)
            if let Some(units) = inject::try_inject_independent_edit(
                &batch_text,
                &mut self.edit_hwnd,
                &mut self.voice_char_offset,
            ) {
                Ok(units)
            } else if self.user_moved_away && switched_from_external.is_none() {
                // Tier 2B: OS-level UIAutomation `IUIAutomationTextRange::Select()` Dual-Cursor injection!
                INJECTING_VOICE.store(true, Ordering::SeqCst);
                let uia_outcome = self.uia.inject_at_voice_range(|| inject::commit_via_tsf_or_type(&batch_text));
                if let Some((res, new_pt)) = uia_outcome {
                    for _ in 0..4 {
                        unsafe { Sleep(4) };
                        self.poll_tsf_caret_messages();
                    }
                    if let Some((nvx, nvy, nlh)) = new_pt {
                        self.voice_cursor_pt = Some((nvx, nvy));
                        self.voice_line_h = nlh;
                    }
                    self.voice_cursor_ready = true;
                    INJECTING_VOICE.store(false, Ordering::SeqCst);
                    if let Some((nvx, nvy)) = self.voice_cursor_pt {
                        self.cursor_mark.show_at(nvx, nvy, self.voice_line_h);
                    }
                    res
                } else if let Some((vx, vy)) = self.voice_cursor_pt {
                    // Fallback when target app does not expose IUIAutomationTextPattern:
                    // hop to `(vx, vy)`, commit, and ONLY update `voice_cursor_pt` from a real OS caret query
                    // (NEVER guess pixel offsets with `advance_point_for_typed_text`!).
                    let saved_user_pt = self.user_cursor_pt;
                    let pre_tsf_time = self.last_tsf_caret.map(|c| c.3);
                    inject::click_screen_point_preserve_mouse(vx, vy);
                    unsafe { Sleep(14) };
                    self.poll_tsf_caret_messages();
                    let res = inject::commit_via_tsf_or_type(&batch_text);
                    for _ in 0..5 {
                        unsafe { Sleep(5) };
                        self.poll_tsf_caret_messages();
                    }
                    let tsf_updated = self.last_tsf_caret.map(|c| c.3) != pre_tsf_time;
                    if !tsf_updated {
                        if let Some((cx, cy, clh)) = self.query_real_caret_pt() {
                            self.voice_cursor_pt = Some((cx, cy));
                            self.voice_line_h = clh;
                        }
                    }
                    let _ = self.uia.anchor_at_current_caret();
                    self.voice_cursor_ready = true;
                    if !self.user_has_selection {
                        if let Some((ux, uy)) = saved_user_pt {
                            inject::click_screen_point_preserve_mouse(ux, uy);
                            unsafe { Sleep(8) };
                        }
                    }
                    INJECTING_VOICE.store(false, Ordering::SeqCst);
                    if let Some((nvx, nvy)) = self.voice_cursor_pt {
                        self.cursor_mark.show_at(nvx, nvy, self.voice_line_h);
                    }
                    res
                } else {
                    if self.user_has_selection {
                        inject::send_collapse_selection_right();
                        self.user_has_selection = false;
                    }
                    let res = inject::commit_via_tsf_or_type(&batch_text);
                    for _ in 0..5 {
                        unsafe { Sleep(5) };
                        self.poll_tsf_caret_messages();
                    }
                    if let Some((cx, cy, clh)) = self.uia.anchor_at_current_caret().or_else(|| self.query_real_caret_pt()) {
                        self.voice_cursor_pt = Some((cx, cy));
                        self.voice_line_h = clh;
                        self.voice_cursor_ready = true;
                    }
                    INJECTING_VOICE.store(false, Ordering::SeqCst);
                    if let Some((nvx, nvy)) = self.voice_cursor_pt {
                        self.cursor_mark.show_at(nvx, nvy, self.voice_line_h);
                    }
                    res
                }
            } else {
                // Active caret in the target window IS the Voice Cursor: NEVER click screen coordinates!
                let pre_tsf_time = self.last_tsf_caret.map(|c| c.3);
                INJECTING_VOICE.store(true, Ordering::SeqCst);
                if self.user_has_selection {
                    inject::send_collapse_selection_right();
                    self.user_has_selection = false;
                }
                let res = inject::commit_via_tsf_or_type(&batch_text);
                for _ in 0..5 {
                    unsafe { Sleep(5) };
                    self.poll_tsf_caret_messages();
                }
                let tsf_updated = self.last_tsf_caret.map(|c| c.3) != pre_tsf_time;
                if let Some((cx, cy, clh)) = self.uia.anchor_at_current_caret() {
                    if !tsf_updated {
                        self.voice_cursor_pt = Some((cx, cy));
                        self.voice_line_h = clh;
                    }
                } else if !tsf_updated {
                    if let Some((gx, gy, glh)) = self.query_real_caret_pt() {
                        self.voice_cursor_pt = Some((gx, gy));
                        self.voice_line_h = glh;
                    }
                }
                self.voice_cursor_ready = true;
                INJECTING_VOICE.store(false, Ordering::SeqCst);
                if let Some((nvx, nvy)) = self.voice_cursor_pt {
                    self.cursor_mark.show_at(nvx, nvy, self.voice_line_h);
                }
                res
            }
        } else {
            // Single-Cursor ("follow") mode: hide second cursor mark, protect any active selection, and commit directly!
            self.cursor_mark.hide();
            INJECTING_VOICE.store(true, Ordering::SeqCst);
            if self.user_has_selection {
                inject::send_collapse_selection_right();
                self.user_has_selection = false;
            }
            let res = inject::commit_via_tsf_or_type(&batch_text);
            for _ in 0..4 {
                unsafe { Sleep(4) };
                self.poll_tsf_caret_messages();
            }
            INJECTING_VOICE.store(false, Ordering::SeqCst);
            res
        };

        // 3. If we temporarily switched focus from another app into the locked target window, restore focus back!
        if let Some(prev_fg) = switched_from_external {
            if unsafe { IsWindow(prev_fg) != 0 } {
                INJECTING_VOICE.store(true, Ordering::SeqCst);
                let _ = inject::force_focus_window(prev_fg);
                INJECTING_VOICE.store(false, Ordering::SeqCst);
            }
        }

        match injected_res {
            Ok(units) => {
                self.last_committed_char = batch_text.chars().last();
                self.last_typed_units = units;
                self.committed_chunks += 1;
                self.dirs.append_voice_history(&batch_text);
                self.publish_live_status();
                if self.phase == Phase::Recording {
                    self.pill.place_near_caret(self.dirs.theme_index());
                    let lbl = self.listening_pill_label();
                    self.pill.show(PillState::Listening { label: lbl });
                }
            }
            Err(e) => self.message(e, true, 3000),
        }
    }

    fn on_result(&mut self, r: AsrResult) {
        if r.session != self.session_id {
            return;
        }
        self.pending_chunks = self.pending_chunks.saturating_sub(1);
        let is_done = self.phase == Phase::Recognizing && (r.final_chunk || self.pending_chunks == 0);
        if is_done {
            self.phase = Phase::Idle;
            self.set_esc(false);
            if inject::is_luciole_hwnd(self.target_root_hwnd) {
                let seq = self.next_luciole_seq();
                let mode = self.cfg.cursor_mode.clone();
                inject::send_luciole_sync(seq, false, &mode);
            }
        }
        self.last_active_at = Instant::now();
        let _ = (r.ms, &r.lang);
        match r.text {
            Ok(t) if !t.is_empty() => {
                if is_done {
                    self.pill.hide();
                }
                let t = if self.cached_replace_rules.is_empty() {
                    t
                } else {
                    corrector::apply_replace_rules(&t, &self.cached_replace_rules)
                };
                let t = self.session_ctx.dedup_boundary_prefix(&t);
                if t.is_empty() {
                    self.publish_live_status();
                    return;
                }
                self.session_ctx.observe_committed(&t);
                self.last_text = t.clone();
                let now_ms = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                self.stream_seq = (self.stream_seq + 1).max(now_ms);
                self.dirs.append_voice_stream(self.stream_seq, &self.rec_lang, &self.active_source, &t);

                if asr::is_voice_undo_command(&t) {
                    if self.pending_texts.pop_back().is_none() {
                        if inject::is_luciole_hwnd(self.target_root_hwnd) {
                            let seq = self.next_luciole_seq();
                            let mode = self.cfg.cursor_mode.clone();
                            let _ = inject::send_luciole_undo(seq, &mode);
                            self.last_typed_units = 0;
                        } else if self.last_typed_units > 0 {
                            inject::wait_modifiers_released(400);
                            let _ = inject::send_backspaces(self.last_typed_units);
                            self.last_typed_units = 0;
                        }
                    }
                } else {
                    self.pending_texts.push_back(t);
                    self.try_flush_pending_texts();
                    if !self.pending_texts.is_empty() {
                        self.start_tick();
                    }
                }
            }
            Ok(_) => {
                if is_done && self.committed_chunks == 0 {
                    self.message("未识别到内容".into(), false, 1500);
                } else if is_done {
                    self.pill.hide();
                }
                self.publish_live_status();
            }
            Err(e) if e.starts_with("MODEL_MISSING") => {
                self.publish_live_status();
                self.message("未安装语音模型 · 请在设置-语音输入中下载".into(), true, 3000);
            }
            Err(e) if e == "RUNTIME_MISSING" => {
                self.publish_live_status();
                self.message("未安装语音运行库 · 请在设置-语音输入中下载".into(), true, 3000);
            }
            Err(e) => {
                self.publish_live_status();
                let short: String = e.chars().take(48).collect();
                self.message(format!("语音识别失败：{}", short), true, 4000);
            }
        }
    }

    fn check_config(&mut self) {
        // Track the most recent non-HEYU foreground window when not locked so if the user clicks a button
        // in `--mini-voice` (`🎙 实时声音转写`), we always know which external app (`Luciole`, `txt`, `Google`, etc.) was active!
        if !self.target_locked && self.phase == Phase::Idle {
            let fg = unsafe { GetForegroundWindow() };
            if !fg.is_null() && !inject::is_heyu_own_hwnd(fg) {
                let root = inject::root_hwnd(fg);
                if root != self.target_root_hwnd {
                    self.target_root_hwnd = root;
                    self.target_title = inject::window_short_title(root);
                }
            }
        }

        // Auto-unload Whisper model from RAM/VRAM after `idle_unload_mins` minutes of inactivity
        // (ported from `SherpaManager` idle timer; reloads at 0ms when recording next starts)
        if self.phase != Phase::Idle {
            self.last_active_at = Instant::now();
        } else if self.cfg.idle_unload_mins > 0 && !self.model_unloaded_for_idle {
            if self.last_active_at.elapsed().as_secs() >= (self.cfg.idle_unload_mins as u64) * 60 {
                self.model_unloaded_for_idle = true;
                let _ = self.tx.send(Job::Unload);
            }
        }

        let m = self.dirs.config_mtime();
        if m != self.cfg_mtime {
            let (new, mt) = self.dirs.load_config();
            self.cfg_mtime = mt;
            self.cached_hotwords = self.dirs.load_custom_hotwords();
            self.cached_replace_rules = self.dirs.load_custom_replace_rules(&new.replace_rules);
            if new != self.cfg {
                let old = std::mem::replace(&mut self.cfg, new);
                if inject::is_luciole_hwnd(self.target_root_hwnd) {
                    self.cursor_mark.hide();
                    let seq = self.next_luciole_seq();
                    let mode = self.cfg.cursor_mode.clone();
                    inject::send_luciole_sync(seq, self.phase == Phase::Recording, &mode);
                } else if self.cfg.cursor_mode != "independent" {
                    self.cursor_mark.hide();
                    self.mark_visible_until = None;
                    self.user_moved_away = false;
                } else if self.phase == Phase::Recording && self.voice_cursor_ready {
                    if let Some((vx, vy)) = self.voice_cursor_pt {
                        self.cursor_mark.show_at(vx, vy, self.voice_line_h);
                    }
                }
                if old.hotkey != self.cfg.hotkey || old.loopback_hotkey != self.cfg.loopback_hotkey || old.enabled != self.cfg.enabled {
                    self.register_hotkey();
                }
                if old.target_pid != self.cfg.target_pid && self.phase == Phase::Recording && (self.active_source == "system" || self.active_source == "both") {
                    let cap_res = if self.active_source == "both" {
                        audio::Capture::start_both(self.cfg.target_pid)
                    } else {
                        audio::Capture::start_process_loopback(self.cfg.target_pid)
                    };
                    if let Ok(c) = cap_res {
                        self.capture = Some(c);
                        self.vad = audio::Vad::new();
                        self.chunk_start = Instant::now();
                        let lbl = self.listening_pill_label();
                        self.pill.show(PillState::Listening { label: lbl });
                    }
                }
                if old.language != self.cfg.language && self.phase == Phase::Recording {
                    self.rec_lang = self.resolve_lang();
                    let lbl = self.listening_pill_label();
                    self.pill.show(PillState::Listening { label: lbl });
                }
                if !self.cfg.enabled {
                    if self.phase != Phase::Idle {
                        self.cancel();
                    }
                    let _ = self.tx.send(Job::Unload);
                } else if old.model != self.cfg.model || old.use_gpu != self.cfg.use_gpu || !old.enabled {
                    self.preload();
                }
            }
        }
        if let Some(cmd) = self.dirs.poll_voice_cmd(&mut self.last_cmd_seq) {
            match cmd.as_str() {
                "toggle_system" => self.on_hotkey_with_source("system"),
                "start_system" => {
                    if self.phase == Phase::Recording && self.active_source != "system" {
                        self.cancel();
                    }
                    if self.phase == Phase::Idle {
                        self.start_recording_source("system");
                    }
                }
                "toggle_mic" => self.on_hotkey_with_source("mic"),
                "start_mic" => {
                    if self.phase == Phase::Recording && self.active_source != "mic" {
                        self.cancel();
                    }
                    if self.phase == Phase::Idle {
                        self.start_recording_source("mic");
                    }
                }
                "toggle_both" => self.on_hotkey_with_source("both"),
                "start_both" => {
                    if self.phase == Phase::Recording && self.active_source != "both" {
                        self.cancel();
                    }
                    if self.phase == Phase::Idle {
                        self.start_recording_source("both");
                    }
                }
                "stop" => {
                    if self.phase == Phase::Recording {
                        self.stop_recording();
                    } else {
                        self.cancel();
                    }
                }
                "lock_window" => {
                    if !self.target_root_hwnd.is_null() && unsafe { IsWindow(self.target_root_hwnd) != 0 } {
                        self.target_locked = true;
                        self.publish_live_status();
                    }
                }
                "unlock_window" => {
                    self.target_locked = false;
                    self.target_selected_at = Some(Instant::now());
                    self.publish_live_status();
                }
                "toggle_lock" => {
                    if self.target_locked {
                        self.target_locked = false;
                        self.target_selected_at = Some(Instant::now());
                    } else if !self.target_root_hwnd.is_null() && unsafe { IsWindow(self.target_root_hwnd) != 0 } {
                        self.target_locked = true;
                    }
                    self.publish_live_status();
                }
                _ => {}
            }
        }
    }
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.borrow_mut().as_mut().map(f))
}

unsafe extern "system" fn msg_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match m {
        WM_HOTKEY => {
            match w as i32 {
                HK_MAIN => {
                    with_app(|a| a.on_hotkey());
                }
                HK_LOOPBACK => {
                    with_app(|a| a.on_loopback_hotkey());
                }
                HK_PIN_CURSOR => {
                    with_app(|a| a.on_pin_cursor_hotkey());
                }
                HK_UNDO_VOICE => {
                    with_app(|a| a.on_undo_voice_hotkey());
                }
                HK_ESC => {
                    with_app(|a| a.cancel());
                }
                _ => {}
            }
            0
        }
        WM_TIMER => {
            match w {
                TIMER_TICK => {
                    with_app(|a| a.on_tick());
                }
                TIMER_CFG => {
                    with_app(|a| a.check_config());
                }
                _ => {}
            }
            0
        }
        WM_ASR_RESULT => {
            let r = Box::from_raw(l as *mut AsrResult);
            with_app(|a| a.on_result(*r));
            0
        }
        WM_VOICE_TOGGLE_MIC => {
            with_app(|a| {
                a.check_config();
                a.on_hotkey_with_source("mic");
            });
            0
        }
        WM_VOICE_TOGGLE_SYSTEM => {
            with_app(|a| {
                a.check_config();
                a.on_hotkey_with_source("system");
            });
            0
        }
        WM_VOICE_STOP => {
            with_app(|a| {
                if a.phase == Phase::Recording {
                    a.stop_recording();
                } else {
                    a.cancel();
                }
            });
            0
        }
        WM_VOICE_START_MIC => {
            with_app(|a| {
                a.check_config();
                if a.phase == Phase::Recording && a.active_source != "mic" {
                    a.cancel();
                }
                if a.phase == Phase::Idle {
                    a.start_recording_source("mic");
                }
            });
            0
        }
        WM_VOICE_START_SYSTEM => {
            with_app(|a| {
                a.check_config();
                if a.phase == Phase::Recording && a.active_source != "system" {
                    a.cancel();
                }
                if a.phase == Phase::Idle {
                    a.start_recording_source("system");
                }
            });
            0
        }
        WM_VOICE_TOGGLE_BOTH => {
            with_app(|a| {
                a.check_config();
                a.on_hotkey_with_source("both");
            });
            0
        }
        WM_VOICE_START_BOTH => {
            with_app(|a| {
                a.check_config();
                if a.phase == Phase::Recording && a.active_source != "both" {
                    a.cancel();
                }
                if a.phase == Phase::Idle {
                    a.start_recording_source("both");
                }
            });
            0
        }
        WM_APP_TSF_CARET => {
            let cx = ((w as u64) >> 32) as u32 as i32;
            let cy = (w as u32) as i32;
            let lh = (l as i32).clamp(16, 56);
            with_app(|a| a.on_tsf_caret(cx, cy, lh));
            0
        }
        WM_APP_ANCHOR_VOICE_CURSOR => {
            let click_x = w as isize as i32;
            let click_y = l as i32;
            with_app(|a| a.on_anchor_voice_cursor(click_x, click_y));
            0
        }
        WM_APP_USER_MOUSE_DOWN => {
            let down_x = w as isize as i32;
            let down_y = l as i32;
            with_app(|a| a.on_user_mouse_down(down_x, down_y));
            0
        }
        WM_APP_USER_MOUSE_UP => {
            let up_x = w as isize as i32;
            let up_y = l as i32;
            with_app(|a| a.on_user_mouse_up(up_x, up_y));
            0
        }
        WM_APP_USER_NEWLINE_KEY => {
            let vk = w as u32;
            with_app(|a| a.on_user_newline_key(vk));
            0
        }
        WM_CLOSE => {
            DestroyWindow(h);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}

fn create_msg_window() -> HWND {
    unsafe {
        let inst = GetModuleHandleW(std::ptr::null());
        let cls = wide(MSG_CLASS);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(msg_proc),
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
        // A hidden top-level window (not HWND_MESSAGE) so `--quit` / installers can FindWindow it.
        CreateWindowExW(WS_EX_TOOLWINDOW, cls.as_ptr(), wide("HEYU Voice").as_ptr(), WS_POPUP, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), inst, std::ptr::null_mut())
    }
}

// ---------------------------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------------------------

struct Args {
    transcribe_file: Option<PathBuf>,
    out_file: Option<PathBuf>,
    loopback_seconds: Option<u32>,
    record: bool,
    source: String,
    target_pid: u32,
    denoise: bool,
    voice_focus: bool,
    max_seconds: Option<u32>,
    stop_file: Option<PathBuf>,
    status_file: Option<PathBuf>,
    fake_mic: Option<PathBuf>,
    model: Option<String>,
    lang: Option<String>,
    data_dir: Option<PathBuf>,
    cpu: bool,
    cli: bool,
    type_out: bool,
    delay_ms: u64,
    repeat: u32,
    quit: bool,
    verbose: bool,
}

fn parse_args() -> Args {
    let mut a = Args {
        transcribe_file: None,
        out_file: None,
        loopback_seconds: None,
        record: false,
        source: "mic".into(),
        target_pid: 0,
        denoise: false,
        voice_focus: false,
        max_seconds: None,
        stop_file: None,
        status_file: None,
        fake_mic: None,
        model: None,
        lang: None,
        data_dir: None,
        cpu: false,
        cli: false,
        type_out: false,
        delay_ms: 0,
        repeat: 1,
        quit: false,
        verbose: false,
    };
    let mut it = std::env::args_os().skip(1);
    while let Some(arg) = it.next() {
        match arg.to_string_lossy().as_ref() {
            "--transcribe-file" => a.transcribe_file = it.next().map(PathBuf::from),
            "--out-file" | "--out" => a.out_file = it.next().map(PathBuf::from),
            "--loopback-seconds" => a.loopback_seconds = it.next().and_then(|s| s.to_string_lossy().parse().ok()),
            "--record" => a.record = true,
            "--source" => a.source = it.next().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_else(|| "mic".into()),
            "--target-pid" => a.target_pid = it.next().and_then(|s| s.to_string_lossy().parse().ok()).unwrap_or(0),
            "--denoise" => a.denoise = true,
            "--voice-focus" => a.voice_focus = true,
            "--max-seconds" => a.max_seconds = it.next().and_then(|s| s.to_string_lossy().parse().ok()),
            "--stop-file" => a.stop_file = it.next().map(PathBuf::from),
            "--status-file" => a.status_file = it.next().map(PathBuf::from),
            "--fake-mic" => a.fake_mic = it.next().map(PathBuf::from),
            "--model" => a.model = it.next().map(|s| s.to_string_lossy().to_string()),
            "--lang" => a.lang = it.next().map(|s| s.to_string_lossy().to_ascii_lowercase()),
            "--data-dir" => a.data_dir = it.next().map(PathBuf::from),
            "--cpu" => a.cpu = true,
            "--cli" => a.cli = true,
            "--type" => a.type_out = true,
            "--delay-ms" => a.delay_ms = it.next().and_then(|s| s.to_string_lossy().parse().ok()).unwrap_or(0),
            "--repeat" => a.repeat = it.next().and_then(|s| s.to_string_lossy().parse().ok()).unwrap_or(1).max(1),
            "--quit" | "--exit" => a.quit = true,
            "--verbose" => a.verbose = true,
            _ => {}
        }
    }
    a
}

fn out(line: &str) {
    use std::io::Write;
    let mut o = std::io::stdout();
    let _ = writeln!(o, "{}", line);
    let _ = o.flush();
}

/// Split long 16 kHz PCM into segments of at most ~24 seconds, choosing 200ms log-dB quiet valleys with recency bias.
fn split_pcm_chunks(pcm: &[i16]) -> Vec<&[i16]> {
    const MAX_CHUNK: usize = 24 * 16_000;
    const MIN_SPLIT: usize = 16 * 16_000;
    if pcm.len() <= 28 * 16_000 {
        return vec![pcm];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < pcm.len() {
        let rem = pcm.len() - start;
        if rem <= 28 * 16_000 {
            chunks.push(&pcm[start..]);
            break;
        }
        let search_from = start + MIN_SPLIT;
        let search_to = (start + MAX_CHUNK).min(pcm.len());
        let best_pos = audio::find_quiet_cut_16k(pcm, search_from, search_to).max(start + 1600);
        chunks.push(&pcm[start..best_pos]);
        start = best_pos;
    }
    chunks
}

/// Offline test & batch file transcription path: WAV/media → (pill) → whisper → stdout / `--out-file` (and optionally typed into foreground app).
fn run_transcribe_file(args: &Args, wav: &Path) -> i32 {
    let dirs = Dirs::resolve(args.data_dir.clone());
    let (cfg, _) = dirs.load_config();
    let model = args.model.clone().unwrap_or(cfg.model.clone());
    let lang = args.lang.clone().unwrap_or_else(|| match cfg.language.as_str() {
        "follow" => dirs.ime_language().to_string(),
        l => l.to_string(),
    });
    let Some(runtime) = dirs.runtime_dir() else {
        out("error=RUNTIME_MISSING");
        return 2;
    };
    let gpu = !args.cpu && match cfg.use_gpu.as_str() {
        "false" => false,
        "true" => true,
        _ => asr::runtime_has_cuda(&runtime),
    };
    let pcm = match audio::read_audio_any_16k_mono(wav) {
        Ok(p) => p,
        Err(e) => {
            out(&format!("error={}", e));
            return 2;
        }
    };
    let hotwords = dirs.load_custom_hotwords();
    let replace_rules = dirs.load_custom_replace_rules(&cfg.replace_rules);
    let prompt_str = asr::build_prompt(&lang, cfg.simplified_chinese, &hotwords);
    let prompt = prompt_str.as_deref();
    let smart_corr = cfg.smart_correction;
    let threads = asr::physical_cores();
    out(&format!("runtime={} model={} gpu={} threads={} lang={} hotwords={} audio_s={:.2}", runtime.display(), model, gpu, threads, lang, hotwords.len(), pcm.len() as f32 / 16000.0));

    if args.delay_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(args.delay_ms));
    }
    let mut pill = if args.type_out {
        let mut p = Pill::new();
        p.place_near_caret(dirs.theme_index());
        p.show(PillState::Recognizing);
        Some(p)
    } else {
        None
    };

    let mut final_text = String::new();
    if args.cli {
        for i in 0..args.repeat {
            let t0 = Instant::now();
            let mut seg_texts = Vec::new();
            for chunk in split_pcm_chunks(&pcm) {
                match asr::transcribe_cli(&runtime, &model, chunk, &lang, prompt, threads, gpu) {
                    Ok(t) => {
                        let clean = asr::postprocess_with_options(&t, &lang, prompt, smart_corr);
                        let clean = corrector::apply_replace_rules(&clean, &replace_rules);
                        if !clean.is_empty() {
                            seg_texts.push(clean);
                        }
                    }
                    Err(e) => {
                        out(&format!("error={}", e));
                        return 3;
                    }
                }
            }
            let sep = if lang == "zh" { "" } else { " " };
            final_text = seg_texts.join(sep);
            out(&format!("run={} path=cli total_ms={} text={}", i + 1, t0.elapsed().as_millis(), final_text));
        }
    } else {
        // pump the pill animation while the model loads/decodes on a worker thread
        let (tx, rx) = channel();
        let (rt2, m2, pcm2, lang2, verbose, repeat, rules2) = (runtime.clone(), model.clone(), pcm.clone(), lang.clone(), args.verbose, args.repeat, replace_rules.clone());
        let prompt2 = prompt.map(String::from);
        std::thread::spawn(move || {
            let mut w = match asr::Whisper::load(&rt2, &m2, gpu, verbose) {
                Ok(w) => w,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            let _ = tx.send(Ok(format!("load_ms={}", w.load_ms)));
            for i in 0..repeat {
                let t0 = Instant::now();
                let mut seg_texts = Vec::new();
                for chunk in split_pcm_chunks(&pcm2) {
                    let f = asr::pcm_to_f32(chunk);
                    match w.transcribe(&f, &lang2, prompt2.as_deref(), threads) {
                        Ok(t) => {
                            let clean = asr::postprocess_with_options(&t, &lang2, prompt2.as_deref(), smart_corr);
                            let clean = corrector::apply_replace_rules(&clean, &rules2);
                            if !clean.is_empty() {
                                seg_texts.push(clean);
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            return;
                        }
                    }
                }
                let ms = t0.elapsed().as_millis();
                let sep = if lang2 == "zh" { "" } else { " " };
                let joined = seg_texts.join(sep);
                let _ = tx.send(Ok(format!("RESULT\t{}\t{}\t{}", i + 1, ms, joined)));
            }
            let _ = tx.send(Ok("DONE".into()));
        });
        loop {
            match rx.recv_timeout(std::time::Duration::from_millis(33)) {
                Ok(Ok(s)) if s == "DONE" => break,
                Ok(Ok(s)) if s.starts_with("RESULT\t") => {
                    let mut p = s.splitn(4, '\t').skip(1);
                    let (run, ms, text) = (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or(""));
                    final_text = text.to_string();
                    let rtf = ms.parse::<f32>().unwrap_or(0.0) / 1000.0 / (pcm.len() as f32 / 16000.0).max(0.1);
                    out(&format!("run={} path=ffi transcribe_ms={} rtf={:.3} text={}", run, ms, rtf, text));
                }
                Ok(Ok(s)) => out(&s),
                Ok(Err(e)) => {
                    out(&format!("error={}", e));
                    if e == "LAYOUT_MISMATCH" {
                        out("hint=FFI layout check failed; retry with --cli");
                    }
                    return 3;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(p) = pill.as_mut() {
                        p.tick();
                    }
                    pump_messages();
                }
                Err(_) => break,
            }
        }
    }
    if let Some(mut p) = pill.take() {
        p.hide();
    }
    if let Some(out_p) = &args.out_file {
        let _ = std::fs::write(out_p, &final_text);
    }
    if args.type_out && !final_text.is_empty() {
        inject::wait_modifiers_released(2000);
        match inject::type_text(&final_text) {
            Ok(n) => {
                dirs.append_voice_history(&final_text);
                out(&format!("typed_utf16_units={}", n));
            }
            Err(e) => out(&format!("type_error={}", e)),
        }
    }
    0
}

fn run_loopback_cli(args: &Args, secs: u32) -> i32 {
    let cap = match audio::Capture::start_process_loopback(args.target_pid) {
        Ok(c) => c,
        Err(e) => {
            out(&format!("error={}", e));
            return 2;
        }
    };
    std::thread::sleep(std::time::Duration::from_secs(secs.clamp(1, 60) as u64));
    let pcm = cap.finish();
    out(&format!("loopback_captured_samples={} audio_s={:.2}", pcm.len(), pcm.len() as f32 / 16000.0));
    let mut v = audio::Vad::new();
    v.update(&pcm);
    if !v.speech_started {
        out("status=SILENT_OR_NO_PLAYBACK text=");
        if let Some(out_p) = &args.out_file {
            let _ = std::fs::write(out_p, "");
        }
        return 0;
    }
    let dir = config::tmp_dir();
    let _ = std::fs::create_dir_all(&dir);
    let wav = dir.join("heyu_loopback_test.wav");
    if let Err(e) = audio::write_wav_16k_mono(&wav, &pcm) {
        out(&format!("error={}", e));
        return 2;
    }
    let rc = run_transcribe_file(args, &wav);
    let _ = std::fs::remove_file(&wav);
    rc
}

fn pump_messages() {
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        const PM_REMOVE: u32 = 1;
        #[link(name = "user32")]
        extern "system" {
            fn PeekMessageW(msg: *mut MSG, h: HWND, a: u32, b: u32, rm: u32) -> BOOL;
        }
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn voice_mouse_ll_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code == HC_ACTION && l_param != 0 {
        let ms = &*(l_param as *const MSLLHOOKSTRUCT);
        if ms.dwExtraInfo != inject::HEYU_MAGIC_EXTRA && !INJECTING_VOICE.load(Ordering::SeqCst) {
            let msg = w_param as u32;
            if msg == WM_LBUTTONDOWN {
                let ctrl_down = (GetAsyncKeyState(VK_CONTROL) as u16 & 0x8000) != 0;
                let alt_down = (GetAsyncKeyState(VK_MENU) as u16 & 0x8000) != 0;
                let shift_down = (GetAsyncKeyState(VK_SHIFT) as u16 & 0x8000) != 0;
                let hwnd = VOICE_HWND_ATOMIC.load(Ordering::SeqCst) as HWND;
                if ctrl_down && alt_down && !shift_down {
                    // User pressed Ctrl + Alt + Left Mouse Click -> Anchor the independent Voice Cursor!
                    SWALLOW_NEXT_LBUTTONUP.store(true, Ordering::SeqCst);
                    if !hwnd.is_null() {
                        let _ = PostMessageW(hwnd, WM_APP_ANCHOR_VOICE_CURSOR, ms.pt.x as isize as WPARAM, ms.pt.y as LPARAM);
                    }
                    return 1;
                } else if !ctrl_down && !alt_down {
                    if !hwnd.is_null() {
                        let _ = PostMessageW(hwnd, WM_APP_USER_MOUSE_DOWN, ms.pt.x as isize as WPARAM, ms.pt.y as LPARAM);
                    }
                }
            } else if msg == WM_LBUTTONUP {
                if SWALLOW_NEXT_LBUTTONUP.swap(false, Ordering::SeqCst) {
                    return 1;
                }
                let ctrl_down = (GetAsyncKeyState(VK_CONTROL) as u16 & 0x8000) != 0;
                let alt_down = (GetAsyncKeyState(VK_MENU) as u16 & 0x8000) != 0;
                if !ctrl_down && !alt_down {
                    let hwnd = VOICE_HWND_ATOMIC.load(Ordering::SeqCst) as HWND;
                    if !hwnd.is_null() {
                        let _ = PostMessageW(hwnd, WM_APP_USER_MOUSE_UP, ms.pt.x as isize as WPARAM, ms.pt.y as LPARAM);
                    }
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param)
}

unsafe extern "system" fn voice_kbd_ll_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code == HC_ACTION && l_param != 0 {
        let kb = &*(l_param as *const KBDLLHOOKSTRUCT);
        if kb.dwExtraInfo != inject::HEYU_MAGIC_EXTRA && !INJECTING_VOICE.load(Ordering::SeqCst) {
            let msg = w_param as u32;
            if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                let hwnd = VOICE_HWND_ATOMIC.load(Ordering::SeqCst) as HWND;
                if !hwnd.is_null() {
                    let _ = PostMessageW(hwnd, WM_APP_USER_NEWLINE_KEY, kb.vkCode as WPARAM, 0);
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param)
}

fn main() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let args = parse_args();

    if args.quit {
        unsafe {
            let h = FindWindowW(wide(MSG_CLASS).as_ptr(), std::ptr::null());
            if !h.is_null() {
                PostMessageW(h, WM_CLOSE, 0, 0);
            }
        }
        return;
    }
    if let Some(wav) = args.transcribe_file.clone() {
        std::process::exit(run_transcribe_file(&args, &wav));
    }
    if let Some(secs) = args.loopback_seconds {
        std::process::exit(run_loopback_cli(&args, secs));
    }
    if args.record {
        let Some(out_wav) = args.out_file.as_deref() else {
            out("error=MISSING_OUT_FILE");
            std::process::exit(2);
        };
        std::process::exit(recorder::run_studio_recorder(
            out_wav,
            &args.source,
            args.target_pid,
            args.denoise,
            args.voice_focus,
            args.max_seconds,
            args.stop_file.as_deref(),
            args.status_file.as_deref(),
        ));
    }

    // ---- resident mode ----
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 1, wide(MUTEX_NAME).as_ptr()) };
    if mutex.is_null() || unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return;
    }
    asr::clean_tmp();
    let dirs = Dirs::resolve(args.data_dir.clone());
    dirs.ensure_config();
    dirs.cleanup_zero_retention();
    let (cfg, cfg_mtime) = dirs.load_config();

    let hwnd = create_msg_window();
    VOICE_HWND_ATOMIC.store(hwnd as usize, Ordering::SeqCst);
    let (tx, rx) = channel::<Job>();
    let verbose = args.verbose;
    let h_usize = hwnd as usize;
    std::thread::spawn(move || asr_worker(rx, h_usize, verbose));

    let cached_hotwords = dirs.load_custom_hotwords();
    let cached_replace_rules = dirs.load_custom_replace_rules(&cfg.replace_rules);
    let app = App {
        hwnd,
        dirs,
        cfg,
        cfg_mtime,
        pill: Pill::new(),
        phase: Phase::Idle,
        session_id: 0,
        stream_seq: 0,
        pending_chunks: 0,
        committed_chunks: 0,
        last_typed_units: 0,
        last_committed_char: None,
        last_cmd_seq: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        last_text: String::new(),
        active_source: "mic".into(),
        target_root_hwnd: std::ptr::null_mut(),
        target_title: String::new(),
        target_selected_at: None,
        target_locked: false,
        target_win_rect: None,
        cursor_mark: VoiceCursorMark::new(),
        uia: uia::UiaVoiceAnchor::new(),
        edit_hwnd: std::ptr::null_mut(),
        voice_char_offset: None,
        luciole_seq: 0,
        voice_cursor_pt: None,
        voice_cursor_ready: false,
        voice_line_h: 26,
        user_cursor_pt: None,
        user_moved_away: false,
        mouse_down_pt: None,
        mouse_down_win_rect: None,
        user_has_selection: false,
        last_user_key_at: None,
        pending_texts: std::collections::VecDeque::new(),
        last_tsf_caret: None,
        mark_visible_until: None,
        last_status_write: Instant::now(),
        cached_hotwords,
        cached_replace_rules,
        last_active_at: Instant::now(),
        model_unloaded_for_idle: false,
        session_ctx: corrector::SessionContext::new(),
        capture: None,
        vad: audio::Vad::new(),
        rec_start: Instant::now(),
        chunk_start: Instant::now(),
        rec_lang: "auto".into(),
        tx,
        hotkey_registered: false,
        loopback_registered: false,
        esc_registered: false,
        message_until: None,
        fake_mic: args.fake_mic.clone(),
        gpu_forced_off: args.cpu,
    };
    APP.with(|a| *a.borrow_mut() = Some(app));
    with_app(|a| {
        a.register_hotkey();
        a.publish_live_status();
        a.preload();
        unsafe {
            SetTimer(a.hwnd, TIMER_CFG, 150, std::ptr::null());
        }
    });

    // Run `WH_MOUSE_LL` and `WH_KEYBOARD_LL` on a dedicated, zero-blocking background thread
    // so mouse movements and keystrokes across Windows NEVER wait on the main thread's I/O or SendInput!
    std::thread::spawn(|| unsafe {
        let inst = GetModuleHandleW(std::ptr::null());
        let h_mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, voice_mouse_ll_proc, inst, 0);
        let h_kbd_hook = SetWindowsHookExW(WH_KEYBOARD_LL, voice_kbd_ll_proc, inst, 0);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if !h_mouse_hook.is_null() {
            UnhookWindowsHookEx(h_mouse_hook);
        }
        if !h_kbd_hook.is_null() {
            UnhookWindowsHookEx(h_kbd_hook);
        }
    });

    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        CloseHandle(mutex);
    }
    APP.with(|a| *a.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys() {
        assert_eq!(parse_hotkey("Ctrl+Alt+V"), Some((MOD_CONTROL | MOD_ALT, 'V' as u32)));
        assert_eq!(parse_hotkey("ctrl + shift + space"), Some((MOD_CONTROL | MOD_SHIFT, 0x20)));
        assert_eq!(parse_hotkey("Win+F9"), Some((MOD_WIN, 0x78)));
        assert_eq!(parse_hotkey("Ctrl+Alt"), None);
        assert_eq!(parse_hotkey("Ctrl+Foo"), None);
    }
}
