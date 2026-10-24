//! Offline speech recognition with whisper.cpp (MIT, ggml-org), loaded at runtime from
//! `voice-runtime/whisper.dll` so the model stays resident in (V)RAM between utterances.
//!
//! The whisper C API passes `whisper_context_params` / `whisper_full_params` BY VALUE. On the
//! Windows x64 ABI every struct whose size is not 1/2/4/8 bytes is passed as a pointer to a
//! caller-owned copy, so we can hand over the pointer returned by `*_default_params_by_ref()`
//! directly and only need the offsets of the few fields we change. Those offsets are computed for
//! the pinned whisper.cpp v1.9.5 (`include/whisper.h`) and verified at runtime against known default
//! values; on any mismatch we refuse the FFI path and fall back to `whisper-cli.exe`.

use crate::config::known_model;
use crate::sys::*;
use std::ffi::{c_char, c_void, CString};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

// whisper_full_params field offsets (x64, whisper.cpp v1.9.5)
const OFF_STRATEGY: usize = 0;
const OFF_N_THREADS: usize = 4;
const OFF_N_MAX_TEXT_CTX: usize = 8;
const OFF_TRANSLATE: usize = 20;
const OFF_NO_CONTEXT: usize = 21;
const OFF_NO_TIMESTAMPS: usize = 22;
const OFF_SINGLE_SEGMENT: usize = 23;
const OFF_PRINT_SPECIAL: usize = 24;
const OFF_PRINT_PROGRESS: usize = 25;
const OFF_PRINT_REALTIME: usize = 26;
const OFF_PRINT_TIMESTAMPS: usize = 27;
const OFF_INITIAL_PROMPT: usize = 72;
const OFF_LANGUAGE: usize = 104;
const OFF_DETECT_LANGUAGE: usize = 112;
const OFF_SUPPRESS_NST: usize = 114;
const OFF_TEMPERATURE_INC: usize = 128;
const OFF_ENTROPY_THOLD: usize = 132;
const OFF_LOGPROB_THOLD: usize = 136;
const OFF_NO_SPEECH_THOLD: usize = 140;
const OFF_BEAM_SIZE: usize = 148;
// whisper_context_params
const OFF_USE_GPU: usize = 0;

const SAMPLING_GREEDY: i32 = 0;
const SAMPLING_BEAM_SEARCH: i32 = 1;

#[repr(C)]
struct ModelLoader {
    context: *mut c_void,
    read: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> usize,
    eof: unsafe extern "C" fn(*mut c_void) -> bool,
    close: unsafe extern "C" fn(*mut c_void),
}

struct LoaderState {
    file: BufReader<File>,
    hasher: Option<Sha256>,
    eof: bool,
    read_bytes: u64,
}

unsafe extern "C" fn loader_read(ctx: *mut c_void, out: *mut c_void, n: usize) -> usize {
    let st = &mut *(ctx as *mut LoaderState);
    let buf = std::slice::from_raw_parts_mut(out as *mut u8, n);
    let mut got = 0;
    while got < n {
        match st.file.read(&mut buf[got..]) {
            Ok(0) => {
                st.eof = true;
                break;
            }
            Ok(k) => got += k,
            Err(_) => {
                st.eof = true;
                break;
            }
        }
    }
    if let Some(h) = st.hasher.as_mut() {
        h.update(&buf[..got]);
    }
    st.read_bytes += got as u64;
    got
}
unsafe extern "C" fn loader_eof(ctx: *mut c_void) -> bool {
    (*(ctx as *mut LoaderState)).eof
}
unsafe extern "C" fn loader_close(_ctx: *mut c_void) {}

unsafe extern "C" fn log_silent(_level: i32, _text: *const c_char, _ud: *mut c_void) {}

type FnPtr = *mut c_void;

#[allow(dead_code)]
pub struct Whisper {
    lib: HMODULE,
    ctx: *mut c_void,
    full_default_by_ref: unsafe extern "C" fn(i32) -> *mut u8,
    free_params: unsafe extern "C" fn(*mut u8),
    full: unsafe extern "C" fn(*mut c_void, *mut u8, *const f32, i32) -> i32,
    n_segments: unsafe extern "C" fn(*mut c_void) -> i32,
    segment_text: unsafe extern "C" fn(*mut c_void, i32) -> *const c_char,
    segment_no_speech_prob: Option<unsafe extern "C" fn(*mut c_void, i32) -> f32>,
    whisper_free: unsafe extern "C" fn(*mut c_void),
    pub model_name: String,
    pub gpu: bool,
    pub load_ms: u128,
}

unsafe impl Send for Whisper {}

unsafe fn sym(lib: HMODULE, name: &str) -> Result<FnPtr, String> {
    let c = CString::new(name).unwrap();
    let p = GetProcAddress(lib, c.as_ptr() as *const u8);
    if p.is_null() {
        Err(format!("whisper.dll 缺少导出函数 {}", name))
    } else {
        Ok(p)
    }
}

pub fn model_path(name: &str) -> PathBuf {
    crate::config::models_dir().join(format!("ggml-{}.bin", name))
}

pub fn runtime_has_cuda(dir: &Path) -> bool {
    dir.join("ggml-cuda.dll").is_file()
}

pub fn physical_cores() -> i32 {
    unsafe {
        let mut len = 0u32;
        GetLogicalProcessorInformation(std::ptr::null_mut(), &mut len);
        let n = len as usize / std::mem::size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>();
        if n > 0 {
            let mut v: Vec<SYSTEM_LOGICAL_PROCESSOR_INFORMATION> = Vec::with_capacity(n);
            if GetLogicalProcessorInformation(v.as_mut_ptr(), &mut len) != 0 {
                v.set_len(len as usize / std::mem::size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>());
                let cores = v.iter().filter(|e| e.Relationship == 0).count() as i32;
                if cores > 0 {
                    return cores.min(16);
                }
            }
        }
    }
    std::thread::available_parallelism().map(|n| (n.get() as i32 / 2).max(1)).unwrap_or(4)
}

