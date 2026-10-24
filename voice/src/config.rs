//! voice_config.json + data-directory resolution + tiny flat-JSON reader (std only).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const CONFIG_NAME: &str = "voice_config.json";

pub const KNOWN_MODELS: [(&str, u64, &str); 6] = [
    ("large-v3-q5_0", 1081140203, "d75795ecff3f83b5faa89d1900604ad8c780abd5739fae406de19f23ecd98ad1"),
    ("large-v3", 3095033483, "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2"),
    ("large-v3-turbo-q5_0", 574041195, "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2"),
    ("medium-q5_0", 539212467, "19fea4b380c3a618ec4723c3eef2eb785ffba0d0538cf43f8f235e7b3b34220f"),
    ("small-q5_1", 190085487, "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb"),
    ("base-q5_1", 59707625, "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct VoiceConfig {
    pub enabled: bool,
    pub hotkey: String,
    pub loopback_hotkey: String,
    /// "mic" | "system" | "both"
    pub audio_source: String,
    /// 0 = all system audio, >0 = isolate specific application process tree via WASAPI Process Loopback
    pub target_pid: u32,
    /// "follow" | "independent"
    pub cursor_mode: String,
    /// auto (default: transcribe in original heard language) | zh | en | it | es (transcribe & translate into target language) | follow
    pub language: String,
    pub model: String,
    /// auto | true | false
    pub use_gpu: String,
    pub auto_stop_silence_ms: u32,
    pub max_record_s: u32,
    pub simplified_chinese: bool,
    pub continuous_mode: bool,
    pub smart_correction: bool,
    /// User-defined voice replacement rules (`wrong=>correct; 错词1,错词2=>正词`)
    pub replace_rules: String,
    /// Automatically unload the Whisper model from RAM/VRAM after N minutes of idle (0 = disabled, default 30)
    pub idle_unload_mins: u32,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            hotkey: "Ctrl+Alt+V".into(),
            loopback_hotkey: "Ctrl+Alt+B".into(),
            audio_source: "mic".into(),
            target_pid: 0,
            cursor_mode: "follow".into(),
            language: "auto".into(),
            model: "large-v3-q5_0".into(),
            use_gpu: "auto".into(),
            auto_stop_silence_ms: 1200,
            max_record_s: 1200,
            simplified_chinese: true,
            continuous_mode: true,
            smart_correction: true,
            replace_rules: String::new(),
            idle_unload_mins: 30,
        }
    }
}

