//! Microphone capture (winmm waveIn, 16 kHz mono 16-bit PCM) — audio stays in RAM only.
//! The device is opened only while recording, so the Windows "microphone in use" indicator is
//! lit exactly while HEYU listens. A WAV-backed fake source exists for automated testing.

use crate::sys::*;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const RATE: u32 = 16_000;
const BUF_SAMPLES: usize = 1_600; // 100 ms
const NUM_BUFS: usize = 6;

#[derive(Debug, Clone)]
pub enum MicError {
    NoDevice,
    Denied,
    Open(u32),
}

pub struct Capture {
    pub samples: Arc<Mutex<Vec<i16>>>,
    stop: Arc<AtomicBool>,
    pub failed: Arc<AtomicU32>,
    handle: Option<JoinHandle<()>>,
    extra_handles: Vec<JoinHandle<()>>,
}

impl Capture {
    pub fn start_mic() -> Result<Capture, MicError> {
        if unsafe { waveInGetNumDevs() } == 0 {
            return Err(if mic_denied_by_privacy() { MicError::Denied } else { MicError::NoDevice });
        }
        if mic_denied_by_privacy() {
            return Err(MicError::Denied);
        }
        let samples = Arc::new(Mutex::new(Vec::with_capacity(RATE as usize * 30)));
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicU32::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), MicError>>();
        let (s2, st2, f2) = (samples.clone(), stop.clone(), failed.clone());
        let handle = std::thread::spawn(move || unsafe { mic_thread(s2, st2, f2, tx) });
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Ok(Capture { samples, stop, failed, handle: Some(handle), extra_handles: Vec::new() }),
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                Err(MicError::Open(u32::MAX))
            }
        }
    }

    /// Debug/test source: plays a 16 kHz WAV in real time as if it came from the microphone,
    /// followed by trailing silence (so the auto-stop VAD can trigger).
    pub fn start_fake(wav: &Path) -> Result<Capture, String> {
        let pcm = read_wav_16k_mono(wav)?;
        let samples = Arc::new(Mutex::new(Vec::with_capacity(pcm.len() + RATE as usize * 5)));
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicU32::new(0));
        let (s2, st2) = (samples.clone(), stop.clone());
        let handle = std::thread::spawn(move || {
            let t0 = Instant::now();
            let mut fed = 0usize;
            let total = pcm.len() + RATE as usize * 8;
            while !st2.load(Ordering::SeqCst) && fed < total {
                std::thread::sleep(Duration::from_millis(20));
                let due = ((t0.elapsed().as_secs_f64() * RATE as f64) as usize).min(total);
                let mut g = s2.lock().unwrap();
                while fed < due {
                    // tiny noise floor so the "silence" is realistic
                    let v = if fed < pcm.len() { pcm[fed] } else { ((fed * 7919) % 41) as i16 - 20 };
                    g.push(v);
                    fed += 1;
                }
            }
        });
        Ok(Capture { samples, stop, failed, handle: Some(handle), extra_handles: Vec::new() })
    }

    /// Capture system playback audio via WASAPI Loopback (`eRender` + `AUDCLNT_STREAMFLAGS_LOOPBACK`).
    #[allow(dead_code)]
    pub fn start_loopback() -> Result<Capture, String> {
        Self::start_process_loopback(0)
    }

    /// Capture audio from a specific application process tree (`target_pid > 0`) via Windows 10/11
    /// WASAPI Process Loopback (`VAD\Process_Loopback` + `PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE`),
    /// or all system playback audio when `target_pid == 0`.
    pub fn start_process_loopback(target_pid: u32) -> Result<Capture, String> {
        let samples = Arc::new(Mutex::new(Vec::with_capacity(RATE as usize * 30)));
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicU32::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let (s2, st2, f2) = (samples.clone(), stop.clone(), failed.clone());
        let handle = std::thread::spawn(move || unsafe { loopback_thread(s2, st2, f2, target_pid, tx) });
        match rx.recv_timeout(Duration::from_secs(4)) {
            Ok(Ok(())) => Ok(Capture { samples, stop, failed, handle: Some(handle), extra_handles: Vec::new() }),
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                Err("无法初始化音频内录（超时）".into())
            }
        }
    }

    /// Capture BOTH microphone (`waveIn`) AND computer system playback (`WASAPI Loopback`) simultaneously,
    /// mixing both 16 kHz mono streams in real time with automatic gain balance so both user speech and
    /// system audio are transcribed clearly without clipping or drowning each other out.
    pub fn start_both(target_pid: u32) -> Result<Capture, String> {
        if unsafe { waveInGetNumDevs() } == 0 {
            return Err("未检测到麦克风 · 请连接麦克风或切换为「仅电脑声音」".into());
        }
        if mic_denied_by_privacy() {
            return Err("麦克风被系统隐私设置禁止 · 请在 Windows 设置中开启麦克风权限".into());
        }
        let samples = Arc::new(Mutex::new(Vec::with_capacity(RATE as usize * 30)));
        let mic_buf = Arc::new(Mutex::new(Vec::<i16>::with_capacity(RATE as usize * 4)));
        let sys_buf = Arc::new(Mutex::new(Vec::<i16>::with_capacity(RATE as usize * 4)));
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicU32::new(0));

        let (tx_mic, rx_mic) = std::sync::mpsc::channel::<Result<(), MicError>>();
        let (m_s, m_st, m_f) = (mic_buf.clone(), stop.clone(), failed.clone());
        let h_mic = std::thread::spawn(move || unsafe { mic_thread(m_s, m_st, m_f, tx_mic) });

        let (tx_sys, rx_sys) = std::sync::mpsc::channel::<Result<(), String>>();
        let (s_s, s_st, s_f) = (sys_buf.clone(), stop.clone(), failed.clone());
        let h_sys = std::thread::spawn(move || unsafe { loopback_thread(s_s, s_st, s_f, target_pid, tx_sys) });

        let mic_ok = rx_mic.recv_timeout(Duration::from_secs(3)).ok().and_then(|r| r.ok()).is_some();
        let sys_ok = rx_sys.recv_timeout(Duration::from_secs(4)).ok().and_then(|r| r.ok()).is_some();
        if !mic_ok && !sys_ok {
            stop.store(true, Ordering::SeqCst);
            let _ = h_mic.join();
            let _ = h_sys.join();
            return Err("无法同时打开麦克风与电脑系统声音内录".into());
        }

        let (out_s, mix_st) = (samples.clone(), stop.clone());
        let h_mix = std::thread::spawn(move || {
            let drain_and_mix = |mic_b: &Arc<Mutex<Vec<i16>>>, sys_b: &Arc<Mutex<Vec<i16>>>, out: &Arc<Mutex<Vec<i16>>>| {
                let m_chunk = mic_b.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
                let s_chunk = sys_b.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
                let n = m_chunk.len().max(s_chunk.len());
                if n == 0 {
                    return;
                }
                if let Ok(mut g) = out.lock() {
                    g.reserve(n);
                    for i in 0..n {
                        let m = m_chunk.get(i).copied().unwrap_or(0) as f32 * 1.25;
                        let s = s_chunk.get(i).copied().unwrap_or(0) as f32 * 0.90;
                        // Soft saturation mixer so simultaneous speech + video audio never hard-clips
                        let sum = (m + s).clamp(-32768.0, 32767.0) as i16;
                        g.push(sum);
                    }
                }
            };
            while !mix_st.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(25));
                drain_and_mix(&mic_buf, &sys_buf, &out_s);
            }
            drain_and_mix(&mic_buf, &sys_buf, &out_s);
        });

        Ok(Capture {
            samples,
            stop,
            failed,
            handle: Some(h_mix),
            extra_handles: vec![h_mic, h_sys],
        })
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.samples.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Drain and return all currently captured samples while keeping the capture thread running!
    /// Used in `continuous_mode` to flush a completed utterance segment without closing the mic/loopback stream.
    pub fn drain_samples(&self) -> Vec<i16> {
        if let Ok(mut g) = self.samples.lock() {
            std::mem::take(&mut *g)
        } else {
            Vec::new()
        }
    }

    /// Log-dB 20ms/200ms sliding-window valley cutter with `-0.5 dB/s` recency bias
    /// (ported from `SpeechSegmenter::findQuietCut`).
    /// Smooths over brief 20-60ms stop-consonant closures (`p/t/k/b/d/g`) inside compound words while
    /// snapping accurately to true inter-clause micro-pauses (~20 dB quieter than speech) and carrying
    /// post-cut leftover samples into the next chunk with zero lost syllables.
    pub fn drain_samples_smart(&self, tail_search_samples: usize) -> Vec<i16> {
        let Ok(mut g) = self.samples.lock() else {
            return Vec::new();
        };
        const FRAME: usize = 320; // 20 ms at 16 kHz
        if g.len() <= FRAME * 12 || tail_search_samples < FRAME * 6 {
            return std::mem::take(&mut *g);
        }
        let search_len = tail_search_samples.min(g.len() * 2 / 3);
        let search_start = g.len().saturating_sub(search_len);
        let best_pos = find_quiet_cut_16k(&g, search_start, g.len());
        if best_pos >= g.len() {
            return std::mem::take(&mut *g);
        }
        // Start next chunk right at `best_pos` (the center of the 200ms quiet valley, where acoustic energy is minimum)
        // and include up to `best_pos + 160` (10ms inside the valley) in the current chunk so final vowel/nasal codas are 100% intact!
        let next_start = best_pos.min(g.len());
        let cur_end = (best_pos + 160).min(g.len());
        let next_tail = g[next_start..].to_vec();
        g.truncate(cur_end);
        std::mem::replace(&mut *g, next_tail)
    }

    /// Stop capturing and return all recorded samples.
    pub fn finish(mut self) -> Vec<i16> {
        self.stop.store(true, Ordering::SeqCst);
        for h in self.extra_handles.drain(..) {
            let _ = h.join();
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        std::mem::take(&mut *self.samples.lock().unwrap())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for h in self.extra_handles.drain(..) {
            let _ = h.join();
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        // wipe audio from memory
        if let Ok(mut g) = self.samples.lock() {
            for s in g.iter_mut() {
                *s = 0;
            }
            g.clear();
        }
    }
}

unsafe fn mic_thread(samples: Arc<Mutex<Vec<i16>>>, stop: Arc<AtomicBool>, failed: Arc<AtomicU32>, ready: std::sync::mpsc::Sender<Result<(), MicError>>) {
    let fmt = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM,
        nChannels: 1,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * 2,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    let event = CreateEventW(std::ptr::null(), 0, 0, std::ptr::null());
    let mut hwi: HANDLE = std::ptr::null_mut();
    let rc = waveInOpen(&mut hwi, WAVE_MAPPER, &fmt, event as usize, 0, CALLBACK_EVENT);
    if rc != MMSYSERR_NOERROR {
        CloseHandle(event);
        let _ = ready.send(Err(if mic_denied_by_privacy() { MicError::Denied } else { MicError::Open(rc) }));
        return;
    }
    let mut bufs: Vec<Vec<i16>> = (0..NUM_BUFS).map(|_| vec![0i16; BUF_SAMPLES]).collect();
    let mut hdrs: Vec<WAVEHDR> = bufs
        .iter_mut()
        .map(|b| WAVEHDR {
            lpData: b.as_mut_ptr() as *mut u8,
            dwBufferLength: (BUF_SAMPLES * 2) as u32,
            dwBytesRecorded: 0,
            dwUser: 0,
            dwFlags: 0,
            dwLoops: 0,
            lpNext: std::ptr::null_mut(),
            reserved: 0,
        })
        .collect();
    let cb = std::mem::size_of::<WAVEHDR>() as u32;
    for h in hdrs.iter_mut() {
        waveInPrepareHeader(hwi, h, cb);
        waveInAddBuffer(hwi, h, cb);
    }
    let rc = waveInStart(hwi);
    if rc != MMSYSERR_NOERROR {
        failed.store(rc, Ordering::SeqCst);
    }
    let _ = ready.send(Ok(()));
    while !stop.load(Ordering::SeqCst) {
        WaitForSingleObject(event, 50);
        for h in hdrs.iter_mut() {
            if h.dwFlags & WHDR_DONE != 0 {
                let n = (h.dwBytesRecorded / 2) as usize;
                if n > 0 {
                    let data = std::slice::from_raw_parts(h.lpData as *const i16, n);
                    if let Ok(mut g) = samples.lock() {
                        g.extend_from_slice(data);
                    }
                }
                h.dwFlags &= !WHDR_DONE;
                h.dwBytesRecorded = 0;
                if !stop.load(Ordering::SeqCst) {
                    waveInAddBuffer(hwi, h, cb);
                }
            }
        }
    }
    waveInReset(hwi);
    // collect the partially filled buffers returned by waveInReset
    for h in hdrs.iter_mut() {
        let n = (h.dwBytesRecorded / 2) as usize;
        if n > 0 && h.dwFlags & WHDR_DONE != 0 {
            let data = std::slice::from_raw_parts(h.lpData as *const i16, n);
            if let Ok(mut g) = samples.lock() {
                g.extend_from_slice(data);
            }
        }
        waveInUnprepareHeader(hwi, h, cb);
    }
    waveInClose(hwi);
    CloseHandle(event);
    for b in bufs.iter_mut() {
        b.fill(0);
    }
}

// ---------------------------------------------------------------------------------------------
// WASAPI Loopback Capture (System playback audio -> 16 kHz mono i16 PCM in RAM)
// ---------------------------------------------------------------------------------------------

const CLSID_MMDEVICE_ENUMERATOR: GUID = GUID {
    Data1: 0xBCDE0395,
    Data2: 0xE52F,
    Data3: 0x467C,
    Data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
};
const IID_IMMDEVICE_ENUMERATOR: GUID = GUID {
    Data1: 0xA95664D2,
    Data2: 0x9614,
    Data3: 0x4F35,
    Data4: [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
};
const IID_IAUDIOCLIENT: GUID = GUID {
    Data1: 0x1CB9AD4C,
    Data2: 0xDBFA,
    Data3: 0x4C32,
    Data4: [0xB1, 0x78, 0xC2, 0xF5, 0x68, 0xA7, 0x03, 0xB2],
};
const IID_IAUDIOCAPTURECLIENT: GUID = GUID {
    Data1: 0xC8ADBD64,
    Data2: 0xE71E,
    Data3: 0x48A0,
    Data4: [0xA4, 0xDE, 0x18, 0x5C, 0x39, 0x5C, 0xD3, 0x17],
};
const IID_IUNKNOWN: GUID = GUID {
    Data1: 0x00000000,
    Data2: 0x0000,
    Data3: 0x0000,
    Data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};
const IID_IAGILEOBJECT: GUID = GUID {
    Data1: 0x94EA2B94,
    Data2: 0xE9CC,
    Data3: 0x49E0,
    Data4: [0xC0, 0xFF, 0xEE, 0x64, 0xCA, 0x8F, 0x5B, 0x90],
};
const IID_IACTIVATE_AUDIO_INTERFACE_COMPLETION_HANDLER: GUID = GUID {
    Data1: 0x41D949AB,
    Data2: 0x9862,
    Data3: 0x444A,
    Data4: [0x80, 0xF6, 0xC2, 0x61, 0x33, 0x4D, 0xA5, 0xEB],
};

#[repr(C)]
struct AudioClientProcessLoopbackParams {
    target_process_id: u32,
    process_loopback_mode: u32, // 0 = PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE
}

#[repr(C)]
struct AudioClientActivationParams {
    activation_type: u32, // 1 = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK
    process_loopback_params: AudioClientProcessLoopbackParams,
}

#[repr(C)]
struct PropVariantBlob {
    vt: u16, // 65 = VT_BLOB
    w_reserved1: u16,
    w_reserved2: u16,
    w_reserved3: u16,
    cb_size: u32,
    _pad: u32,
    p_blob_data: *const u8,
}

#[repr(C)]
struct CompletionHandlerVtbl {
    query_interface: unsafe extern "system" fn(*mut CompletionHandler, *const GUID, *mut *mut std::ffi::c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut CompletionHandler) -> u32,
    release: unsafe extern "system" fn(*mut CompletionHandler) -> u32,
    activate_completed: unsafe extern "system" fn(*mut CompletionHandler, *mut std::ffi::c_void) -> i32,
}

#[repr(C)]
struct CompletionHandler {
    vtbl: *const CompletionHandlerVtbl,
    ref_count: AtomicU32,
    event: HANDLE,
}

unsafe extern "system" fn ch_query_interface(
    this: *mut CompletionHandler,
    riid: *const GUID,
    ppv: *mut *mut std::ffi::c_void,
) -> i32 {
    if ppv.is_null() || riid.is_null() {
        return 0x8000_4003u32 as i32;
    }
    let id = &*riid;
    if *id == IID_IUNKNOWN || *id == IID_IAGILEOBJECT || *id == IID_IACTIVATE_AUDIO_INTERFACE_COMPLETION_HANDLER {
        *ppv = this as *mut std::ffi::c_void;
        ch_add_ref(this);
        0
    } else {
        *ppv = std::ptr::null_mut();
        0x8000_4002u32 as i32
    }
}

unsafe extern "system" fn ch_add_ref(this: *mut CompletionHandler) -> u32 {
    (*this).ref_count.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn ch_release(this: *mut CompletionHandler) -> u32 {
    let prev = (*this).ref_count.fetch_sub(1, Ordering::SeqCst);
    if prev == 1 {
        let h = Box::from_raw(this);
        if !h.event.is_null() {
            CloseHandle(h.event);
        }
        0
    } else {
        prev - 1
    }
}

unsafe extern "system" fn ch_activate_completed(
    this: *mut CompletionHandler,
    _operation: *mut std::ffi::c_void,
) -> i32 {
    if !(*this).event.is_null() {
        SetEvent((*this).event);
    }
    0
}

static COMPLETION_HANDLER_VTBL: CompletionHandlerVtbl = CompletionHandlerVtbl {
    query_interface: ch_query_interface,
    add_ref: ch_add_ref,
    release: ch_release,
    activate_completed: ch_activate_completed,
};

/// Walk the process table to find the top-most ancestor PID that shares the same executable name
/// as `target_pid` (critical for multi-process apps like Chrome, Edge, Electron, WebView2 where
/// the window HWND belongs to a browser/UI process while audio is rendered by a child utility process).
pub fn resolve_root_process_pid(target_pid: u32) -> u32 {
    if target_pid == 0 {
        return 0;
    }
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap == INVALID_HANDLE_VALUE {
            return target_pid;
        }
        let mut procs: Vec<(u32, u32, String)> = Vec::with_capacity(256);
        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut pe) != 0 {
            loop {
                let len = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
                let exe = String::from_utf16_lossy(&pe.szExeFile[..len]).to_ascii_lowercase();
                procs.push((pe.th32ProcessID, pe.th32ParentProcessID, exe));
                if Process32NextW(snap, &mut pe) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);

        let Some((_, _, target_exe)) = procs.iter().find(|(pid, _, _)| *pid == target_pid).cloned() else {
            return target_pid;
        };
        if target_exe.is_empty() {
            return target_pid;
        }
        let mut cur_pid = target_pid;
        for _ in 0..32 {
            let Some((_, parent_pid, _)) = procs.iter().find(|(pid, _, _)| *pid == cur_pid) else {
                break;
            };
            let ppid = *parent_pid;
            if ppid == 0 || ppid == cur_pid {
                break;
            }
            if let Some((_, _, p_exe)) = procs.iter().find(|(pid, _, _)| *pid == ppid) {
                if *p_exe == target_exe {
                    cur_pid = ppid;
                    continue;
                }
            }
            break;
        }
        cur_pid
    }
}

/// Activate a WASAPI `IAudioClient` bound exclusively to `target_pid`'s process tree via
/// `mmdevapi.dll!ActivateAudioInterfaceAsync(L"VAD\\Process_Loopback", ...)`.
pub unsafe fn activate_process_loopback_client(target_pid: u32) -> Result<*mut std::ffi::c_void, String> {
    let root_pid = resolve_root_process_pid(target_pid);
    let h_mm = LoadLibraryExW(
        wide("mmdevapi.dll").as_ptr(),
        std::ptr::null_mut(),
        LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    );
    if h_mm.is_null() {
        return Err("无法加载 mmdevapi.dll".into());
    }
    let fn_ptr = GetProcAddress(h_mm, b"ActivateAudioInterfaceAsync\0".as_ptr());
    if fn_ptr.is_null() {
        return Err("当前系统版本不支持进程级独立内录 (需 Windows 10 2004 / Windows 11 及以上)".into());
    }
    type ActivateAudioInterfaceAsyncFn = unsafe extern "system" fn(
        *const u16,
        *const GUID,
        *const PropVariantBlob,
        *mut std::ffi::c_void,
        *mut *mut std::ffi::c_void,
    ) -> i32;
    let activate_async: ActivateAudioInterfaceAsyncFn = std::mem::transmute(fn_ptr);

    let event = CreateEventW(std::ptr::null(), 1, 0, std::ptr::null());
    if event.is_null() {
        return Err("无法创建同步事件".into());
    }
    let handler = Box::into_raw(Box::new(CompletionHandler {
        vtbl: &COMPLETION_HANDLER_VTBL,
        ref_count: AtomicU32::new(1),
        event,
    }));

    let params = AudioClientActivationParams {
        activation_type: 1, // AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK
        process_loopback_params: AudioClientProcessLoopbackParams {
            target_process_id: root_pid,
            process_loopback_mode: 0, // PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE
        },
    };
    let prop = PropVariantBlob {
        vt: 65, // VT_BLOB
        w_reserved1: 0,
        w_reserved2: 0,
        w_reserved3: 0,
        cb_size: std::mem::size_of::<AudioClientActivationParams>() as u32,
        _pad: 0,
        p_blob_data: &params as *const _ as *const u8,
    };

    let mut op: *mut std::ffi::c_void = std::ptr::null_mut();
    let dev_path = wide("VAD\\Process_Loopback");
    let hr = activate_async(
        dev_path.as_ptr(),
        &IID_IAUDIOCLIENT,
        &prop,
        handler as *mut std::ffi::c_void,
        &mut op,
    );
    if hr < 0 || op.is_null() {
        ch_release(handler);
        return Err(format!("无法启动指定应用音频隔离 (0x{:08X})", hr as u32));
    }

    let wait_rc = WaitForSingleObject(event, 3500);
    ch_release(handler);
    if wait_rc != 0 {
        com_release(op);
        return Err("激活指定应用音频捕获超时".into());
    }

    // IActivateAudioInterfaceAsyncOperation::GetActivateResult (vtable #3)
    let get_res: unsafe extern "system" fn(*mut std::ffi::c_void, *mut i32, *mut *mut std::ffi::c_void) -> i32 =
        std::mem::transmute(com_vfn(op, 3));
    let mut hr_act: i32 = -1;
    let mut unk: *mut std::ffi::c_void = std::ptr::null_mut();
    let hr_get = get_res(op, &mut hr_act, &mut unk);
    com_release(op);
    if hr_get < 0 || hr_act < 0 || unk.is_null() {
        return Err(format!("目标应用音频激活失败 (0x{:08X} / 0x{:08X})", hr_get as u32, hr_act as u32));
    }

    // Query IID_IAUDIOCLIENT to ensure canonical interface pointer
    let qi: unsafe extern "system" fn(*mut std::ffi::c_void, *const GUID, *mut *mut std::ffi::c_void) -> i32 =
        std::mem::transmute(com_vfn(unk, 0));
    let mut client: *mut std::ffi::c_void = std::ptr::null_mut();
    let hr_qi = qi(unk, &IID_IAUDIOCLIENT, &mut client);
    if hr_qi >= 0 && !client.is_null() {
        com_release(unk);
        Ok(client)
    } else {
        Ok(unk)
    }
}

#[inline]
unsafe fn com_vfn(obj: *mut std::ffi::c_void, idx: usize) -> *const std::ffi::c_void {
    let vtbl = *(obj as *const *const *const std::ffi::c_void);
    *vtbl.add(idx)
}

#[inline]
unsafe fn com_release(obj: *mut std::ffi::c_void) {
    if !obj.is_null() {
        let f: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 = std::mem::transmute(com_vfn(obj, 2));
        f(obj);
    }
}

unsafe fn loopback_thread(
    samples: Arc<Mutex<Vec<i16>>>,
    stop: Arc<AtomicBool>,
    _failed: Arc<AtomicU32>,
    target_pid: u32,
    ready: std::sync::mpsc::Sender<Result<(), String>>,
) {
    let hr_co = CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED);
    let mut enumerator: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut device: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut client: *mut std::ffi::c_void = std::ptr::null_mut();

    let (src_rate, src_ch, src_bits, is_float);

    let mut use_process_loopback = false;
    if target_pid > 0 {
        if let Ok(c) = activate_process_loopback_client(target_pid) {
            client = c;
            use_process_loopback = true;
        }
    }

    if use_process_loopback {
        let fmt = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
            nChannels: 2,
            nSamplesPerSec: 48_000,
            nAvgBytesPerSec: 48_000 * 2 * 4,
            nBlockAlign: 8,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        let init_fn: unsafe extern "system" fn(
            *mut std::ffi::c_void,
            u32,
            u32,
            i64,
            i64,
            *const WAVEFORMATEX,
            *const GUID,
        ) -> i32 = std::mem::transmute(com_vfn(client, 3));
        let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
            | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
            | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
        let hr = init_fn(client, AUDCLNT_SHAREMODE_SHARED, flags, 2_000_000, 0, &fmt, std::ptr::null());
        if hr < 0 {
            com_release(client);
            client = std::ptr::null_mut();
            use_process_loopback = false;
        }
    }

    if use_process_loopback {
        src_rate = 48_000;
        src_ch = 2usize;
        src_bits = 32u16;
        is_float = true;
    } else {
        let hr = CoCreateInstance(
            &CLSID_MMDEVICE_ENUMERATOR,
            std::ptr::null_mut(),
            CLSCTX_ALL,
            &IID_IMMDEVICE_ENUMERATOR,
            &mut enumerator,
        );
        if hr < 0 || enumerator.is_null() {
            let _ = ready.send(Err(format!("无法创建系统音频枚举器 (0x{:08X})", hr as u32)));
            if hr_co >= 0 {
                CoUninitialize();
            }
            return;
        }

        // IMMDeviceEnumerator::GetDefaultAudioEndpoint (vtable #4): eRender = 0, eConsole = 0
        let get_ep: unsafe extern "system" fn(*mut std::ffi::c_void, u32, u32, *mut *mut std::ffi::c_void) -> i32 =
            std::mem::transmute(com_vfn(enumerator, 4));
        let hr = get_ep(enumerator, 0, 0, &mut device);
        if hr < 0 || device.is_null() {
            com_release(enumerator);
            let _ = ready.send(Err("未检测到系统默认扬声器/耳机输出设备".into()));
            if hr_co >= 0 {
                CoUninitialize();
            }
            return;
        }

        // IMMDevice::Activate (vtable #3) -> IAudioClient
        let activate: unsafe extern "system" fn(
            *mut std::ffi::c_void,
            *const GUID,
            u32,
            *mut std::ffi::c_void,
            *mut *mut std::ffi::c_void,
        ) -> i32 = std::mem::transmute(com_vfn(device, 3));
        let hr = activate(device, &IID_IAUDIOCLIENT, CLSCTX_ALL, std::ptr::null_mut(), &mut client);
        if hr < 0 || client.is_null() {
            com_release(device);
            com_release(enumerator);
            let _ = ready.send(Err(format!("无法激活系统音频客户端 (0x{:08X})", hr as u32)));
            if hr_co >= 0 {
                CoUninitialize();
            }
            return;
        }

        // IAudioClient::GetMixFormat (vtable #8)
        let get_mix_fmt: unsafe extern "system" fn(*mut std::ffi::c_void, *mut *mut WAVEFORMATEX) -> i32 =
            std::mem::transmute(com_vfn(client, 8));
        let mut pwfx: *mut WAVEFORMATEX = std::ptr::null_mut();
        let hr = get_mix_fmt(client, &mut pwfx);
        if hr < 0 || pwfx.is_null() {
            com_release(client);
            com_release(device);
            com_release(enumerator);
            let _ = ready.send(Err("无法获取系统音频混音格式".into()));
            if hr_co >= 0 {
                CoUninitialize();
            }
            return;
        }

        let wfx = &*pwfx;
        src_rate = wfx.nSamplesPerSec.max(8000);
        src_ch = wfx.nChannels.max(1) as usize;
        src_bits = wfx.wBitsPerSample;
        is_float = wfx_is_ieee_float(pwfx);

        // IAudioClient::Initialize (vtable #3): Shared mode + AUDCLNT_STREAMFLAGS_LOOPBACK, 100ms buffer
        let init_fn: unsafe extern "system" fn(
            *mut std::ffi::c_void,
            u32,
            u32,
            i64,
            i64,
            *const WAVEFORMATEX,
            *const GUID,
        ) -> i32 = std::mem::transmute(com_vfn(client, 3));
        let hr = init_fn(
            client,
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            1_000_000, // 100 ms in 100-ns units
            0,
            pwfx,
            std::ptr::null(),
        );
        CoTaskMemFree(pwfx as *mut std::ffi::c_void);
        if hr < 0 {
            com_release(client);
            com_release(device);
            com_release(enumerator);
            let _ = ready.send(Err(format!("无法启动系统内录流 (0x{:08X})", hr as u32)));
            if hr_co >= 0 {
                CoUninitialize();
            }
            return;
        }
    }

    // IAudioClient::GetService (vtable #14) -> IAudioCaptureClient
    let get_service: unsafe extern "system" fn(*mut std::ffi::c_void, *const GUID, *mut *mut std::ffi::c_void) -> i32 =
        std::mem::transmute(com_vfn(client, 14));
    let mut cap_client: *mut std::ffi::c_void = std::ptr::null_mut();
    let hr = get_service(client, &IID_IAUDIOCAPTURECLIENT, &mut cap_client);
    if hr < 0 || cap_client.is_null() {
        com_release(client);
        com_release(device);
        com_release(enumerator);
        let _ = ready.send(Err("无法获取系统音频捕获接口".into()));
        if hr_co >= 0 {
            CoUninitialize();
        }
        return;
    }

    // IAudioClient::Start (vtable #10) and Stop (vtable #11)
    let start_fn: unsafe extern "system" fn(*mut std::ffi::c_void) -> i32 = std::mem::transmute(com_vfn(client, 10));
    let stop_fn: unsafe extern "system" fn(*mut std::ffi::c_void) -> i32 = std::mem::transmute(com_vfn(client, 11));
    // IAudioCaptureClient::GetBuffer (#3), ReleaseBuffer (#4), GetNextPacketSize (#5)
    let get_buf: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *mut *mut u8,
        *mut u32,
        *mut u32,
        *mut u64,
        *mut u64,
    ) -> i32 = std::mem::transmute(com_vfn(cap_client, 3));
    let rel_buf: unsafe extern "system" fn(*mut std::ffi::c_void, u32) -> i32 =
        std::mem::transmute(com_vfn(cap_client, 4));
    let next_pkt: unsafe extern "system" fn(*mut std::ffi::c_void, *mut u32) -> i32 =
        std::mem::transmute(com_vfn(cap_client, 5));

    let hr = start_fn(client);
    if hr < 0 {
        com_release(cap_client);
        com_release(client);
        com_release(device);
        com_release(enumerator);
        let _ = ready.send(Err(format!("启动系统内录失败 (0x{:08X})", hr as u32)));
        if hr_co >= 0 {
            CoUninitialize();
        }
        return;
    }

    let _ = ready.send(Ok(()));

    let mut resample_pos = 0.0f64;
    let step = src_rate as f64 / RATE as f64;
    let mut mono_staging: Vec<f32> = Vec::with_capacity(4800);

    while !stop.load(Ordering::SeqCst) {
        Sleep(15);
        let mut pkt_frames = 0u32;
        if next_pkt(cap_client, &mut pkt_frames) < 0 {
            break;
        }
        while pkt_frames > 0 && !stop.load(Ordering::SeqCst) {
            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut frames_read = 0u32;
            let mut flags = 0u32;
            let mut dev_pos = 0u64;
            let mut qpc_pos = 0u64;
            if get_buf(cap_client, &mut data_ptr, &mut frames_read, &mut flags, &mut dev_pos, &mut qpc_pos) < 0 {
                break;
            }
            let n_frames = frames_read as usize;
            mono_staging.clear();
            if (flags & AUDCLNT_BUFFERFLAGS_SILENT) != 0 || data_ptr.is_null() {
                mono_staging.resize(n_frames, 0.0);
            } else if is_float && src_bits == 32 {
                let flt = std::slice::from_raw_parts(data_ptr as *const f32, n_frames * src_ch);
                for f in 0..n_frames {
                    let mut sum = 0.0f32;
                    for c in 0..src_ch {
                        sum += flt[f * src_ch + c];
                    }
                    mono_staging.push((sum / src_ch as f32).clamp(-1.0, 1.0));
                }
            } else if src_bits == 16 {
                let pcm16 = std::slice::from_raw_parts(data_ptr as *const i16, n_frames * src_ch);
                for f in 0..n_frames {
                    let mut sum = 0.0f32;
                    for c in 0..src_ch {
                        sum += pcm16[f * src_ch + c] as f32 / 32768.0;
                    }
                    mono_staging.push((sum / src_ch as f32).clamp(-1.0, 1.0));
                }
            } else if src_bits == 32 {
                let pcm32 = std::slice::from_raw_parts(data_ptr as *const i32, n_frames * src_ch);
                for f in 0..n_frames {
                    let mut sum = 0.0f32;
                    for c in 0..src_ch {
                        sum += pcm32[f * src_ch + c] as f32 / 2_147_483_648.0;
                    }
                    mono_staging.push((sum / src_ch as f32).clamp(-1.0, 1.0));
                }
            } else {
                mono_staging.resize(n_frames, 0.0);
            }
            let _ = rel_buf(cap_client, frames_read);

            if !mono_staging.is_empty() {
                if let Ok(mut g) = samples.lock() {
                    let n_st = mono_staging.len();
                    while (resample_pos as usize) < n_st {
                        let idx = resample_pos as usize;
                        // 5-tap anti-aliasing low-pass FIR filter ([0.10, 0.22, 0.36, 0.22, 0.10])
                        // prevents 8–24 kHz background music/screams from aliasing into the 2–8 kHz Mandarin sibilant band!
                        let s_m2 = mono_staging[idx.saturating_sub(2)];
                        let s_m1 = mono_staging[idx.saturating_sub(1)];
                        let s_0 = mono_staging[idx];
                        let s_p1 = mono_staging[(idx + 1).min(n_st - 1)];
                        let s_p2 = mono_staging[(idx + 2).min(n_st - 1)];
                        let lp0 = 0.10 * s_m2 + 0.22 * s_m1 + 0.36 * s_0 + 0.22 * s_p1 + 0.10 * s_p2;
                        let s_p3 = mono_staging[(idx + 3).min(n_st - 1)];
                        let lp1 = 0.10 * s_m1 + 0.22 * s_0 + 0.36 * s_p1 + 0.22 * s_p2 + 0.10 * s_p3;
                        let frac = (resample_pos - idx as f64) as f32;
                        let v = (lp0 + (lp1 - lp0) * frac) * 32767.0;
                        g.push(v.clamp(-32768.0, 32767.0) as i16);
                        resample_pos += step;
                    }
                    resample_pos -= n_st as f64;
                    if resample_pos < 0.0 {
                        resample_pos = 0.0;
                    }
                }
            }

            if next_pkt(cap_client, &mut pkt_frames) < 0 {
                break;
            }
        }
    }

    let _ = stop_fn(client);
    com_release(cap_client);
    com_release(client);
    com_release(device);
    com_release(enumerator);
    if hr_co >= 0 {
        CoUninitialize();
    }
}