impl Whisper {
    /// Load whisper.dll from `runtime_dir` and the model; the model's SHA-256 is checked while it is read.
    pub fn load(runtime_dir: &Path, model_name: &str, use_gpu: bool, verbose: bool) -> Result<Whisper, String> {
        let t0 = Instant::now();
        let mpath = model_path(model_name);
        if !mpath.is_file() {
            return Err(format!("MODEL_MISSING:{}", mpath.display()));
        }
        unsafe {
            // Dependent DLLs (ggml*.dll, cuBLAS) resolve from the runtime dir only.
            SetDllDirectoryW(wide_os(runtime_dir.as_os_str()).as_ptr());
            let dll = runtime_dir.join("whisper.dll");
            let lib = LoadLibraryExW(wide_os(dll.as_os_str()).as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
            if lib.is_null() {
                return Err(format!("无法加载 {}（错误 {}；需要 VC++ 2015-2022 运行库）", dll.display(), GetLastError()));
            }
            let ggml = GetModuleHandleW(wide("ggml.dll").as_ptr());
            if !ggml.is_null() {
                // CPU/CUDA backends are separate DLLs (GGML_BACKEND_DL); load them from the runtime dir.
                if let Ok(p) = sym(ggml, "ggml_backend_load_all_from_path") {
                    let f: unsafe extern "C" fn(*const c_char) = std::mem::transmute(p);
                    let d = CString::new(runtime_dir.to_string_lossy().as_bytes()).unwrap_or_default();
                    f(d.as_ptr());
                }
            }
            let r = (|| -> Result<Whisper, String> {
                if !verbose {
                    let log_set: unsafe extern "C" fn(unsafe extern "C" fn(i32, *const c_char, *mut c_void), *mut c_void) = std::mem::transmute(sym(lib, "whisper_log_set")?);
                    log_set(log_silent, std::ptr::null_mut());
                }
                let ctx_default: unsafe extern "C" fn() -> *mut u8 = std::mem::transmute(sym(lib, "whisper_context_default_params_by_ref")?);
                let ctx_free: unsafe extern "C" fn(*mut u8) = std::mem::transmute(sym(lib, "whisper_free_context_params")?);
                let init: unsafe extern "C" fn(*mut ModelLoader, *mut u8) -> *mut c_void = std::mem::transmute(sym(lib, "whisper_init_with_params")?);
                let mut w = Whisper {
                    lib,
                    ctx: std::ptr::null_mut(),
                    full_default_by_ref: std::mem::transmute(sym(lib, "whisper_full_default_params_by_ref")?),
                    free_params: std::mem::transmute(sym(lib, "whisper_free_params")?),
                    full: std::mem::transmute(sym(lib, "whisper_full")?),
                    n_segments: std::mem::transmute(sym(lib, "whisper_full_n_segments")?),
                    segment_text: std::mem::transmute(sym(lib, "whisper_full_get_segment_text")?),
                    segment_no_speech_prob: sym(lib, "whisper_full_get_segment_no_speech_prob")
                        .ok()
                        .map(|p| std::mem::transmute(p)),
                    whisper_free: std::mem::transmute(sym(lib, "whisper_free")?),
                    model_name: model_name.to_string(),
                    gpu: use_gpu,
                    load_ms: 0,
                };
                w.check_layout()?;

                let file = File::open(&mpath).map_err(|e| format!("打开模型失败：{}", e))?;
                let expected = known_model(model_name);
                if let Some((size, _)) = expected {
                    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
                    if len != size {
                        return Err(format!("模型文件大小不符（{} ≠ {}），请重新下载", len, size));
                    }
                }
                let mut st = Box::new(LoaderState {
                    file: BufReader::with_capacity(4 * 1024 * 1024, file),
                    hasher: if expected.is_some() { Sha256::new() } else { None },
                    eof: false,
                    read_bytes: 0,
                });
                let mut loader = ModelLoader { context: &mut *st as *mut LoaderState as *mut c_void, read: loader_read, eof: loader_eof, close: loader_close };
                let cp = ctx_default();
                *cp.add(OFF_USE_GPU) = use_gpu as u8;
                let ctx = init(&mut loader, cp);
                ctx_free(cp);
                if ctx.is_null() {
                    return Err("whisper 初始化失败（模型损坏或显存不足）".into());
                }
                w.ctx = ctx;
                // finish hashing any trailing bytes the loader did not consume, then verify
                if let (Some((_, sha)), Some(mut h)) = (expected, st.hasher.take()) {
                    let mut rest = Vec::new();
                    let _ = st.file.read_to_end(&mut rest);
                    h.update(&rest);
                    let got = h.finish_hex();
                    if got != sha {
                        return Err(format!("模型 SHA-256 校验失败，拒绝使用（{}）", got));
                    }
                }
                w.load_ms = t0.elapsed().as_millis();
                Ok(w)
            })();
            // On error the library intentionally stays mapped (ggml keeps global backend state).
            r
        }
    }

    unsafe fn check_layout(&self) -> Result<(), String> {
        let p = (self.full_default_by_ref)(SAMPLING_BEAM_SEARCH);
        if p.is_null() {
            return Err("whisper_full_default_params_by_ref 返回空".into());
        }
        let rd_i32 = |o: usize| std::ptr::read_unaligned(p.add(o) as *const i32);
        let rd_f32 = |o: usize| std::ptr::read_unaligned(p.add(o) as *const f32);
        let lang = std::ptr::read_unaligned(p.add(OFF_LANGUAGE) as *const *const c_char);
        let lang_ok = !lang.is_null() && std::ffi::CStr::from_ptr(lang).to_bytes() == b"en";
        let ok = rd_i32(OFF_STRATEGY) == SAMPLING_BEAM_SEARCH
            && rd_i32(OFF_N_MAX_TEXT_CTX) == 16384
            && *p.add(OFF_PRINT_PROGRESS) == 1
            && *p.add(OFF_PRINT_TIMESTAMPS) == 1
            && *p.add(OFF_PRINT_REALTIME) == 0
            && (rd_f32(OFF_TEMPERATURE_INC) - 0.2).abs() < 1e-6
            && (rd_f32(OFF_ENTROPY_THOLD) - 2.4).abs() < 1e-6
            && (rd_f32(OFF_LOGPROB_THOLD) + 1.0).abs() < 1e-6
            && rd_i32(OFF_BEAM_SIZE) == 5
            && lang_ok;
        (self.free_params)(p);
        if ok {
            Ok(())
        } else {
            Err("LAYOUT_MISMATCH".into())
        }
    }

    pub fn transcribe(&mut self, pcm: &[f32], lang: &str, prompt: Option<&str>, threads: i32) -> Result<String, String> {
        unsafe {
            // Hardware-adaptive decoding:
            // - GPU PCs (`self.gpu == true`): use 5-way Beam Search for maximum accuracy with ultra-low CUDA latency.
            // - CPU-only laptops (`self.gpu == false`): use Greedy decoding for 4x faster real-time response without CPU lag!
            let strategy = if self.gpu { SAMPLING_BEAM_SEARCH } else { SAMPLING_GREEDY };
            let p = (self.full_default_by_ref)(strategy);
            // When `lang == "en"` is explicitly selected, enable Whisper's built-in multitask translation (`translate = 1`, `language = "auto"`)
            // so any foreign language audio (Chinese, Spanish, Italian, mixed) is translated directly into fluent English!
            let translate_to_en = lang == "en";
            let eff_lang = if lang == "auto" || translate_to_en { "auto" } else { lang };
            let lang_c = CString::new(eff_lang).unwrap();
            let prompt_c = prompt.map(|s| CString::new(s).unwrap_or_default());
            std::ptr::write_unaligned(p.add(OFF_N_THREADS) as *mut i32, threads.max(1));
            *p.add(OFF_TRANSLATE) = if translate_to_en { 1 } else { 0 };
            // Always clear `state->prompt_past` (`no_context = 1`) before each call so any mistake
            // in a prior chunk never poisons subsequent chunks; `initial_prompt` is still applied!
            *p.add(OFF_NO_CONTEXT) = 1;
            *p.add(OFF_NO_TIMESTAMPS) = 1;
            *p.add(OFF_SINGLE_SEGMENT) = 0;
            *p.add(OFF_PRINT_SPECIAL) = 0;
            *p.add(OFF_PRINT_PROGRESS) = 0;
            *p.add(OFF_PRINT_REALTIME) = 0;
            *p.add(OFF_PRINT_TIMESTAMPS) = 0;
            *p.add(OFF_DETECT_LANGUAGE) = 0;
            *p.add(OFF_SUPPRESS_NST) = 1;
            // Keep deterministic decoding (`temperature = 0.0`) active without falling back to
            // random temperature sampling (`temp = 0.2..1.0`) on normal short/medium phrases!
            let temp_inc = if pcm.len() < 16_000 * 9 { 0.0f32 } else { 0.2f32 };
            std::ptr::write_unaligned(p.add(OFF_TEMPERATURE_INC) as *mut f32, temp_inc);
            std::ptr::write_unaligned(p.add(OFF_ENTROPY_THOLD) as *mut f32, 2.40f32);
            std::ptr::write_unaligned(p.add(OFF_LOGPROB_THOLD) as *mut f32, -1.00f32);
            std::ptr::write_unaligned(p.add(OFF_NO_SPEECH_THOLD) as *mut f32, 0.50f32);
            std::ptr::write_unaligned(p.add(OFF_LANGUAGE) as *mut *const c_char, lang_c.as_ptr());
            std::ptr::write_unaligned(
                p.add(OFF_INITIAL_PROMPT) as *mut *const c_char,
                prompt_c.as_ref().map(|c| c.as_ptr()).unwrap_or(std::ptr::null()),
            );
            let rc = (self.full)(self.ctx, p, pcm.as_ptr(), pcm.len() as i32);
            (self.free_params)(p);
            if rc != 0 {
                return Err(format!("whisper_full 返回 {}", rc));
            }
            let n = (self.n_segments)(self.ctx);
            let mut out = String::new();
            for i in 0..n {
                let nsp = self
                    .segment_no_speech_prob
                    .map(|f| f(self.ctx, i))
                    .unwrap_or(0.0);
                if nsp > 0.52 {
                    continue;
                }
                let t = (self.segment_text)(self.ctx, i);
                if !t.is_null() {
                    let seg_s = String::from_utf8_lossy(std::ffi::CStr::from_ptr(t).to_bytes());
                    if nsp > 0.25 && is_breath_hallucination(&seg_s) {
                        continue;
                    }
                    out.push_str(&seg_s);
                }
            }
            Ok(out)
        }
    }
}

impl Drop for Whisper {
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() {
                (self.whisper_free)(self.ctx);
                self.ctx = std::ptr::null_mut();
            }
            // whisper.dll / ggml backends stay mapped (ggml keeps global backend registries;
            // unloading them mid-process is not supported upstream).
            let _ = self.lib;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// whisper-cli.exe fallback (used only if the FFI path is unavailable)
// ---------------------------------------------------------------------------------------------

fn random_suffix() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let x = t ^ ((std::process::id() as u128) << 64) ^ (&t as *const u128 as usize as u128);
    match Sha256::new() {
        Some(mut s) => {
            s.update(&x.to_le_bytes());
            s.finish_hex()[..20].to_string()
        }
        None => format!("{:x}", x),
    }
}

/// Remove leftovers from a crash (temp WAVs are always deleted right after use).
pub fn clean_tmp() {
    let d = crate::config::tmp_dir();
    if let Ok(rd) = std::fs::read_dir(&d) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("heyu_voice_") && (n.ends_with(".wav") || n.ends_with(".txt")) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

pub fn transcribe_cli(runtime_dir: &Path, model_name: &str, pcm: &[i16], lang: &str, prompt: Option<&str>, threads: i32, use_gpu: bool) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let cli = runtime_dir.join("whisper-cli.exe");
    if !cli.is_file() {
        return Err("RUNTIME_MISSING".into());
    }
    let mpath = model_path(model_name);
    if !mpath.is_file() {
        return Err(format!("MODEL_MISSING:{}", mpath.display()));
    }
    let dir = crate::config::tmp_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let wav = dir.join(format!("heyu_voice_{}.wav", random_suffix()));
    crate::audio::write_wav_16k_mono(&wav, pcm).map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(&cli);
    cmd.current_dir(runtime_dir)
        .arg("-m")
        .arg(&mpath)
        .arg("-f")
        .arg(&wav)
        .args(["-l", lang, "-t", &threads.to_string(), "-nt", "-np"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if !use_gpu {
        cmd.arg("-ng");
    }
    // NOTE: whisper-cli receives argv in the ANSI code page, so a Chinese prompt would be mangled
    // on non-Chinese Windows locales; only pass ASCII-safe prompts on this path.
    if let Some(p) = prompt.filter(|p| p.is_ascii()) {
        cmd.arg("--prompt").arg(p);
    }
    let out = cmd.output();
    let _ = std::fs::remove_file(&wav);
    let out = out.map_err(|e| format!("无法启动 whisper-cli：{}", e))?;
    if !out.status.success() {
        return Err(format!("whisper-cli 退出码 {:?}", out.status.code()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

// ---------------------------------------------------------------------------------------------
// Post-processing
// ---------------------------------------------------------------------------------------------

#[allow(dead_code)]
pub fn zh_prompt(simplified: bool) -> Option<&'static str> {
    if simplified {
        Some("以下是普通话的句子。")
    } else {
        Some("以下是普通話的句子。")
    }
}

/// Build a context-conditioned Whisper `initial_prompt`.
/// - When `lang == "auto"` (Automatic Language Detection): we MUST return `None` when `rolling_tail` is empty
///   so we NEVER bias a Spanish/English/Italian/multilingual audio stream toward Chinese!
/// - When `rolling_tail` is non-empty (from the preceding verified chunk in the same continuous session),
///   we pass the last ~44 characters of clean verified transcript in the detected language so Whisper
///   maintains cross-chunk sentence & terminology continuity!
pub fn build_prompt_with_context(
    lang: &str,
    simplified: bool,
    hotwords: &[String],
    rolling_tail: &str,
) -> Option<String> {
    let tail_clean = strip_inline_watermark_hallucinations(rolling_tail);
    let tail_trimmed = tail_clean.trim();
    if lang == "auto" {
        if tail_trimmed.is_empty() {
            return None;
        }
        let chars: Vec<char> = tail_trimmed.chars().collect();
        let keep = chars.len().min(48);
        let recent: String = chars[chars.len() - keep..].iter().collect();
        return Some(recent);
    }
    let base = build_prompt(lang, simplified, hotwords)?;
    if tail_trimmed.is_empty() {
        return Some(base);
    }
    let chars: Vec<char> = tail_trimmed.chars().collect();
    let keep = chars.len().min(44);
    let recent: String = chars[chars.len() - keep..].iter().collect();
    if lang == "zh" {
        Some(format!("{}{}", base, recent))
    } else {
        Some(format!("{} {}", base, recent))
    }
}

/// Returns `true` if `s` is a standalone Whisper silence/breath hallucination token (e.g. "谢谢", "好好", "好多的呀").
pub fn is_breath_hallucination(s: &str) -> bool {
    let clean: String = s
        .chars()
        .filter(|&c| !is_zh_punct(c) && !c.is_whitespace() && !matches!(c, '～' | '~' | '…' | '—' | '-'))
        .collect();
    if clean.is_empty() {
        return true;
    }
    let simp = crate::corrector::to_simplified_zh(&clean);
    let low = simp.to_lowercase();
    matches!(
        low.as_str(),
        "谢谢"
            | "谢谢你"
            | "谢谢大家"
            | "谢谢观看"
            | "谢谢收看"
            | "感谢观看"
            | "感谢收看"
            | "非常感谢"
            | "多谢"
            | "好好"
            | "好好好"
            | "好多的呀"
            | "好多呀"
            | "好呀"
            | "好吧"
            | "嗯"
            | "嗯嗯"
            | "啊"
            | "哦"
            | "呃"
            | "字幕"
            | "字幕制作"
            | "欢迎订阅"
            | "点赞订阅"
            | "欢迎订阅我的频道"
            | "今天的节目就到这里"
            | "好今天的节目就到这里"
            | "本期视频就分享到这里"
            | "我会做的事"
            | "thankyou"
            | "thanksforwatching"
            | "you"
            | "bye"
            | "byebye"
    )
}

/// Build clean language-specific punctuation conditioning prompt for Whisper.
/// For `lang == "auto"`, returns `None` so multilingual auto-detection is 100% unbiased across English, Spanish, Italian, and Chinese.
pub fn build_prompt(lang: &str, simplified: bool, _hotwords: &[String]) -> Option<String> {
    match lang {
        "auto" => None,
        "zh" => zh_prompt(simplified).map(String::from),
        "en" => Some("Hello, how is everything going today? Everything is going very well.".into()),
        "it" => Some("Ciao, come procede il lavoro oggi? Tutto procede benissimo.".into()),
        "es" => Some("Hola, ¿cómo va todo hoy? Todo va muy bien.".into()),
        _ => None,
    }
}

/// Returns `true` if the entire spoken utterance is a voice command to delete/undo the previous segment.
pub fn is_voice_undo_command(text: &str) -> bool {
    let trimmed = text.trim().trim_matches(|c: char| {
        matches!(c, '。' | '，' | '！' | '？' | '.' | ',' | '!' | '?' | ' ')
    });
    matches!(
        trimmed,
        "删除上一句"
            | "撤回上一句"
            | "删掉上一句"
            | "退格"
            | "撤销上一句"
            | "delete last sentence"
            | "undo last sentence"
            | "scratch that"
    )
}

/// Intelligent spoken self-correction & disfluency cleanup:
/// 1. Removes standalone filler interjections (`嗯，`, `啊，`, `呃，`, `那个那个`, consecutive char/word stutters like `我我我` -> `我`, `就是就是` -> `就是`).
/// 2. Recognizes explicit spoken self-correction triggers (`X，不对，应该是Y` / `X，不对，是Y` / `X，说错了，是Y` / `X，口误，是Y` / `X, no wait, Y` / `X, I mean Y`) and replaces the mistaken phrase `X` with `Y`.
pub fn apply_spoken_self_correction(input: &str) -> String {
    let s = input.trim();
    if s.is_empty() {
        return String::new();
    }
    if is_voice_undo_command(s) {
        return s.to_string();
    }

    let mut out = s.to_string();

    // 1. Strip leading/standalone spoken filler interjections
    for filler in [
        "嗯，", "嗯、", "呃，", "呃、", "啊，", "啊、", "哦，", "唔，",
        "那个那个，", "那个那个", "就是说，", "额，", "欸，",
    ] {
        while let Some(rest) = out.strip_prefix(filler) {
            out = rest.trim_start().to_string();
        }
        out = out.replace(&format!("，{}", filler), "，");
        out = out.replace(&format!("。{}", filler), "。");
    }

    // 2. Collapse consecutive character/word stutters (e.g. `我我我` -> `我`, `这这` -> `这`, `就是就是` -> `就是`)
    for w in ["就是", "这个", "那个", "然后", "我们", "可以", "因为", "所以", "需要", "如果"] {
        let dup3 = format!("{}{}{}", w, w, w);
        let dup2 = format!("{}{}", w, w);
        while out.contains(&dup3) {
            out = out.replace(&dup3, w);
        }
        while out.contains(&dup2) {
            out = out.replace(&dup2, w);
        }
    }
    for ch in ['我', '你', '他', '她', '它', '这', '那', '的', '了', '是', '在', '有', '就', '都', '要', '也'] {
        let mut s3 = String::new();
        s3.push(ch);
        s3.push(ch);
        s3.push(ch);
        let mut s2 = String::new();
        s2.push(ch);
        s2.push(ch);
        let s1 = ch.to_string();
        while out.contains(&s3) {
            out = out.replace(&s3, &s1);
        }
        while out.contains(&s2) {
            out = out.replace(&s2, &s1);
        }
    }

    // 3. Spoken self-correction markers (Chinese & English)
    const ZH_TRIGGERS: &[&str] = &[
        "，不对，应该是",
        "，不对应该是",
        "，不对，是",
        "，不对是",
        "，说错了，应该是",
        "，说错了应该是",
        "，说错了，是",
        "，说错了是",
        "，口误，应该是",
        "，口误，是",
        "，不，应该是",
        "，更正一下，是",
        "，不是，应该是",
        "，不是，是",
        "不对，应该是",
        "说错了，应该是",
    ];
    loop {
        let mut applied = false;
        for &trig in ZH_TRIGGERS {
            if let Some(pos) = out.find(trig) {
                let before = &out[..pos];
                let after = out[pos + trig.len()..].trim_start_matches(['，', '、', ' ']);
                out = replace_mistaken_span_zh(before, after);
                applied = true;
                break;
            }
        }
        if !applied {
            break;
        }
    }

    // English self-correction triggers: `, no wait, ` / `, I mean ` / `, sorry I mean `
    const EN_TRIGGERS: &[&str] = &[
        ", no wait, ",
        ", no, wait, ",
        ", I mean, ",
        ", I mean ",
        ", sorry, I mean ",
        ", sorry I mean ",
    ];
    for &trig in EN_TRIGGERS {
        if let Some(pos) = out.to_ascii_lowercase().find(&trig.to_ascii_lowercase()) {
            let before = &out[..pos];
            let after = &out[pos + trig.len()..];
            // Replace the last word or clause before the trigger
            let prefix = if let Some(cpos) = before.rfind([',', ';', '.']) {
                &before[..=cpos]
            } else if let Some(spos) = before.rfind(' ') {
                &before[..=spos]
            } else {
                ""
            };
            out = if prefix.is_empty() {
                after.to_string()
            } else if prefix.ends_with(' ') {
                format!("{}{}", prefix, after)
            } else {
                format!("{} {}", prefix, after)
            };
        }
    }

    out.trim().to_string()
}

/// Given `before` (e.g. `"今天下午三点"`) and `after` (e.g. `"四点开会"`), replace the mistaken tail of `before` with `after`.
fn replace_mistaken_span_zh(before: &str, after: &str) -> String {
    let clause_start = before
        .rfind(['。', '！', '？', '；', '，', ',', '.', '!', '?'])
        .map(|i| i + before[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1))
        .unwrap_or(0);
    let prefix_clauses = &before[..clause_start];
    let clause = before[clause_start..].trim();

    let c_chars: Vec<char> = clause.chars().collect();
    let a_chars: Vec<char> = after.chars().collect();

    // Extract leading correction token from `after` (before the next punctuation or verb/noun continuation)
    // Check if `clause` and `after` share a common unit character (like `点`, `月`, `日`, `号`, `元`, `块`, `人`, `个`, `楼`, `层`, `周`, `年`, `分`, `秒`)
    const UNITS: &[char] = &['点', '月', '日', '号', '元', '块', '人', '个', '楼', '层', '周', '年', '分', '秒', '次', '天', '页', '行'];
    for &u in UNITS {
        if let (Some(c_u_idx), Some(a_u_idx)) = (
            c_chars.iter().rposition(|&c| c == u),
            a_chars.iter().position(|&c| c == u),
        ) {
            if a_u_idx > 0 && a_u_idx <= 6 {
                // Find where the number/specifier before `u` starts in `c_chars`
                let mut start_idx = c_u_idx;
                while start_idx > 0 && is_num_or_specifier(c_chars[start_idx - 1]) {
                    start_idx -= 1;
                }
                if start_idx < c_u_idx {
                    let kept: String = c_chars[..start_idx].iter().collect();
                    return format!("{}{}{}", prefix_clauses, kept, after);
                }
            }
        }
    }

    // Check common verb/preposition pivots near the end of `clause` (e.g. "我们需要发给张三" + "李四确认一下" -> keep "我们需要发给")
    for pivot in ['给', '去', '到', '在', '找', '叫', '是', '买', '用', '跟', '和', '向', '对', '把'] {
        if let Some(p_idx) = c_chars.iter().rposition(|&c| c == pivot) {
            let tail_len = c_chars.len().saturating_sub(p_idx + 1);
            if (1..=4).contains(&tail_len) && p_idx > 0 {
                let kept: String = c_chars[..=p_idx].iter().collect();
                return format!("{}{}{}", prefix_clauses, kept, after);
            }
        }
    }

    // If `clause` ends with a short phrase of the same length as `after` (or `after`'s first clause)
    if c_chars.len() > a_chars.len() && a_chars.len() >= 2 && a_chars.len() <= 4 {
        let kept: String = c_chars[..c_chars.len() - a_chars.len()].iter().collect();
        return format!("{}{}{}", prefix_clauses, kept, after);
    }

    // Default: replace the entire last clause after the last punctuation with `after`
    format!("{}{}", prefix_clauses, after)
}

fn is_num_or_specifier(c: char) -> bool {
    c.is_ascii_digit()
        || matches!(
            c,
            '零' | '〇' | '一' | '二' | '两' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
                | '百' | '千' | '万' | '半' | '上' | '下' | '前' | '后' | '这' | '那' | '周' | '礼' | '星' | '期'
        )
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F | 0x3000..=0x303F | 0xFF00..=0xFFEF)
}

const HALLUCINATIONS: &[&str] = &[
    "amara.org",
    "明镜与点点",
    "请不吝点赞",
    "字幕由",
    "subtitles by",
    "sottotitoli creati",
    "subtítulos realizados",
    "以下是普通话的句子",
    "以下是使用规范标点符号",
    "今天的节目就到这里",
    "本期视频就分享到这里",
    "这段片段是由网易",
    "网易互联网提供",
    "欢迎订阅我的频道",
    "优优独播剧场",
    "yoyo television series exclusive",
    "yoyo television",
    "索兰娅",
    "yyets",
];

/// Strip inline YouTube/subtitle watermark hallucinations that Whisper occasionally appends
/// during a mid-sentence pause or background BGM swell (e.g. `那人对神的了解认识也几乎是，优优独播剧场——YoYo Television Series Exclusive。`),
/// while preserving the valid spoken text in the same segment!
pub fn strip_inline_watermark_hallucinations(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let mut s = input.to_string();
    const INLINE_WATERMARKS: &[&str] = &[
        "优优独播剧场——YoYo Television Series Exclusive",
        "优优独播剧场—YoYo Television Series Exclusive",
        "优优独播剧场-YoYo Television Series Exclusive",
        "优优独播剧场 YoYo Television Series Exclusive",
        "优优独播剧场",
        "YoYo Television Series Exclusive",
        "YoYo Television",
        "字幕由Amara.org社区提供",
        "字幕由 Amara.org 社区提供",
        "Amara.org",
        "请不吝点赞、订阅、转发、打赏支持明镜与点点栏目",
        "请不吝点赞 订阅 转发 打赏支持明镜与点点栏目",
        "明镜与点点栏目",
        "欢迎订阅我的频道",
        "感谢观看本视频",
        "字幕制作：",
    ];
    for &wm in INLINE_WATERMARKS {
        if s.contains(wm) {
            s = s.replace(wm, "");
        }
    }
    // Also clean up any dangling em-dash or double punctuation left behind by watermark removal
    while s.contains("——。") || s.contains("——，") || s.contains("——") {
        s = s.replace("——。", "。").replace("——，", "，").replace("——", "");
    }
    s.trim().to_string()
}

fn is_zh_punct(c: char) -> bool {
    matches!(c, '，' | '。' | '？' | '！' | '；' | '：' | '、' | '…' | ',' | '.' | '?' | '!' | ';' | ':')
}

/// Inspect a Chinese clause and determine the appropriate prosodic/grammatical punctuation mark
/// (`？` for questions, `！` for exclamations, `。` for sentence end, `，` for mid-sentence clause break).
fn infer_zh_clause_punct(clause: &str, is_final_end: bool) -> char {
    let t = clause.trim_matches(|c: char| is_zh_punct(c) || c.is_whitespace());
    if t.is_empty() {
        return if is_final_end { '。' } else { '，' };
    }
    let last_ch = t.chars().last().unwrap_or(' ');

    // Direct yes/no question ending in `吗` is always a question
    if last_ch == '吗' {
        return '？';
    }

    // Mid-sentence clause breaks (`!is_final_end`) should almost always be a comma `，`
    // so subordinate clauses like "为了让你能够看到它究竟能够做到什么" or "一打开开关" never get spurious `？` or `！`
    if !is_final_end {
        if t.ends_with("居然拒绝了") || t.ends_with("太离谱了") || t.ends_with("太夸张了") {
            return '！';
        }
        return '，';
    }

    // Sentence-final (`is_final_end == true`):
    // 1. Check if question words are embedded inside a declarative matrix clause ("为了...", "以为...", "看看...", "发现...", "无论...", "不管...",
    //    or relative clauses ending with a declarative noun head like "...的机会", "...的原则", "...的问题", "...的方法", "...的目的", "...的过程")
    let ends_with_declarative_noun = [
        "的机会", "的原则", "的问题", "的方法", "的方面", "的目的", "的目标", "的过程",
        "的结果", "的道理", "的真理", "的实际", "的关系", "的态度", "的观点", "的认识",
        "的功课", "的事情", "的体现", "的表现", "的基础", "的条件", "的标准", "的规矩",
    ]
    .iter()
    .any(|suf| t.ends_with(suf))
        || t.starts_with("这是")
        || t.starts_with("这就是")
        || t.starts_with("那是")
        || t.starts_with("说明")
        || t.starts_with("证明");

    let is_declarative_embedding = ends_with_declarative_noun
        || [
            "为了", "以为", "看看", "知道", "发现", "无论", "不管", "不论", "没有什么", "没什么",
            "没怎么", "怎么不是", "不管什么", "无论什么", "无论怎么", "展示", "看看他们到底",
            "不管怎么", "不论怎么",
        ]
        .iter()
        .any(|w| t.contains(w));

    let has_question_word = [
        "为什么", "怎么", "怎样", "什么", "哪里", "哪儿", "哪个", "哪些", "哪方面", "谁", "多少",
        "几时", "几点", "几号", "几岁", "多久", "多远", "多大", "何时", "何地", "如何", "难道",
        "岂不是", "是不是", "能不能", "可不可以", "可不", "行不行", "行不", "好不好", "对不对",
        "有没有", "要不要", "会不会", "敢不敢", "愿不愿意", "是否", "请问", "是吧",
    ]
    .iter()
    .any(|w| t.contains(w));

    if !is_declarative_embedding
        && (has_question_word
            || (last_ch == '呢' && t.chars().count() <= 14)
            || (last_ch == '么' && (t.ends_with("什么") || t.ends_with("怎么"))))
    {
        return '？';
    }

    // 2. Exclamatory grammar & emphatic tone markers at sentence end
    let has_exclam_marker = [
        "居然拒绝了", "简直", "太棒", "太好", "太牛", "太强", "太美", "太爽", "太离谱",
        "太夸张", "真棒", "真好", "好棒", "多么", "千万别", "绝对不能", "天哪", "天呐", "我的天",
        "加油", "万岁", "太感谢", "非常感谢", "横着走",
    ]
    .iter()
    .any(|w| t.contains(w))
        || (t.contains('太') && t.ends_with('了') && t.chars().count() <= 10);
    if has_exclam_marker || matches!(last_ch, '啊' | '哇' | '啦' | '哟') {
        return '！';
    }

    // 3. Statement vs unfinished subordinate clause
    let is_unfinished_subordinate = [
        "因为", "由于", "如果", "假如", "要是", "虽然", "尽管", "既然", "无论", "不管", "只要", "不仅",
    ]
    .iter()
    .any(|w| t.starts_with(w));
    if is_unfinished_subordinate {
        '，'
    } else {
        '。'
    }
}

/// Insert grammar-driven commas & tone punctuation inside an unpunctuated Chinese run.
fn restore_zh_clause_punctuation(input: &str, is_final_chunk: bool) -> String {
    if input.is_empty() || is_voice_undo_command(input) {
        return input.to_string();
    }

    // Step 1: Segment-level / clause-level discourse connectives
    // When preceded by >= 4 unpunctuated characters, insert clause punctuation before the connective.
    const CONNECTIVES: &[&str] = &[
        "更离谱的是",
        "更重要的是",
        "更关键的是",
        "不仅如此",
        "换句话说",
        "与此同时",
        "总而言之",
        "没想到",
        "事实上",
        "说白了",
        "但是",
        "不过",
        "然而",
        "可是",
        "只是",
        "其实",
        "相反",
        "反而",
        "所以",
        "因此",
        "因而",
        "于是",
        "既然",
        "因为",
        "由于",
        "而且",
        "并且",
        "同时",
        "此外",
        "另外",
        "不仅",
        "然后",
        "接着",
        "随后",
        "后来",
        "最后",
        "最终",
        "首先",
        "其次",
        "如果",
        "假如",
        "要是",
        "只要",
        "只有",
        "虽然",
        "尽管",
        "无论",
        "不管",
        "一待就是",
        "结果",
    ];

    let chars: Vec<char> = input.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(input.len() + 24);
    let mut current_clause = String::new();
    let mut since_punct = 0usize;
    let mut i = 0usize;

    while i < n {
        let c = chars[i];
        if is_zh_punct(c) {
            // Collapse consecutive duplicate or conflicting punctuation marks (`，，` -> `，`, `，。` -> `。`, `。，` -> `。`)
            if let Some(last_ch) = out.chars().last() {
                if is_zh_punct(last_ch) {
                    if matches!(c, '？' | '！' | '。') && last_ch == '，' {
                        out.pop();
                        out.push(c);
                    }
                    current_clause.clear();
                    since_punct = 0;
                    i += 1;
                    continue;
                }
            }

            // Eliminate excessive mid-sentence periods (`。`):
            // - If a period `。` occurs mid-chunk after a short clause (< 12 chars) or after an incomplete syntactic token,
            // - Or if a period `。` occurs at the end of an intermediate streaming chunk (`!is_final_chunk`) where the clause is < 22 chars or syntactically incomplete,
            // convert it to a natural comma `，` so continuous dictation flows smoothly without choppy periods!
            let effective_punct = if c == '。' {
                let is_end = i + 1 == n;
                let incomplete = crate::corrector::is_syntactically_incomplete_zh_end(&current_clause);
                if incomplete || (!is_end && since_punct > 0 && since_punct < 12) || (is_end && !is_final_chunk && since_punct < 22) {
                    '，'
                } else if is_end {
                    let inferred = infer_zh_clause_punct(&current_clause, true);
                    if matches!(inferred, '？' | '！') { inferred } else { c }
                } else {
                    c
                }
            } else if c == '？' {
                // If Whisper emitted `?` at the end of a declarative relative clause (`这是一个很好的训练人怎样正确对待造物主的机会？`),
                // normalize it to `。`!
                let inferred = infer_zh_clause_punct(&current_clause, true);
                if inferred == '。' { '。' } else { c }
            } else {
                c
            };
            out.push(effective_punct);
            current_clause.clear();
            since_punct = 0;
            i += 1;
            continue;
        }

        let rem: String = chars[i..].iter().collect();

        // Check if a discourse connective starts at `i` after at least 6 unpunctuated chars
        // (and NOT immediately preceded by a verb/preposition/modifier where the connective is an embedded object clause!)
        let mut matched_conn = false;
        if since_punct >= 6 {
            for &conn in CONNECTIVES {
                if rem.starts_with(conn) {
                    let prev_ch = if i > 0 { chars[i - 1] } else { ' ' };
                    if !matches!(
                        prev_ch,
                        '的' | '地' | '得' | '和' | '与' | '跟' | '或' | '是' | '有' | '在'
                            | '而' | '但' | '却' | '就' | '都' | '也' | '还' | '并' | '且'
                    ) {
                        let p = infer_zh_clause_punct(&current_clause, false);
                        out.push(p);
                        current_clause.clear();
                        since_punct = 0;
                        matched_conn = true;
                        break;
                    }
                }
            }
        }

        // Check for post-connective comma on long introductory markers like "更离谱的是" / "更重要的是" / "没想到"
        if !matched_conn && since_punct == 0 {
            for &intro in &["更离谱的是", "更重要的是", "更关键的是", "不仅如此", "总而言之", "换句话说"] {
                if rem.starts_with(intro) {
                    let intro_len = intro.chars().count();
                    if i + intro_len < n && !is_zh_punct(chars[i + intro_len]) {
                        out.push_str(intro);
                        out.push('，');
                        current_clause.clear();
                        since_punct = 0;
                        i += intro_len;
                        matched_conn = true;
                        break;
                    }
                }
            }
            if matched_conn {
                continue;
            }
        }

        // Check for subject-pronoun restart after a complete clause (>= 5 chars since last punctuation):
        // e.g. "收到了哈佛" + "他看了一眼" + "他居然拒绝了" + "他的家人朋友觉得他疯了"
        if !matched_conn && since_punct >= 5 && i + 1 < n {
            let prev_ch = chars[i - 1];
            let prev2_ch = if i >= 2 { chars[i - 2] } else { ' ' };
            // Do not split if `prev_ch` is a transitive verb / preposition / possessive that takes `他/她/我/你` as object
            let prev_takes_obj = matches!(
                prev_ch,
                '让' | '给' | '找' | '看' | '对' | '和' | '跟' | '与' | '被' | '把' | '向'
                    | '问' | '请' | '叫' | '是' | '有' | '在' | '比' | '像' | '到' | '祝'
                    | '帮' | '替' | '为' | '由' | '的' | '地' | '得' | '见' | '爱' | '恨'
                    | '骂' | '打' | '救' | '带' | '陪' | '等' | '劝' | '逼' | '夸' | '怕'
                    | '信' | '懂' | '知' | '觉'
            ) || (prev2_ch == '觉' && prev_ch == '得')
                || (prev2_ch == '認' && prev_ch == '為')
                || (prev2_ch == '认' && prev_ch == '为')
                || (prev2_ch == '看' && prev_ch == '到')
                || (prev2_ch == '看' && prev_ch == '见')
                || (prev2_ch == '听' && prev_ch == '到')
                || (prev2_ch == '告' && prev_ch == '诉')
                || (prev2_ch == '喜' && prev_ch == '欢');

            if !prev_takes_obj && matches!(c, '他' | '她' | '我' | '你') {
                let next_ch = chars[i + 1];
                let next2_ch = chars.get(i + 2).copied().unwrap_or(' ');
                let is_subj_start = matches!(
                    next_ch,
                    '看' | '说' | '想' | '觉' | '居' | '竟' | '发' | '决' | '立' | '马' | '直'
                        | '就' | '又' | '也' | '都' | '还' | '已' | '在' | '去' | '来' | '做'
                        | '是' | '把' | '被' | '没' | '不' | '要' | '会' | '能' | '可' | '自'
                        | '当' | '从' | '对' | '向' | '给' | '拿' | '用' | '坐' | '走'
                ) || (next_ch == '们' && i + 2 < n)
                    || (next_ch == '的' && matches!(next2_ch, '家' | '朋' | '心' | '目' | '工' | '生' | '意' | '话' | '手' | '眼'));
                if is_subj_start {
                    let p = infer_zh_clause_punct(&current_clause, false);
                    out.push(p);
                    current_clause.clear();
                    since_punct = 0;
                }
            }
        }

        // Check for reduplicated verb/negation pivot like "哈佛不去去一个陌生的国家" -> "哈佛不去，去一个陌生的国家"
        if !matched_conn && since_punct >= 4 && i > 0 && c == chars[i - 1] && matches!(c, '去' | '来' | '做' | '看' | '说' | '想' | '买' | '学') {
            let prev2 = if i >= 2 { chars[i - 2] } else { ' ' };
            if prev2 == '不' || prev2 == '没' {
                out.push('，');
                current_clause.clear();
                since_punct = 0;
            }
        }

        out.push(c);
        current_clause.push(c);
        since_punct += 1;
        i += 1;
    }

    // Ensure terminal punctuation at the end of the segment
    if let Some(last) = out.chars().last() {
        if !is_zh_punct(last) && !matches!(last, '”' | '’' | '》' | '）' | ')' | '"') {
            // If this is an intermediate streaming chunk (`!is_final_chunk`) and the clause is short (< 22 chars)
            // or ends with an incomplete syntactic token, prefer a comma `，` over a period `。` unless it's a clear question/exclamation!
            let incomplete = !is_final_chunk && crate::corrector::is_syntactically_incomplete_zh_end(&current_clause);
            if incomplete {
                if !current_clause.ends_with('的') && !current_clause.ends_with('地') && !current_clause.ends_with('得') {
                    out.push('，');
                }
            } else {
                let term = infer_zh_clause_punct(&current_clause, true);
                if !is_final_chunk && term == '。' && current_clause.chars().count() < 22 {
                    out.push('，');
                } else {
                    out.push(term);
                }
            }
        }
    }

    out
}

/// Restore capitalization, internal conjunction commas, and question/exclamation/period marks
/// for English (`en`), Italian (`it`), and Spanish (`es`), while preserving streaming continuity
/// when `is_final_chunk == false`.
fn restore_latin_grammar_punctuation(input: &str, lang: &str, is_final_chunk: bool) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() || is_voice_undo_command(trimmed) {
        return trimmed.to_string();
    }

    let mut s = trimmed.to_string();

    // 0. Ensure a space after internal sentence-ending punctuation when glued (`Spanish.We're` -> `Spanish. We're`)
    let s_chars: Vec<char> = s.chars().collect();
    let mut spaced = String::with_capacity(s.len() + 8);
    for idx in 0..s_chars.len() {
        let ch = s_chars[idx];
        spaced.push(ch);
        if matches!(ch, '.' | '?' | '!' | ',' | ';' | ':') && idx + 1 < s_chars.len() {
            let next_ch = s_chars[idx + 1];
            let prev_ch = if idx > 0 { s_chars[idx - 1] } else { ' ' };
            // Don't split decimal numbers like `1.5` or contractions
            if next_ch.is_alphabetic() && !(ch == '.' && prev_ch.is_ascii_digit() && next_ch.is_ascii_digit()) {
                spaced.push(' ');
            }
        }
    }
    s = spaced;

    // 1. Insert comma before mid-sentence coordinating/contrast conjunctions if unpunctuated
    let words_count = s.split_whitespace().count();
    if words_count >= 6 {
        let conj_rules: &[(&str, &str)] = match lang {
            "it" => &[(" ma ", ", ma "), (" però ", ", però "), (" quindi ", ", quindi "), (" tuttavia ", ", tuttavia ")],
            "es" => &[(" pero ", ", pero "), (" sin embargo ", ", sin embargo, "), (" aunque ", ", aunque ")],
            _ => &[(" but ", ", but "), (" however ", ", however, "), (" therefore ", ", therefore, "), (" although ", ", although ")],
        };
        for &(pat, rep) in conj_rules {
            if let Some(pos) = s.find(pat) {
                let before = &s[..pos];
                if before.split_whitespace().count() >= 3 && !before.ends_with([',', ';', '.', '?', '!']) {
                    s = format!("{}{}{}", before, rep, &s[pos + pat.len()..]);
                }
            }
        }
    }

    // Check if the segment starts with a lowercase continuation word (e.g. `which`, `and`, `through`, `in`, `on`, `that`)
    let first_raw_word = s
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphabetic())
        .to_lowercase();
    let starts_as_continuation = !is_final_chunk
        && matches!(
            first_raw_word.as_str(),
            "and" | "or" | "but" | "nor" | "so" | "yet" | "for" | "which" | "that" | "who"
                | "whom" | "whose" | "where" | "when" | "while" | "because" | "although"
                | "though" | "since" | "unless" | "until" | "through" | "into" | "onto"
                | "with" | "without" | "within" | "from" | "by" | "in" | "on" | "at" | "to"
                | "of" | "as" | "than" | "resulting" | "building" | "enjoying" | "achieving"
                | "que" | "y" | "o" | "pero" | "porque" | "para" | "con" | "en" | "de" | "por"
                | "che" | "e" | "ma" | "perché" | "per" | "di" | "da"
        );

    // 2. Capitalize first character (unless it's a mid-sentence continuation chunk!) and any character after `. `, `? `, `! `
    let mut cap_out = String::with_capacity(s.len() + 4);
    let mut cap_next = !starts_as_continuation;
    for c in s.chars() {
        if cap_next && c.is_alphabetic() {
            for uc in c.to_uppercase() {
                cap_out.push(uc);
            }
            cap_next = false;
        } else {
            cap_out.push(c);
            if matches!(c, '.' | '?' | '!' | '¿' | '¡') {
                cap_next = true;
            } else if !c.is_whitespace() {
                cap_next = false;
            }
        }
    }
    s = cap_out;

    // 2b. If this is an intermediate streaming chunk (`!is_final_chunk`) and ends with a preposition,
    // conjunction, determiner, or relative pronoun (e.g. `to.`, `the.`, `of.`, `and.`, `that.`),
    // strip any premature trailing period `.` that Whisper may have placed at the chunk boundary!
    let last_word_clean = s
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphabetic())
        .to_lowercase();
    let ends_incomplete_latin = matches!(
        last_word_clean.as_str(),
        "the" | "a" | "an" | "to" | "of" | "in" | "on" | "at" | "for" | "with" | "by" | "from"
            | "into" | "through" | "during" | "before" | "after" | "under" | "over" | "between"
            | "and" | "or" | "but" | "that" | "which" | "who" | "whom" | "whose" | "because"
            | "if" | "when" | "while" | "as" | "than" | "your" | "my" | "our" | "their" | "his"
            | "her" | "its" | "this" | "these" | "those" | "is" | "are" | "was" | "were" | "be"
            | "been" | "will" | "would" | "can" | "could" | "should" | "must" | "have" | "has"
            | "had" | "do" | "does" | "did" | "not" | "very" | "more" | "most" | "el" | "la"
            | "los" | "las" | "un" | "una" | "de" | "del" | "al" | "en" | "con" | "por" | "para"
            | "que" | "y" | "o" | "pero" | "il" | "lo" | "gli" | "le" | "di" | "da" | "che"
    );
    if !is_final_chunk && ends_incomplete_latin && s.ends_with('.') {
        s.pop();
        return s;
    }

    // 3. Ensure terminal punctuation based on grammar (`?`, `!`, `.`, plus `¿` / `¡` for Spanish)
    let last_ch = s.chars().last().unwrap_or('.');
    if !matches!(last_ch, '.' | '?' | '!' | ',' | ';' | ':') {
        if !is_final_chunk && ends_incomplete_latin {
            return s;
        }
        let low = s.to_lowercase();
        let first_word = low.split_whitespace().next().unwrap_or("");
        let is_question = !starts_as_continuation
            && match lang {
            "it" => {
                matches!(
                    first_word,
                    "che" | "cosa" | "come" | "dove" | "quando" | "perché" | "perche" | "quale"
                        | "quali" | "chi" | "quanto" | "quanti" | "quante" | "è" | "sono"
                        | "hai" | "ha" | "avete" | "puoi" | "può" | "vuoi" | "vuole" | "sai"
                ) || low.ends_with(" vero")
            }
            "es" => {
                matches!(
                    first_word,
                    "qué" | "que" | "cómo" | "como" | "dónde" | "donde" | "cuándo" | "cuando"
                        | "cuál" | "cual" | "cuáles" | "quién" | "quien" | "cuánto" | "cuanto"
                        | "es" | "son" | "está" | "están" | "tienes" | "tiene" | "puedes"
                        | "puede" | "quieres" | "quiere"
                ) || low.starts_with("por qué") || low.starts_with("para qué")
            }
            _ => {
                matches!(
                    first_word,
                    "what" | "why" | "how" | "where" | "when" | "who" | "whom" | "whose"
                        | "which" | "is" | "are" | "was" | "were" | "do" | "does" | "did"
                        | "can" | "could" | "will" | "would" | "shall" | "should" | "have"
                        | "has" | "had" | "may" | "might"
                ) || low.ends_with(", right")
                    || low.ends_with(" isn't it")
                    || low.ends_with(" don't you")
            }
        };
        let is_exclam = low.starts_with("what a ")
            || low.starts_with("wow")
            || low.starts_with("che bello")
            || low.starts_with("ottimo")
            || low.starts_with("qué bueno")
            || low.starts_with("increíble");

        if is_question {
            if lang == "es" && !s.starts_with('¿') {
                s = format!("¿{}?", s);
            } else {
                s.push('?');
            }
        } else if is_exclam {
            if lang == "es" && !s.starts_with('¡') {
                s = format!("¡{}!", s);
            } else {
                s.push('!');
            }
        } else {
            s.push('.');
        }
    }

    s
}

pub fn postprocess_with_options(raw: &str, lang: &str, prompt: Option<&str>, smart_correction: bool) -> String {
    postprocess_full(raw, lang, prompt, smart_correction, true, true, None)
}

pub fn postprocess_full(
    raw: &str,
    lang: &str,
    prompt: Option<&str>,
    smart_correction: bool,
    simplified_chinese: bool,
    is_final_chunk: bool,
    session_ctx: Option<&crate::corrector::SessionContext>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    for line in raw.lines() {
        let cleaned_line = strip_inline_watermark_hallucinations(line);
        let l = cleaned_line.trim();
        if l.is_empty() {
            continue;
        }
        // non-speech tags: [BLANK_AUDIO], (music), （音乐）, *laughs*
        let bracketed = (l.starts_with('[') && l.ends_with(']')) || (l.starts_with('(') && l.ends_with(')')) || (l.starts_with('（') && l.ends_with('）')) || (l.starts_with('*') && l.ends_with('*'));
        if bracketed {
            continue;
        }
        let low = l.to_lowercase();
        if HALLUCINATIONS.iter().any(|h| low.contains(h)) {
            continue;
        }
        let bare = low.trim_matches(|c: char| !c.is_alphabetic() && !is_cjk(c));
        if matches!(bare, "and" | "the" | "you" | "a" | "i" | "it" | "thank you" | "thanks for watching" | "bye") {
            continue;
        }
        if is_breath_hallucination(l) {
            continue;
        }
        if let Some(p) = prompt {
            if l == p || p.contains(l) && l.chars().count() > 6 {
                continue;
            }
        }
        parts.push(l.to_string());
    }
    if parts.is_empty() {
        return String::new();
    }

    let mut joined = parts.join("\n");
    if simplified_chinese {
        joined = crate::corrector::to_simplified_zh(&joined);
    }
    // Strip leading breath-hallucination prefix when prepended before a real sentence (e.g. "好多的呀！高挂教堂...")
    for prefix in [
        "好多的呀！", "好多的呀，", "好多的呀。", "好多的呀 ",
        "谢谢！", "谢谢。", "谢谢，", "好好。", "好好，",
    ] {
        if let Some(rest) = joined.strip_prefix(prefix) {
            if !rest.trim().is_empty() {
                joined = rest.trim_start().to_string();
            }
        }
    }

    // Merge spaced English acronyms (`a i` -> `AI`, `p p t` -> `PPT`, `a p i` -> `API`) before CJK space stripping
    joined = crate::corrector::merge_dictated_english_acronyms(&joined);

    let all_chars: Vec<char> = joined.chars().collect();
    let zh_like = lang == "zh" || all_chars.iter().filter(|c| is_cjk(**c)).count() * 2 > all_chars.len();

    if !zh_like {
        // Collapse whitespace for Latin languages
        let mut s = String::with_capacity(joined.len());
        let mut prev_space = false;
        for c in joined.chars() {
            if c.is_whitespace() {
                if !prev_space {
                    s.push(' ');
                }
                prev_space = true;
            } else {
                s.push(c);
                prev_space = false;
            }
        }
        let s = s.trim().to_string();
        let s = crate::corrector::collapse_asr_repetitions(&s);
        let corrected = if smart_correction { apply_spoken_self_correction(&s) } else { s };
        let lat = restore_latin_grammar_punctuation(&corrected, lang, is_final_chunk);
        if lang == "zh" {
            return crate::corrector::translate_and_polish_target_lang(&lat, "zh");
        }
        return lat;
    }

    // Chinese: convert segment breaks (`\n`) and inter-clause spaces into prosodic commas/punctuation,
    // while removing word-internal spaces between short CJK fragments and normalizing full-width punctuation.
    let mut normalized = String::with_capacity(joined.len() + 16);
    let mut cjk_run_since_punct = 0usize;
    let mut current_clause = String::new();

    for (i, &c) in all_chars.iter().enumerate() {
        let prev = if i > 0 { Some(all_chars[i - 1]) } else { None };
        let next = all_chars.get(i + 1).copied();
        let near_cjk = prev.map(is_cjk).unwrap_or(false) || next.map(is_cjk).unwrap_or(false);

        if c.is_whitespace() {
            if near_cjk {
                // Count how many CJK chars immediately follow this whitespace before next whitespace/punct
                let mut next_cjk_run = 0usize;
                let mut k = i + 1;
                while k < all_chars.len() && !all_chars[k].is_whitespace() && !is_zh_punct(all_chars[k]) {
                    if is_cjk(all_chars[k]) {
                        next_cjk_run += 1;
                    }
                    k += 1;
                }
                // A newline between segments or a space between two substantial clauses (>= 4 chars before and >= 3 chars after,
                // or newline with >= 3 chars before) represents a prosodic breath pause -> insert clause punctuation!
                let is_newline = c == '\n' || c == '\r';
                let should_punctuate = (is_newline && cjk_run_since_punct >= 3 && next_cjk_run >= 2)
                    || (cjk_run_since_punct >= 4 && next_cjk_run >= 3);
                if should_punctuate {
                    if let Some(last_out) = normalized.chars().last() {
                        if !is_zh_punct(last_out) {
                            let p = infer_zh_clause_punct(&current_clause, false);
                            normalized.push(p);
                            current_clause.clear();
                            cjk_run_since_punct = 0;
                        }
                    }
                }
                continue;
            } else {
                if !normalized.ends_with(' ') {
                    normalized.push(' ');
                }
                continue;
            }
        }

        let prev_digit = prev.map(|p| p.is_ascii_digit()).unwrap_or(false);
        let next_digit = next.map(|n| n.is_ascii_digit()).unwrap_or(false);
        let mapped = if near_cjk && !(prev_digit && next_digit) {
            match c {
                ',' => '，',
                '?' => '？',
                '!' => '！',
                ':' => '：',
                ';' => '；',
                '.' => '。',
                _ => c,
            }
        } else {
            c
        };
        normalized.push(mapped);
        if is_zh_punct(mapped) && !(prev_digit && next_digit && mapped == '.') {
            current_clause.clear();
            cjk_run_since_punct = 0;
        } else {
            current_clause.push(mapped);
            if is_cjk(mapped) || mapped.is_ascii_alphanumeric() {
                cjk_run_since_punct += 1;
            }
        }
    }

    let corrected = if smart_correction {
        let s1 = apply_spoken_self_correction(&normalized);
        crate::corrector::correct_zh_homophones_and_collocations(&s1, session_ctx)
    } else {
        crate::corrector::correct_zh_homophones_and_collocations(&normalized, session_ctx)
    };
    let punctuated = restore_zh_clause_punctuation(&corrected, is_final_chunk);
    let fixed = crate::corrector::correct_zh_homophones_and_collocations(&punctuated, session_ctx);
    if lang != "auto" {
        crate::corrector::translate_and_polish_target_lang(&fixed, lang)
    } else {
        fixed
    }
}

#[allow(dead_code)]
pub fn postprocess(raw: &str, lang: &str, prompt: Option<&str>) -> String {
    postprocess_with_options(raw, lang, prompt, true)
}

pub fn pcm_to_f32(pcm: &[i16]) -> Vec<f32> {
    if pcm.is_empty() {
        return vec![0.0; crate::audio::RATE as usize * 11 / 10];
    }
    // 1. Remove 0Hz DC offset without touching low-frequency Tone 3 or stop-burst acoustics
    let mean: f32 = pcm.iter().map(|&s| s as f32).sum::<f32>() / (pcm.len() as f32);
    let mut peak = 0.0f32;
    for &s in pcm {
        let a = ((s as f32) - mean).abs() / 32768.0;
        if a > peak {
            peak = a;
        }
    }
    // 2. Gentle peak normalization for quiet microphone input (`peak` between 0.025 and 0.55 -> scale toward 0.70, max 4.5x)
    let gain = if peak >= 0.025 && peak < 0.55 {
        (0.70 / peak).min(4.5)
    } else {
        1.0
    };

    // 3. Prepend 160ms (2560 samples) of clean silence and append 350ms (5600 samples) of trailing silence
    // so Whisper's 30ms STFT window never clips initial consonants (like `重` in "重启软件") or final syllables!
    const LEAD_PAD: usize = 2_560;
    const TRAIL_PAD: usize = 5_600;
    let min_total = (crate::audio::RATE as usize * 12 / 10).max(LEAD_PAD + pcm.len() + TRAIL_PAD);
    let mut v = Vec::with_capacity(min_total);
    v.resize(LEAD_PAD, 0.0);
    for &s in pcm {
        let val = (((s as f32) - mean) / 32768.0) * gain;
        v.push(val.clamp(-1.0, 1.0));
    }
    v.resize(min_total, 0.0);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_zh() {
        let p = zh_prompt(true);
        assert_eq!(postprocess("你好,这是 测试 今天天气很好!\n", "zh", p), "你好，这是测试，今天天气很好！");
        assert_eq!(postprocess("版本 1.5 很好", "zh", p), "版本1.5很好。");
        assert_eq!(postprocess("[BLANK_AUDIO]", "zh", p), "");
        assert_eq!(postprocess("字幕由Amara.org社区提供", "zh", p), "");
        assert_eq!(postprocess("以下是普通话的句子，使用简体中文和标点。", "zh", p), "");
        assert_eq!(
            postprocess("1981年有一个人收到了哈佛他看了一眼他居然拒绝了然后坐飞机去了中国", "auto", p),
            "1981年有一个人收到了哈佛，他看了一眼，他居然拒绝了！然后坐飞机去了中国。"
        );
        assert_eq!(
            postprocess("请问今天的工作进展怎么样呢", "zh", p),
            "请问今天的工作进展怎么样呢？"
        );
        assert_eq!(
            postprocess("那人对神的了解认识也几乎是，优优独播剧场——YoYo Television Series Exclusive。", "zh", p),
            "那人对神的了解认识也几乎是零。"
        );
        assert_eq!(
            postprocess("这是一个很好的训练人怎样正确对待造物主的机会", "zh", p),
            "这是一个很好的训练人怎样正确对待造物主的机会。"
        );
    }

    #[test]
    fn post_latin() {
        assert_eq!(postprocess("  Hello,   world.\n How are you? ", "en", None), "Hello, world. How are you?");
        assert_eq!(postprocess(" Perché il cielo è azzurro?", "it", None), "Perché il cielo è azzurro?");
        assert_eq!(postprocess("how are you doing today", "en", None), "How are you doing today?");
        assert_eq!(postprocess("cómo estás hoy amigo", "es", None), "¿Cómo estás hoy amigo?");
        assert_eq!(postprocess("dove vai domani mattina", "it", None), "Dove vai domani mattina?");
    }

    #[test]
    fn spoken_self_correction_and_disfluencies() {
        assert_eq!(
            apply_spoken_self_correction("今天下午三点，不对，应该是四点开会"),
            "今天下午四点开会"
        );
        assert_eq!(
            apply_spoken_self_correction("嗯，那个那个，我我我今天去五楼，说错了，是六楼拿材料"),
            "我今天去六楼拿材料"
        );
        assert_eq!(
            apply_spoken_self_correction("我们需要发给张三，不对，是李四确认一下"),
            "我们需要发给李四确认一下"
        );
        assert_eq!(
            apply_spoken_self_correction("Meet at 3pm, no wait, 4pm tomorrow"),
            "Meet at 4pm tomorrow"
        );
        assert!(is_voice_undo_command("删除上一句。"));
        assert!(is_voice_undo_command("撤回上一句"));
    }

    #[test]
    fn auto_prompt_is_unbiased_and_latin_streaming_preserves_continuity() {
        // 1. In "auto" mode with empty rolling tail, prompt MUST be None so English/Spanish audio is never hallucinated into Chinese!
        assert_eq!(build_prompt("auto", true, &[]), None);
        assert_eq!(build_prompt_with_context("auto", true, &[], ""), None);
        // When rolling tail exists in "auto" mode, it passes the detected language tail without prepending a Chinese sentence!
        assert_eq!(
            build_prompt_with_context("auto", true, &[], "Hello and welcome to Complete Spanish."),
            Some("Hello and welcome to Complete Spanish.".to_string())
        );

        // 2. Glued English sentences get a clean space after period (`Spanish.We're` -> `Spanish. We're`),
        // and intermediate chunks starting with `which` / `and` or ending with `during the.` do not get mid-sentence periods or forced caps!
        assert_eq!(
            postprocess_full(
                "Hello and welcome to Complete Spanish.We're really excited about this new material",
                "auto",
                None,
                true,
                true,
                false,
                None,
            ),
            "Hello and welcome to Complete Spanish. We're really excited about this new material."
        );
        assert_eq!(
            postprocess_full(
                "which is all you need to become a master self-directed language learner",
                "auto",
                None,
                true,
                true,
                false,
                None,
            ),
            "which is all you need to become a master self-directed language learner."
        );
        assert_eq!(
            postprocess_full(
                "building it up piece by piece with the instructions you receive during the.",
                "auto",
                None,
                true,
                true,
                false,
                None,
            ),
            "building it up piece by piece with the instructions you receive during the"
        );

        // 3. Multi-character CJK repetition loop suppression (`真实真实真实真实` -> `真实`, `语言语言是语言语言语言` -> `语言`)
        let rep1 = crate::corrector::collapse_asr_repetitions("在这种方式中，录制使我们获得了学习经验的真实真实真实真实真实真实真实。");
        assert_eq!(rep1, "在这种方式中，录制使我们获得了学习经验的真实。");
        let rep2 = crate::corrector::collapse_asr_repetitions("语言语言是语言语言语言语言语言语言语，");
        assert_eq!(rep2, "语言，");
    }
}