impl VoiceConfig {
    pub fn parse(txt: &str) -> Self {
        let d = Self::default();
        let lang = json_str(txt, "language").unwrap_or(d.language.clone()).to_ascii_lowercase();
        let lang = if ["follow", "auto", "zh", "en", "it", "es"].contains(&lang.as_str()) { lang } else { d.language.clone() };
        let audio_source = json_str(txt, "audio_source")
            .map(|s| match s.to_ascii_lowercase().as_str() {
                "system" | "loopback" => "system".to_string(),
                "both" | "mix" | "mixed" => "both".to_string(),
                _ => "mic".to_string(),
            })
            .unwrap_or(d.audio_source.clone());
        let cursor_mode = json_str(txt, "cursor_mode")
            .map(|s| match s.to_ascii_lowercase().as_str() {
                "follow" => "follow".to_string(),
                _ => "independent".to_string(),
            })
            .unwrap_or(d.cursor_mode.clone());
        let use_gpu = match json_raw(txt, "use_gpu").as_deref() {
            Some("true") => "true".to_string(),
            Some("false") => "false".to_string(),
            _ => json_str(txt, "use_gpu")
                .map(|s| match s.to_ascii_lowercase().as_str() {
                    "on" | "true" => "true".to_string(),
                    "off" | "false" => "false".to_string(),
                    "auto" => "auto".to_string(),
                    _ => d.use_gpu.clone(),
                })
                .unwrap_or(d.use_gpu.clone()),
        };
        let model = json_str(txt, "model").filter(|m| is_safe_model_name(m)).unwrap_or(d.model.clone());
        Self {
            enabled: json_bool(txt, "enabled").unwrap_or(d.enabled),
            hotkey: json_str(txt, "hotkey").filter(|h| !h.trim().is_empty()).unwrap_or(d.hotkey),
            loopback_hotkey: json_str(txt, "loopback_hotkey").filter(|h| !h.trim().is_empty()).unwrap_or(d.loopback_hotkey),
            audio_source,
            target_pid: json_num(txt, "target_pid").map(|v| v.max(0.0) as u32).unwrap_or(d.target_pid),
            cursor_mode,
            language: lang,
            model,
            use_gpu,
            auto_stop_silence_ms: json_num(txt, "auto_stop_silence_ms").map(|v| v.clamp(0.0, 10_000.0) as u32).unwrap_or(d.auto_stop_silence_ms),
            max_record_s: json_num(txt, "max_record_s").map(|v| v.clamp(3.0, 3600.0) as u32).unwrap_or(d.max_record_s),
            simplified_chinese: json_bool(txt, "simplified_chinese").unwrap_or(d.simplified_chinese),
            continuous_mode: json_bool(txt, "continuous_mode").unwrap_or(d.continuous_mode),
            smart_correction: json_bool(txt, "smart_correction").unwrap_or(d.smart_correction),
            replace_rules: json_str(txt, "replace_rules").unwrap_or(d.replace_rules),
            idle_unload_mins: json_num(txt, "idle_unload_mins").map(|v| v.clamp(0.0, 1440.0) as u32).unwrap_or(d.idle_unload_mins),
        }
    }

    pub fn to_json(&self) -> String {
        let mut esc_rules = String::with_capacity(self.replace_rules.len() + 8);
        for c in self.replace_rules.chars() {
            match c {
                '\\' => esc_rules.push_str("\\\\"),
                '"' => esc_rules.push_str("\\\""),
                '\n' => esc_rules.push_str("\\n"),
                '\r' => esc_rules.push_str("\\r"),
                '\t' => esc_rules.push_str("\\t"),
                _ => esc_rules.push(c),
            }
        }
        format!(
            "{{\n  \"enabled\": {},\n  \"hotkey\": \"{}\",\n  \"loopback_hotkey\": \"{}\",\n  \"audio_source\": \"{}\",\n  \"target_pid\": {},\n  \"cursor_mode\": \"{}\",\n  \"language\": \"{}\",\n  \"model\": \"{}\",\n  \"use_gpu\": \"{}\",\n  \"auto_stop_silence_ms\": {},\n  \"max_record_s\": {},\n  \"simplified_chinese\": {},\n  \"continuous_mode\": {},\n  \"smart_correction\": {},\n  \"replace_rules\": \"{}\",\n  \"idle_unload_mins\": {}\n}}\n",
            self.enabled,
            self.hotkey,
            self.loopback_hotkey,
            self.audio_source,
            self.target_pid,
            self.cursor_mode,
            self.language,
            self.model,
            self.use_gpu,
            self.auto_stop_silence_ms,
            self.max_record_s,
            self.simplified_chinese,
            self.continuous_mode,
            self.smart_correction,
            esc_rules,
            self.idle_unload_mins
        )
    }
}

/// Model names become file names: keep them to [A-Za-z0-9._-] so a config value can never escape the models dir.
pub fn is_safe_model_name(m: &str) -> bool {
    !m.is_empty() && m.len() < 64 && !m.contains("..") && m.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

pub fn known_model(name: &str) -> Option<(u64, &'static str)> {
    KNOWN_MODELS.iter().find(|(n, _, _)| *n == name).map(|(_, s, h)| (*s, *h))
}

// ---------------------------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------------------------

fn has_sys_dict(dir: &Path) -> bool {
    let d = dir.join("data");
    d.join("heyu_sys.tsv").exists()
        || d.join("flypy_sys.tsv").exists()
        || d.join("open").join("heyu_open_sys.tsv").exists()
}

/// Same rules as heyu-ime.exe: the exe dir, or the repo root when running from target/<profile>/.
pub fn find_base_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if has_sys_dict(dir) {
                return dir.to_path_buf();
            }
            if let Some(parent) = dir.parent().and_then(|p| p.parent()) {
                if has_sys_dict(parent) {
                    return parent.to_path_buf();
                }
            }
            return dir.to_path_buf();
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn appdata_heyu() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("HEYU"))
}