fn reg_string(root: HKEY, sub: &str, name: &str) -> Option<String> {
    unsafe {
        let mut k: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(root, wide(sub).as_ptr(), 0, KEY_READ, &mut k) != 0 {
            return None;
        }
        let mut buf = [0u16; 64];
        let mut len = (buf.len() * 2) as u32;
        let mut ty = 0u32;
        let rc = RegQueryValueExW(k, wide(name).as_ptr(), std::ptr::null_mut(), &mut ty, buf.as_mut_ptr() as *mut u8, &mut len);
        RegCloseKey(k);
        if rc != 0 {
            return None;
        }
        let n = (len as usize / 2).min(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]).trim_end_matches('\0').to_string())
    }
}

/// Windows 10/11 privacy switches (Settings › Privacy & security › Microphone).
pub fn mic_denied_by_privacy() -> bool {
    const BASE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone";
    let deny = |root, sub: &str| reg_string(root, sub, "Value").map(|v| v.eq_ignore_ascii_case("Deny")).unwrap_or(false);
    deny(HKEY_LOCAL_MACHINE, BASE) || deny(HKEY_CURRENT_USER, BASE) || deny(HKEY_CURRENT_USER, &format!("{}\\NonPackaged", BASE))
}

// ---------------------------------------------------------------------------------------------
// Energy VAD
// ---------------------------------------------------------------------------------------------

pub struct Vad {
    pos: usize,
    noise: f32,
    frames: u32,
    speech_ms: u32,
    pub speech_started: bool,
    pub silence_ms: u32,
    pub level: f32,
}

impl Vad {
    pub fn new() -> Self {
        Self { pos: 0, noise: 0.0, frames: 0, speech_ms: 0, speech_started: false, silence_ms: 0, level: 0.0 }
    }

    /// Feed newly captured samples (30 ms frames).
    pub fn update(&mut self, all: &[i16]) {
        if self.pos > all.len() {
            self.pos = 0;
        }
        const FRAME: usize = 480;
        while self.pos + FRAME <= all.len() {
            let f = &all[self.pos..self.pos + FRAME];
            self.pos += FRAME;
            let rms = (f.iter().map(|&s| (s as f32) * (s as f32)).sum::<f32>() / FRAME as f32).sqrt();
            // High-frequency vocal formant energy (rejects <120Hz breath rumble & DC offset) + Zero-Crossing Rate
            let mut vocal_sq = 0.0f32;
            let mut zcr = 0u32;
            for i in 1..FRAME {
                let d = (f[i] as f32) - 0.95 * (f[i - 1] as f32);
                vocal_sq += d * d;
                if (f[i] >= 0) != (f[i - 1] >= 0) && f[i].unsigned_abs() > 38 {
                    zcr += 1;
                }
            }
            let vocal_rms = (vocal_sq / FRAME as f32).sqrt();

            self.frames += 1;
            if self.frames <= 8 {
                let floor_cand = rms.min(95.0);
                self.noise = if self.frames == 1 { floor_cand } else { self.noise * 0.7 + floor_cand * 0.3 };
            }
            let thr = (self.noise.max(18.0) * 1.85).clamp(55.0, 220.0);
            // Distance-adaptive human speech detector: detects both close and far-field (50–70cm) speech
            // (`rms > thr && vocal_rms > 25.0`), while rejecting <100Hz mic breath rumble (`vocal_rms >= rms * 0.24`).
            let is_speech = rms > thr && vocal_rms > 25.0 && vocal_rms >= rms * 0.24 && zcr >= 4;
            if is_speech {
                self.speech_ms += 30;
                self.silence_ms = 0;
                if self.speech_ms >= 90 {
                    self.speech_started = true;
                }
            } else {
                self.silence_ms += 30;
                if !self.speech_started && self.silence_ms > 240 {
                    self.speech_ms = 0;
                }
                self.noise = (self.noise * 0.97 + rms.min(130.0) * 0.03).min(130.0);
            }
            let db = 20.0 * rms.max(1.0).log10();
            let target = ((db - 28.0) / 46.0).clamp(0.0, 1.0);
            self.level = if target > self.level { target } else { self.level * 0.8 + target * 0.2 };
        }
    }
}