pub fn localappdata_heyu() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("HEYU"))
}

pub fn is_portable_mode() -> bool {
    find_base_dir().join("portable.flag").exists()
}

pub fn models_dir() -> PathBuf {
    let base = find_base_dir();
    let local_models = base.join("models");
    if base.join("portable.flag").exists() {
        return local_models;
    }
    if local_models.is_dir() {
        if fs::read_dir(&local_models)
            .ok()
            .and_then(|mut it| {
                it.find(|e| {
                    e.as_ref()
                        .ok()
                        .map(|x| x.path().extension().and_then(|s| s.to_str()) == Some("bin"))
                        .unwrap_or(false)
                })
            })
            .is_some()
        {
            return local_models;
        }
    }
    appdata_heyu().unwrap_or(base).join("models")
}

pub fn tmp_dir() -> PathBuf {
    if is_portable_mode() {
        return find_base_dir().join("data").join("tmp");
    }
    localappdata_heyu().unwrap_or_else(std::env::temp_dir).join("tmp")
}

#[derive(Clone, Debug)]
pub struct Dirs {
    pub base: PathBuf,
    /// Candidate data dirs, preferred first: %APPDATA%\HEYU\data (if it exists and not portable), then <base>\data.
    pub data: Vec<PathBuf>,
}

impl Dirs {
    pub fn resolve(override_dir: Option<PathBuf>) -> Self {
        let base = find_base_dir();
        let mut data = Vec::new();
        if let Some(d) = override_dir {
            data.push(d);
        } else if base.join("portable.flag").exists() {
            data.push(base.join("data"));
        } else {
            if let Some(a) = appdata_heyu().map(|p| p.join("data")) {
                if a.is_dir() {
                    data.push(a);
                }
            }
            let portable = base.join("data");
            if portable.is_dir() && !data.iter().any(|d| same_path(d, &portable)) {
                data.push(portable);
            }
            if data.is_empty() {
                data.push(base.join("data"));
            }
        }
        Self { base, data }
    }

    /// The most recently modified copy of `name` among the data dirs (the IME server, the studio and
    /// the installer may each write a different copy depending on portable vs. installed layout).
    pub fn newest(&self, name: &str) -> Option<PathBuf> {
        let mut best: Option<(SystemTime, PathBuf)> = None;
        for d in &self.data {
            let p = d.join(name);
            if let Some(t) = mtime(&p) {
                if best.as_ref().map(|(bt, _)| t > *bt).unwrap_or(true) {
                    best = Some((t, p));
                }
            }
        }
        best.map(|(_, p)| p)
    }

    /// Create voice_config.json with defaults in every data dir that lacks it.
    pub fn ensure_config(&self) {
        let def = VoiceConfig::default().to_json();
        for d in &self.data {
            let p = d.join(CONFIG_NAME);
            if d.is_dir() && !p.exists() {
                let _ = fs::write(&p, &def);
            }
        }
    }

    pub fn save_config(&self, cfg: &VoiceConfig) {
        let raw = cfg.to_json();
        for d in &self.data {
            if d.is_dir() {
                let _ = fs::write(d.join(CONFIG_NAME), &raw);
            }
        }
    }

    pub fn load_config(&self) -> (VoiceConfig, Option<SystemTime>) {
        match self.newest(CONFIG_NAME) {
            Some(p) => match fs::read_to_string(&p) {
                Ok(t) => (VoiceConfig::parse(t.trim_start_matches('\u{feff}')), mtime(&p)),
                Err(_) => (VoiceConfig::default(), None),
            },
            None => (VoiceConfig::default(), None),
        }
    }