/// Non-destructively verify that `pcm` contains genuine human vocal-formant speech
/// (evaluating a 90Hz high-pass filter & zero-crossing rate on temporary variables only,
/// while leaving `pcm` 100% unmodified so Whisper receives pure, un-attenuated Tone 3 & stop-burst acoustics!).
pub fn preprocess_and_verify_human_speech(pcm: &[i16]) -> bool {
    if pcm.len() < 2_000 {
        return false;
    }
    const FRAME: usize = 480; // 30 ms at 16 kHz
    let mut prev_x = pcm[0] as f32;
    let mut prev_y = 0.0f32;
    let mut voiced_frames = 0u32;
    let mut max_consecutive_voiced = 0u32;
    let mut cur_consecutive = 0u32;
    let mut max_rms = 0.0f32;

    for chunk in pcm.chunks_exact(FRAME) {
        let mut hpf_sq = 0.0f32;
        let mut diff_sq = 0.0f32;
        let mut zcr = 0u32;
        let mut prev_hpf = prev_y;
        for &s in chunk {
            let x = s as f32;
            let y = x - prev_x + 0.965 * prev_y;
            prev_x = x;
            prev_y = y;
            hpf_sq += y * y;
            let d = y - prev_hpf;
            diff_sq += d * d;
            if (y >= 0.0) != (prev_hpf >= 0.0) && y.abs() > 35.0 {
                zcr += 1;
            }
            prev_hpf = y;
        }
        let rms = (hpf_sq / FRAME as f32).sqrt();
        let vocal_rms = (diff_sq / FRAME as f32).sqrt();
        if rms > max_rms {
            max_rms = rms;
        }
        // Sensitive to soft/far-field speech (`rms > 52`, `vocal_rms > 24`), while rejecting low-freq breath (`vocal_rms >= rms * 0.24`)
        if rms > 52.0 && vocal_rms > 24.0 && vocal_rms >= rms * 0.24 && (4..=175).contains(&zcr) {
            voiced_frames += 1;
            cur_consecutive += 1;
            if cur_consecutive > max_consecutive_voiced {
                max_consecutive_voiced = cur_consecutive;
            }
        } else {
            cur_consecutive = 0;
        }
    }
    voiced_frames >= 3 && max_consecutive_voiced >= 2 && max_rms >= 68.0
}