    pub fn config_mtime(&self) -> Option<SystemTime> {
        self.newest(CONFIG_NAME).and_then(|p| mtime(&p))
    }

    pub fn runtime_dir(&self) -> Option<PathBuf> {
        let mut c = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(d) = exe.parent() {
                c.push(d.join("voice-runtime"));
            }
        }
        c.push(self.base.join("voice-runtime"));
        if let Some(l) = localappdata_heyu() {
            c.push(l.join("voice-runtime"));
        }
        c.into_iter().find(|d| d.join("whisper.dll").is_file() || d.join("whisper-cli.exe").is_file())
    }

    /// Current IME mode → whisper language code (`follow` setting).
    pub fn ime_language(&self) -> &'static str {
        if let Some(p) = self.newest("telemetry.json") {
            if let Ok(t) = fs::read_to_string(&p) {
                if let Some(m) = json_str(&t, "mode") {
                    if m.contains("中文") || m.contains('鹤') || m.to_ascii_uppercase().starts_with("ZH") {
                        return "zh";
                    }
                    let u = m.to_ascii_uppercase();
                    if u.starts_with("IT") {
                        return "it";
                    }
                    if u.starts_with("ES") {
                        return "es";
                    }
                    if u.starts_with("EN") {
                        return "en";
                    }
                    // If user temporarily toggled `Shift` to `A · 直通` on the keyboard, fall through
                    // to `config.json` `default_mode` so keyboard Shift never alters voice transcription!
                }
            }
        }
        if let Some(p) = self.newest("config.json") {
            if let Ok(t) = fs::read_to_string(&p) {
                return match json_str(&t, "default_mode").unwrap_or_default().to_ascii_uppercase().as_str() {
                    "ZH" => "zh",
                    "IT" => "it",
                    "ES" => "es",
                    "EN" => "en",
                    _ => "auto",
                };
            }
        }
        "auto"
    }

    pub fn theme_index(&self) -> usize {
        self.newest("config.json")
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|t| json_num(&t, "theme_index"))
            .map(|v| v.max(0.0) as usize)
            .unwrap_or(0)
    }

    /// Collect up to ~40 custom words/terms from `user_custom.tsv` (skipping long template sentences > 12 chars or containing `{`).
    pub fn load_custom_hotwords(&self) -> Vec<String> {
        let mut words = Vec::new();
        if let Some(p) = self.newest("user_custom.tsv") {
            if let Some(txt) = heyu_core::vault::read_user_lexicon_decrypted(&p) {
                for line in txt.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let mut parts = line.split('\t');
                    let _code = parts.next();
                    let Some(word) = parts.next() else { continue };
                    let w_raw = word.trim();
                    let w = if let Some((_, rhs)) = w_raw.split_once("=>").or_else(|| w_raw.split_once("->")) {
                        rhs.trim()
                    } else {
                        w_raw
                    };
                    if w.is_empty() || w.contains('{') || w.contains("\\n") || w.chars().count() > 12 {
                        continue;
                    }
                    if !words.iter().any(|existing| existing == w) {
                        words.push(w.to_string());
                        if words.len() >= 40 {
                            break;
                        }
                    }
                }
            }
        }
        let (cfg, _) = self.load_config();
        for (_, correct) in crate::corrector::parse_replace_rules(&cfg.replace_rules) {
            if !correct.is_empty()
                && correct.chars().count() <= 12
                && !words.iter().any(|existing| existing == &correct)
                && words.len() < 48
            {
                words.push(correct);
            }
        }
        words
    }

    /// Combine `replace_rules` from `voice_config.json` with any `wrong=>correct` entries stored in `user_custom.tsv` (`HEYUENC1`).
    pub fn load_custom_replace_rules(&self, cfg_rules: &str) -> String {
        let mut combined = cfg_rules.trim().to_string();
        if let Some(p) = self.newest("user_custom.tsv") {
            if let Some(txt) = heyu_core::vault::read_user_lexicon_decrypted(&p) {
                for line in txt.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let mut parts = line.split('\t');
                    let _code = parts.next();
                    let Some(word) = parts.next() else { continue };
                    let w = word.trim();
                    if w.contains("=>") || w.contains("->") {
                        if !combined.is_empty() && !combined.ends_with(';') {
                            combined.push(';');
                        }
                        combined.push_str(w);
                    }
                }
            }
        }
        combined
    }

    /// On startup or when retention is 0 (`用完即毁`), securely zero-wipe any leftover history/stream files.
    pub fn cleanup_zero_retention(&self) {
        let cfg_txt = self.newest("config.json").and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default();
        let retention_mins = json_num(&cfg_txt, "history_retention_mins").map(|v| v.max(0.0) as u64).unwrap_or(0);
        if retention_mins == 0 {
            for d in &self.data {
                heyu_core::vault::secure_wipe_file(&d.join("input_history.enc"));
                heyu_core::vault::secure_wipe_file(&d.join("voice_stream.jsonl"));
            }
        }
    }

    /// Record a committed voice segment to `<data_dir>/input_history.enc` (encrypted at rest) if `history_retention_mins > 0` in `config.json`.
    pub fn append_voice_history(&self, text: &str) {
        let clean = text.trim();
        if clean.is_empty() {
            return;
        }
        let cfg_txt = self.newest("config.json").and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default();
        let retention_mins = json_num(&cfg_txt, "history_retention_mins").map(|v| v.max(0.0) as u64).unwrap_or(0);
        let max_items = json_num(&cfg_txt, "history_max_items").map(|v| v.max(1.0) as usize).unwrap_or(200);
        let pin_salt = json_str(&cfg_txt, "history_pin_salt").unwrap_or_default();
        let wrapped_key = json_str(&cfg_txt, "history_wrapped_key").unwrap_or_default();

        if retention_mins == 0 {
            for d in &self.data {
                heyu_core::vault::secure_wipe_file(&d.join("input_history.enc"));
            }
            return;
        }
        let primary_dir = self.data.first().cloned().unwrap_or_else(|| self.base.join("data"));
        let key = heyu_core::vault::resolve_history_key(&primary_dir, &pin_salt, &wrapped_key);
        let now_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cutoff = now_secs.saturating_sub(retention_mins * 60);

        let mut items = if let Some(p) = self.newest("input_history.enc") {
            heyu_core::vault::read_history_items(p.parent().unwrap_or(&primary_dir), &key, cutoff)
        } else {
            Vec::new()
        };
        items.insert(
            0,
            heyu_core::vault::HistoryItem {
                timestamp: now_secs,
                source: "语音".to_string(),
                text: clean.to_string(),
            },
        );
        items.truncate(max_items.max(1));
        heyu_core::vault::write_history_items(&self.data, &key, &items);
    }

    /// Publish a live transcribed segment to `<data_dir>/voice_stream.jsonl` ONLY when the Dual-Cursor Co-Editing Workbench
    /// is actively open (`is_dual_cursor_sink_active()`) or the user explicitly enabled history retention (`history_retention_mins > 0`).
    /// Default is Burn-After-Use (`用完即毁`): zero voice transcript files are persisted on disk!
    pub fn append_voice_stream(&self, seq: u64, lang: &str, source: &str, text: &str) {
        use std::io::Write;
        let clean = text.trim();
        if clean.is_empty() {
            return;
        }
        let cfg_txt = self.newest("config.json").and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default();
        let retention_mins = json_num(&cfg_txt, "history_retention_mins").map(|v| v.max(0.0) as u64).unwrap_or(0);
        if retention_mins == 0 && !self.is_dual_cursor_sink_active() {
            for d in &self.data {
                heyu_core::vault::secure_wipe_file(&d.join("voice_stream.jsonl"));
            }
            return;
        }
        let ts_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let eff_seq = seq.max(ts_ms);
        let mut esc = String::with_capacity(clean.len() + 8);
        for c in clean.chars() {
            match c {
                '\\' => esc.push_str("\\\\"),
                '"' => esc.push_str("\\\""),
                '\n' => esc.push_str("\\n"),
                '\r' => esc.push_str("\\r"),
                '\t' => esc.push_str("\\t"),
                _ => esc.push(c),
            }
        }
        let line = format!(
            "{{\"ts\":{},\"seq\":{},\"lang\":\"{}\",\"source\":\"{}\",\"text\":\"{}\"}}\n",
            ts_ms, eff_seq, lang, source, esc
        );
        for d in &self.data {
            if !d.is_dir() {
                continue;
            }
            let p = d.join("voice_stream.jsonl");
            let too_large = fs::metadata(&p).map(|m| m.len() > 256 * 1024).unwrap_or(false);
            let mut opts = fs::OpenOptions::new();
            opts.create(true).write(true);
            if too_large {
                opts.truncate(true);
            } else {
                opts.append(true);
            }
            if let Ok(mut f) = opts.open(&p) {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }

    /// Check if the Dual-Cursor Co-Editing Workbench in `heyu_studio.py` is currently open and actively
    /// capturing the voice stream into its independent `┃🎙转录光标` (updated within the last 6 seconds).
    pub fn is_dual_cursor_sink_active(&self) -> bool {
        let Some(p) = self.newest("dual_cursor_sink.json") else {
            return false;
        };
        let Some(mt) = mtime(&p) else {
            return false;
        };
        let fresh = SystemTime::now()
            .duration_since(mt)
            .map(|d| d.as_secs() <= 6)
            .unwrap_or(false);
        if !fresh {
            return false;
        }
        fs::read_to_string(&p)
            .ok()
            .and_then(|t| json_bool(&t, "active"))
            .unwrap_or(false)
    }

    /// Poll `<data_dir>/voice_cmd.json` for control commands (`start_system`, `toggle_system`, `start_mic`, `toggle_mic`, `stop`).
    pub fn poll_voice_cmd(&self, last_seq: &mut u64) -> Option<String> {
        let p = self.newest("voice_cmd.json")?;
        let txt = fs::read_to_string(&p).ok()?;
        let seq = json_num(&txt, "seq").map(|v| v.max(0.0) as u64).unwrap_or(0);
        if seq == 0 || seq <= *last_seq {
            return None;
        }
        *last_seq = seq;
        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        if now_ms.saturating_sub(seq) > 8_000 {
            return None;
        }
        json_str(&txt, "cmd")
    }

    /// Write a lightweight live status heartbeat `voice_live_status.json` so standalone mini-windows
    /// (`--mini-voice`) can render real-time recording state, active source (`mic` vs `system`),
    /// audio level bar, segment count, latest recognized sentence, and 10-second target-window lock status.
    pub fn write_live_status(
        &self,
        phase: &str,
        source: &str,
        level: f32,
        chunks: u32,
        elapsed_s: f32,
        last_text: &str,
        target_title: &str,
        target_locked: bool,
        lock_rem_s: u32,
    ) {
        let ts_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let ts_sec = (ts_ms as f64) / 1000.0;
        let lvl = level.clamp(0.0, 1.0);
        let peak_db = if lvl <= 0.001 { -60.0 } else { -46.0 + lvl * 45.0 };
        let esc_str = |s: &str| -> String {
            let mut e = String::with_capacity(s.len() + 8);
            for c in s.chars() {
                match c {
                    '\\' => e.push_str("\\\\"),
                    '"' => e.push_str("\\\""),
                    '\n' | '\r' | '\t' => e.push(' '),
                    _ => e.push(c),
                }
            }
            e
        };
        let esc = esc_str(last_text);
        let esc_title = esc_str(target_title);
        let body = format!(
            "{{\"phase\":\"{}\",\"source\":\"{}\",\"level\":{:.3},\"peak_db\":{:.1},\"chunks\":{},\"committed_chunks\":{},\"elapsed_s\":{:.1},\"last_text\":\"{}\",\"target_title\":\"{}\",\"target_locked\":{},\"lock_rem_s\":{},\"ts\":{:.3},\"ts_ms\":{}}}\n",
            phase, source, lvl, peak_db, chunks, chunks, elapsed_s.max(0.0), esc, esc_title, target_locked, lock_rem_s, ts_sec, ts_ms
        );
        for d in &self.data {
            if d.is_dir() {
                let _ = fs::write(d.join("voice_live_status.json"), &body);
            }
        }
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

pub fn mtime(p: &Path) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

// ---------------------------------------------------------------------------------------------
// Flat JSON value extraction (top-level scalar keys only — all our config files are flat).
// ---------------------------------------------------------------------------------------------

fn value_start<'a>(txt: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{}\"", key);
    let mut from = 0;
    while let Some(i) = txt[from..].find(&pat) {
        let rest = &txt[from + i + pat.len()..];
        let rest_trim = rest.trim_start();
        if let Some(r) = rest_trim.strip_prefix(':') {
            return Some(r.trim_start());
        }
        from += i + pat.len();
    }
    None
}

pub fn json_str(txt: &str, key: &str) -> Option<String> {
    let v = value_start(txt, key)?;
    let mut chars = v.strip_prefix('"')?.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let cp = u32::from_str_radix(&hex, 16).ok()?;
                    if (0xD800..0xDC00).contains(&cp) {
                        // surrogate pair \uD83C\uDF99
                        let mut look = chars.clone();
                        if look.next() == Some('\\') && look.next() == Some('u') {
                            let hex2: String = look.by_ref().take(4).collect();
                            if let Ok(lo) = u32::from_str_radix(&hex2, 16) {
                                if let Some(ch) = char::from_u32(0x10000 + ((cp - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF)) {
                                    out.push(ch);
                                    chars = look;
                                    continue;
                                }
                            }
                        }
                    } else if let Some(ch) = char::from_u32(cp) {
                        out.push(ch);
                    }
                }
                other => out.push(other),
            },
            _ => out.push(c),
        }
    }
    None
}

pub fn json_raw(txt: &str, key: &str) -> Option<String> {
    let v = value_start(txt, key)?;
    let end = v.find(|c: char| c == ',' || c == '}' || c == '\n' || c == '\r').unwrap_or(v.len());
    Some(v[..end].trim().to_string())
}

pub fn json_bool(txt: &str, key: &str) -> Option<bool> {
    match json_raw(txt, key)?.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

pub fn json_num(txt: &str, key: &str) -> Option<f64> {
    json_raw(txt, key)?.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_schema_and_defaults() {
        let c = VoiceConfig::parse(r#"{"enabled": false, "hotkey": "Ctrl+Shift+Space", "language": "IT", "model": "small-q5_1", "use_gpu": false, "auto_stop_silence_ms": 900, "max_record_s": 30, "simplified_chinese": false}"#);
        assert!(!c.enabled);
        assert_eq!(c.hotkey, "Ctrl+Shift+Space");
        assert_eq!(c.language, "it");
        assert_eq!(c.model, "small-q5_1");
        assert_eq!(c.use_gpu, "false");
        assert_eq!(c.auto_stop_silence_ms, 900);
        assert_eq!(c.max_record_s, 30);
        assert!(!c.simplified_chinese);
        let d = VoiceConfig::parse("{}");
        assert_eq!(d, VoiceConfig::default());
        assert_eq!(VoiceConfig::parse(&d.to_json()), d);
        // path traversal in model name is rejected
        assert_eq!(VoiceConfig::parse(r#"{"model": "..\\..\\evil"}"#).model, "large-v3-q5_0");
    }

    #[test]
    fn json_unicode() {
        assert_eq!(json_str(r#"{"mode": "\u9e64 \u00b7 \u4e2d\u6587"}"#, "mode").unwrap(), "鹤 · 中文");
        assert_eq!(json_str(r#"{"mode": "鹤 · 中文"}"#, "mode").unwrap(), "鹤 · 中文");
    }
}