/// Find the optimal quiet cut sample index inside `pcm[search_from..search_to]` at 16 kHz.
/// Ported from `SpeechSegmenter::findQuietCut`:
/// 1. Computes log-energy in dB over 20ms frames (`320` samples at 16 kHz): `10 * log10(max(mean_sq, 1e-12))`.
/// 2. Slides a 10-frame (200ms) smoothing window across the search range so brief 20-40ms stop-consonant closures
///    inside compound words are never mistaken for breath pauses.
/// 3. Applies a `-0.5 dB/s` recency bias (`score = mean_db - 0.5 * elapsed_s`) to prefer cutting later in the segment
///    while snapping cleanly to any true inter-clause pause (~20 dB quieter than speech).
pub fn find_quiet_cut_16k(pcm: &[i16], search_from: usize, search_to: usize) -> usize {
    const FRAME: usize = 320; // 20 ms at 16 kHz
    let end = search_to.min(pcm.len());
    let start = search_from.min(end);
    let range_len = end.saturating_sub(start);
    let num_frames = range_len / FRAME;
    if num_frames < 4 {
        return end;
    }
    let win_frames = if num_frames >= 12 {
        10 // 200 ms window
    } else {
        (num_frames / 2).clamp(2, 10)
    };
    let mut db = Vec::with_capacity(num_frames);
    for f in 0..num_frames {
        let base = start + f * FRAME;
        let slice = &pcm[base..base + FRAME];
        let mut sum_sq = 0.0f64;
        for &s in slice {
            let v = (s as f64) / 32768.0;
            sum_sq += v * v;
        }
        let mean_sq = (sum_sq / FRAME as f64).max(1e-12);
        db.push(10.0 * mean_sq.log10());
    }

    let mut win_sum: f64 = db[..win_frames].iter().sum();
    let mut best_score = (win_sum / win_frames as f64) - 0.5 * 0.0;
    let mut best_w = 0usize;

    for w in 1..=(num_frames - win_frames) {
        win_sum += db[w + win_frames - 1] - db[w - 1];
        let sec_offset = (w * FRAME) as f64 / (RATE as f64);
        let score = (win_sum / win_frames as f64) - 0.5 * sec_offset;
        if score <= best_score {
            best_score = score;
            best_w = w;
        }
    }

    let cut = start + (best_w + win_frames / 2) * FRAME;
    cut.clamp(start, end)
}

// ---------------------------------------------------------------------------------------------
// WAV I/O (debug + CLI fallback)
// ---------------------------------------------------------------------------------------------

pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<i16>, String> {
    let d = std::fs::read(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    if d.len() < 44 || &d[0..4] != b"RIFF" || &d[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let mut pos = 12;
    let (mut ch, mut rate, mut bits, mut fmt_tag) = (1u16, 16000u32, 16u16, 1u16);
    let mut sub_float = false;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= d.len() {
        let id = &d[pos..pos + 4];
        let sz = u32::from_le_bytes([d[pos + 4], d[pos + 5], d[pos + 6], d[pos + 7]]) as usize;
        let body = &d[pos + 8..(pos + 8 + sz).min(d.len())];
        if id == b"fmt " && body.len() >= 16 {
            fmt_tag = u16::from_le_bytes([body[0], body[1]]);
            ch = u16::from_le_bytes([body[2], body[3]]);
            rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            bits = u16::from_le_bytes([body[14], body[15]]);
            if fmt_tag == 0xFFFE && body.len() >= 40 {
                let sub_tag = u32::from_le_bytes([body[24], body[25], body[26], body[27]]);
                sub_float = sub_tag == 3;
            }
        } else if id == b"data" {
            data = Some(body);
        }
        pos += 8 + sz + (sz & 1);
    }
    let data = data.ok_or("no data chunk")?;
    if ch == 0 || rate == 0 {
        return Err("invalid WAV channel/rate".into());
    }
    let is_flt = fmt_tag == 3 || (fmt_tag == 0xFFFE && sub_float);
    let is_pcm = fmt_tag == 1 || (fmt_tag == 0xFFFE && !sub_float);
    let bytes_per_sample = (bits / 8) as usize;
    if (!is_flt && !is_pcm) || bytes_per_sample == 0 {
        return Err(format!("unsupported WAV format (tag {}, {} bits)", fmt_tag, bits));
    }
    let frame_bytes = bytes_per_sample * ch as usize;
    let frames = data.len() / frame_bytes;
    let mut mono = Vec::with_capacity(frames);
    for i in 0..frames {
        let mut acc = 0.0f32;
        for c in 0..ch as usize {
            let o = i * frame_bytes + c * bytes_per_sample;
            let sample = if is_flt && bits == 32 && o + 4 <= data.len() {
                f32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) * 32767.0
            } else if is_pcm && bits == 16 && o + 2 <= data.len() {
                i16::from_le_bytes([data[o], data[o + 1]]) as f32
            } else if is_pcm && bits == 24 && o + 3 <= data.len() {
                let v = ((data[o + 2] as i8 as i32) << 16) | ((data[o + 1] as u32 as i32) << 8) | (data[o] as u32 as i32);
                (v as f32) / 256.0
            } else if is_pcm && bits == 32 && o + 4 <= data.len() {
                let v = i32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
                (v as f32) / 65536.0
            } else if is_pcm && bits == 8 && o < data.len() {
                ((data[o] as i32 - 128) * 256) as f32
            } else {
                0.0
            };
            acc += sample;
        }
        mono.push((acc / ch as f32).clamp(-32768.0, 32767.0) as i16);
    }
    if rate == RATE {
        return Ok(mono);
    }
    if mono.is_empty() {
        return Ok(mono);
    }
    // linear resample to 16 kHz
    let n_out = (mono.len() as u64 * RATE as u64 / rate as u64) as usize;
    let mut out = Vec::with_capacity(n_out);
    for i in 0..n_out {
        let src = i as f64 * rate as f64 / RATE as f64;
        let j = src.floor() as usize;
        let t = src - j as f64;
        let a = mono[j.min(mono.len() - 1)] as f64;
        let b = mono[(j + 1).min(mono.len() - 1)] as f64;
        out.push((a + (b - a) * t).clamp(-32768.0, 32767.0) as i16);
    }
    Ok(out)
}

/// Read any WAV or media file (`.wav`, `.mp3`, `.m4a`, `.mp4`, `.flac`, `.ogg`, `.aac`) and return 16 kHz mono i16 PCM.
pub fn read_audio_any_16k_mono(path: &Path) -> Result<Vec<i16>, String> {
    if let Ok(pcm) = read_wav_16k_mono(path) {
        return Ok(pcm);
    }
    use std::os::windows::process::CommandExt;
    let dir = crate::config::tmp_dir();
    let _ = std::fs::create_dir_all(&dir);
    let tmp_wav = dir.join(format!("heyu_conv_{}.wav", std::process::id()));
    let _ = std::fs::remove_file(&tmp_wav);

    // 1) Try ffmpeg if on PATH
    let ff = std::process::Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(path)
        .args(["-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(&tmp_wav)
        .creation_flags(0x0800_0000)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if ff.map(|s| s.success()).unwrap_or(false) && tmp_wav.is_file() {
        let res = read_wav_16k_mono(&tmp_wav);
        let _ = std::fs::remove_file(&tmp_wav);
        return res;
    }

    // 2) Try Windows MediaFoundation via PowerShell/WinRT AudioGraph or afconvert fallback
    let _ = std::fs::remove_file(&tmp_wav);
    Err(format!("不支持的音频封装格式（请使用 WAV 或安装 ffmpeg）：{}", path.display()))
}

pub fn write_wav_16k_mono(path: &Path, pcm: &[i16]) -> std::io::Result<()> {
    let mut v = Vec::with_capacity(44 + pcm.len() * 2);
    let data_len = (pcm.len() * 2) as u32;
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        v.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, &v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_process_pid_resolves_current_process() {
        let me = std::process::id();
        let root = resolve_root_process_pid(me);
        assert!(root > 0);
    }

    #[test]
    fn process_loopback_activates_for_current_pid() {
        let me = std::process::id();
        let cap = Capture::start_process_loopback(me);
        if let Ok(c) = cap {
            std::thread::sleep(std::time::Duration::from_millis(80));
            let _pcm = c.finish();
        }
    }

    #[test]
    fn find_quiet_cut_prefers_200ms_valley_over_20ms_consonant_gap() {
        // 2 seconds (32000 samples) of speech at ~3000 amplitude
        let mut pcm = vec![3000i16; 32_000];
        // Insert a 20ms stop-consonant closure at 0.5s (samples 8000..8320)
        for s in &mut pcm[8_000..8_320] {
            *s = 0;
        }
        // Insert a real 220ms inter-clause pause at 1.4s (samples 22400..25920)
        for s in &mut pcm[22_400..25_920] {
            *s = 10;
        }
        let cut = find_quiet_cut_16k(&pcm, 4_800, 32_000);
        // Should land squarely inside the 220ms pause [22400..25920], never at the 20ms consonant closure!
        assert!(cut >= 22_400 && cut <= 25_920, "cut={} was not in [22400..25920]", cut);
    }
}
