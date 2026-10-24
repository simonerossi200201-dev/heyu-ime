//! HEYU (鹤羽输入法) - Clean-Room Core Input Method & Polyglot Engine
//! ====================================================================
//! 100% Pure Offline Rust Implementation:
//! - Xiaohe Yinxing (小鹤音形 v10.9z) 4-code Sound+Shape State Machine
//! - 4-Code Unique Auto-Commit (四码唯一自动上屏) & 5th-Key Push-Commit (第五码顶字上屏)
//! - 2-Letter Shuangpin Shape-Code Memory Recovery Hints (双拼鹤形记忆唤醒提示)
//! - 220+ CPM Turbo Super-Phrases & Contextual Ghost [Tab] Predictor (幽灵整句续写)
//! - 4-Language Seamless Polyglot Engine: Chinese (ZH), English (EN), Italiano (IT), Español (ES)
//! - Zero-Accent Blind Typing -> Automatic Native Accents (perche -> perché, espanol -> español)

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LangMode {
    /// 🇨🇳 中文 · 小鹤音形 (含四语混输 & 220+ 极速幽灵续写)
    ZhFlypy,
    /// 🇮🇹 Italiano · 意语智能重音与变位补全
    It,
    /// 🇪🇸 Español · 西语智能重音与变位补全
    Es,
    /// 🇬🇧 English · 英语极速补全
    En,
    /// ⌨️ 直通模式 (Shift 临时/持久英文直通)
    Direct,
}

impl LangMode {
    pub fn badge_text(&self) -> &'static str {
        match self {
            LangMode::ZhFlypy => "鹤 · 中文",
            LangMode::It => "IT · 意语",
            LangMode::Es => "ES · 西语",
            LangMode::En => "EN · 英语",
            LangMode::Direct => "A · 直通",
        }
    }

    pub fn short_tag(&self) -> &'static str {
        match self {
            LangMode::ZhFlypy => "鹤",
            LangMode::It => "IT",
            LangMode::Es => "ES",
            LangMode::En => "EN",
            LangMode::Direct => "英",
        }
    }

    pub fn next_cycle(&self) -> Self {
        match self {
            LangMode::ZhFlypy => LangMode::It,
            LangMode::It => LangMode::Es,
            LangMode::Es => LangMode::En,
            LangMode::En => LangMode::ZhFlypy,
            LangMode::Direct => LangMode::ZhFlypy,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub text: String,
    pub comment: String,
    pub layer: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeHint {
    pub ch: String,
    pub suffix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    /// Key was not consumed by IME; let OS pass it through to target window
    PassThrough,
    /// Key was consumed; HUD state updated (no text committed yet)
    Consumed,
    /// Key was consumed and `text` should be injected via SendInput(KEYEVENTF_UNICODE)
    Commit(String),
}

#[derive(Debug, Clone)]
pub struct PolyglotEntry {
    pub word: String,
    pub freq: u64,
    pub gloss: String,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub theme_index: usize,
    pub font_scale: f32,
    pub show_shape_hints: bool,
    pub auto_commit_4code: bool,
    pub auto_clear_empty_4code: bool,
    pub enable_ghost_tab: bool,
    pub page_size: usize,
    pub show_status_pill: bool,
    pub hide_candidate_hud: bool,
    pub immersion_mode: String,
    /// "yinxing" (鹤羽音形四码) | "shuangpin" (纯双拼免形码) | "quanpin" (全拼兼容)
    pub input_scheme: String,
    /// Output Traditional Chinese (繁体中文) when true.
    pub traditional_chinese: bool,
    /// Automatically output native Chinese fullwidth punctuation (`【】`, `（）`, `“”`, `；`, `￥`, etc.) in Chinese mode.
    pub chinese_punctuation: bool,
    pub pill_x: i32,
    pub pill_y: i32,
    pub hud_x: i32,
    pub hud_y: i32,
    /// Global hotkey for the quick-add word popup (e.g. "Ctrl+Alt+N"; empty = disabled).
    pub quick_add_hotkey: String,
    /// Local auto-expiring input history retention window in minutes (`0` = disabled, default `30`).
    pub history_retention_mins: u64,
    /// Maximum number of recent history items retained on disk (default `200`).
    pub history_max_items: usize,
    /// Optional PIN lock metadata (hex strings) for `input_history.enc`.
    pub history_pin_salt: String,
    pub history_pin_verifier: String,
    pub history_wrapped_key: String,
    /// Status Pill custom username (replaces default "鹤" when non-empty) and custom avatar settings.
    pub user_name: String,
    pub avatar_enabled: bool,
    pub avatar_size: u32,
    /// Overall floating status capsule scale & text size multiplier (`0.70..=1.65`, default `1.0`).
    pub pill_scale: f32,
    /// Comma-separated list of active capsule modules & action buttons.
    pub pill_items: String,
    /// Master Password & Device-Bound Vault metadata.
    pub master_pwd_salt: String,
    pub master_pwd_verifier: String,
    pub master_wrapped_vault_key: String,
    pub device_wrapped_vault_key: String,
    pub authorized_device_id: String,
    pub last_password_verified_ts: u64,
    pub last_lexicon_backup_ts: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            theme_index: 0,
            font_scale: 1.0,
            show_shape_hints: true,
            auto_commit_4code: true,
            auto_clear_empty_4code: true,
            enable_ghost_tab: true,
            page_size: 5,
            show_status_pill: true,
            hide_candidate_hud: false,
            immersion_mode: "ALL".to_string(),
            input_scheme: "yinxing".to_string(),
            traditional_chinese: false,
            chinese_punctuation: true,
            pill_x: -1,
            pill_y: -1,
            hud_x: -1,
            hud_y: -1,
            quick_add_hotkey: "Ctrl+Alt+N".to_string(),
            history_retention_mins: 0,
            history_max_items: 200,
            history_pin_salt: String::new(),
            history_pin_verifier: String::new(),
            history_wrapped_key: String::new(),
            user_name: String::new(),
            avatar_enabled: false,
            avatar_size: 26,
            pill_scale: 1.0,
            pill_items: "badge,cpm,sub,scheme,trad,voice,recorder,quick_add,settings".to_string(),
            master_pwd_salt: String::new(),
            master_pwd_verifier: String::new(),
            master_wrapped_vault_key: String::new(),
            device_wrapped_vault_key: String::new(),
            authorized_device_id: String::new(),
            last_password_verified_ts: 0,
            last_lexicon_backup_ts: 0,
        }
    }
}

pub struct HeyuEngine {
    pub data_dir: PathBuf,
    pub config: EngineConfig,
    pub mode: LangMode,
    pub mode_before_direct: LangMode,

    // Core Clean-Room Heyu Dictionaries
    pub flypy_map: HashMap<String, Vec<Candidate>>,
    pub user_map: HashMap<String, Vec<Candidate>>,
    pub reverse_map: HashMap<String, String>,
    pub sp2_hints: HashMap<String, Vec<ShapeHint>>,
    /// User-local private tables (`%APPDATA%\HEYU\data\private`), never distributed.
    pub private_dir: Option<PathBuf>,
    /// Which core table file was actually loaded (private / legacy data/ / open default).
    pub core_table_path: Option<PathBuf>,
    pub turbo_phrases: Vec<(String, String)>, // (code4, phrase)
    pub context_ngrams: HashMap<String, Vec<String>>,
    pub user_bigrams: HashMap<String, Vec<String>>,
    pub immersion_map: HashMap<String, (String, String, String)>, // zh -> (en, it, es)

    // Intelligent Quanpin & Shuangpin Lattice Index (120+ CPM Whole-Sentence Viterbi Decoder)
    pub sp_char_map: HashMap<String, Vec<(String, u32)>>,
    pub sp_word_map: HashMap<String, Vec<(String, u32)>>,
    pub init_word_map: HashMap<String, Vec<(String, u32)>>,
    pub word_init_hint: HashMap<String, String>,
    pub user_freq_boost: HashMap<String, u32>,
    pub candidate_consumed: HashMap<String, usize>,
    pub preedit_segmented: String,

    // Smart Stateful Punctuation Trackers
    pub quote_double_open: bool,
    pub quote_single_open: bool,
    pub es_question_open: bool,
    pub es_exclam_open: bool,

    // Polyglot Lexicons: ascii_key -> sorted entries
    pub poly_en: HashMap<String, Vec<PolyglotEntry>>,
    pub poly_it: HashMap<String, Vec<PolyglotEntry>>,
    pub poly_es: HashMap<String, Vec<PolyglotEntry>>,
    // Prefix index (2-4 char ascii prefix -> top words for fast completion)
    pub prefix_en: HashMap<String, Vec<PolyglotEntry>>,
    pub prefix_it: HashMap<String, Vec<PolyglotEntry>>,
    pub prefix_es: HashMap<String, Vec<PolyglotEntry>>,

    // Live Composition State
    pub input_buf: String,
    pub page: usize,
    pub candidates: Vec<Candidate>,
    pub active_shape_hints: Vec<ShapeHint>,
    pub ghost_tab_text: Option<(String, String)>, // (commit_text, display_label)
    pub post_commit_ghosts: Vec<String>,          // Available right after a commit
    pub last_committed: String,
    pub commit_stack: Vec<String>,
    pub pending_undo_backspaces: usize,
    pub pending_mic_toggle: bool,
    pub last_immersion_tip: String,

    // Speed & Telemetry Metrics
    pub keystroke_timestamps: Vec<Instant>,
    pub char_timestamps: Vec<(Instant, usize)>,
    pub total_chars_typed: u64,
    pub total_keystrokes: u64,
    pub saved_keystrokes: u64,
    pub peak_cpm: u32,
}

/// `%APPDATA%\HEYU\data\private` - where user-imported (non-redistributable) tables live.
pub fn default_private_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("HEYU").join("data").join("private"))
}

impl HeyuEngine {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self::with_private_dir(data_dir, default_private_dir())
    }

    /// Like [`HeyuEngine::new`] but with an explicit private-table directory (`None` = no
    /// private tables). Used by tests and portable setups.
    pub fn with_private_dir(data_dir: impl AsRef<Path>, private_dir: Option<PathBuf>) -> Self {
        let data_dir = data_dir.as_ref().to_path_buf();
        let mut engine = Self {
            data_dir,
            private_dir,
            core_table_path: None,
            config: EngineConfig::default(),
            mode: LangMode::ZhFlypy,
            mode_before_direct: LangMode::ZhFlypy,
            flypy_map: HashMap::with_capacity(72000),
            user_map: HashMap::new(),
            reverse_map: HashMap::with_capacity(10000),
            sp2_hints: HashMap::with_capacity(450),
            turbo_phrases: Vec::new(),
            context_ngrams: HashMap::new(),
            user_bigrams: HashMap::new(),
            immersion_map: HashMap::with_capacity(1024),
            sp_char_map: HashMap::with_capacity(450),
            sp_word_map: HashMap::with_capacity(64000),
            init_word_map: HashMap::with_capacity(32000),
            word_init_hint: HashMap::with_capacity(2048),
            user_freq_boost: HashMap::new(),
            candidate_consumed: HashMap::new(),
            preedit_segmented: String::new(),
            quote_double_open: false,
            quote_single_open: false,
            es_question_open: false,
            es_exclam_open: false,
            poly_en: HashMap::with_capacity(22000),
            poly_it: HashMap::with_capacity(22000),
            poly_es: HashMap::with_capacity(22000),
            prefix_en: HashMap::with_capacity(15000),
            prefix_it: HashMap::with_capacity(15000),
            prefix_es: HashMap::with_capacity(15000),
            input_buf: String::new(),
            page: 0,
            candidates: Vec::new(),
            active_shape_hints: Vec::new(),
            ghost_tab_text: None,
            post_commit_ghosts: Vec::new(),
            last_committed: String::new(),
            commit_stack: Vec::new(),
            pending_undo_backspaces: 0,
            pending_mic_toggle: false,
            last_immersion_tip: String::new(),
            keystroke_timestamps: Vec::new(),
            char_timestamps: Vec::new(),
            total_chars_typed: 0,
            total_keystrokes: 0,
            saved_keystrokes: 0,
            peak_cpm: 0,
        };
        engine.load_all_data();
        engine.load_config();
        engine.load_telemetry();
        engine
    }

    pub fn load_all_data(&mut self) {
        self.load_flypy_sys();
        self.load_user_custom();
        self.load_reverse();
        self.load_sp2_hints();
        self.load_turbo_and_ngrams();
        self.load_immersion_gloss();
        self.build_pinyin_lattice_index();
        let (en_exact, en_pfx) = Self::load_polyglot_file(&self.data_dir.join("polyglot_en.tsv"));
        self.poly_en = en_exact;
        self.prefix_en = en_pfx;
        let (it_exact, it_pfx) = Self::load_polyglot_file(&self.data_dir.join("polyglot_it.tsv"));
        self.poly_it = it_exact;
        self.prefix_it = it_pfx;
        let (es_exact, es_pfx) = Self::load_polyglot_file(&self.data_dir.join("polyglot_es.tsv"));
        self.poly_es = es_exact;
        self.prefix_es = es_pfx;
    }

    fn load_flypy_sys(&mut self) {
        self.flypy_map.clear();
        let path = self.resolve_table(&["heyu_sys.tsv", "flypy_sys.tsv"], "heyu_open_sys.tsv");
        self.core_table_path = path.clone();
        if let Some(content) = path.and_then(|p| fs::read_to_string(p).ok()) {
            for line in content.lines() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split('\t');
                if let (Some(code), Some(word)) = (parts.next(), parts.next()) {
                    let _rank = parts.next();
                    let layer = parts.next().unwrap_or("primary");
                    self.flypy_map
                        .entry(code.to_string())
                        .or_default()
                        .push(Candidate {
                            text: word.to_string(),
                            comment: String::new(),
                            layer: layer.to_string(),
                        });
                }
            }
        }
    }

    pub fn load_user_custom(&mut self) {
        self.user_map.clear();
        let path = self.data_dir.join("user_custom.tsv");
        if let Some(content) = crate::vault::read_user_lexicon_decrypted(&path) {
            for line in content.lines() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split('\t');
                if let (Some(code), Some(word)) = (parts.next(), parts.next()) {
                    let comment = parts.next().unwrap_or("个人").to_string();
                    self.user_map
                        .entry(code.trim().to_string())
                        .or_default()
                        .push(Candidate {
                            text: word.trim().replace("\\n", "\n"),
                            comment,
                            layer: "user".to_string(),
                        });
                }
            }
        }
        if !self.sp_char_map.is_empty() {
            self.index_user_custom_into_lattice();
        }
    }

    /// Add or pin a phrase `(code, text, comment)` at the top of `data/user_custom.tsv` and reload.
    pub fn add_or_pin_user_phrase(&mut self, code: &str, text: &str, comment: &str) -> bool {
        let path = self.data_dir.join("user_custom.tsv");
        if write_user_phrase_file(&path, code, text, comment) {
            if let Some(app_root) = crate::paths::appdata_root() {
                let app_data = app_root.join("data");
                if app_data.is_dir() && app_data != self.data_dir {
                    let _ = write_user_phrase_file(&app_data.join("user_custom.tsv"), code, text, comment);
                }
            }
            self.load_user_custom();
            return true;
        }
        false
    }

    /// Commits candidate at `offset` (0-based on current page) AND pins it to #1 in `user_custom.tsv`.
    pub fn pin_candidate_at_page_offset(&mut self, offset: usize) -> KeyAction {
        let ps = self.config.page_size.max(1);
        let idx = self.page * ps + offset;
        let code = self.input_buf.clone();
        if !code.is_empty() && !code.starts_with('`') && !code.starts_with(';') {
            if let Some(cand) = self.candidates.get(idx).cloned() {
                let _ = self.add_or_pin_user_phrase(&code, &cand.text, "置顶首选");
                let keys_used = code.len() + 1;
                self.clear_composition();
                self.record_commit(&cand.text, keys_used);
                return KeyAction::Commit(cand.text);
            }
        }
        KeyAction::PassThrough
    }

    fn load_reverse(&mut self) {
        self.reverse_map.clear();
        let path = self.resolve_table(&["heyu_reverse.tsv", "flypy_reverse.tsv"], "heyu_open_reverse.tsv");
        if let Some(content) = path.and_then(|p| fs::read_to_string(p).ok()) {
            for line in content.lines() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split('\t');
                if let (Some(ch), Some(codes)) = (parts.next(), parts.next()) {
                    self.reverse_map.insert(ch.to_string(), codes.to_string());
                }
            }
        }
    }

    /// Table lookup order (first existing file wins):
    /// 1. `<private_dir>/<name>`  - user-imported private tables, never distributed
    /// 2. `<data_dir>/<name>`     - legacy location (keeps existing local setups working)
    /// 3. `<data_dir>/open/<open_name>` - distributable open default table
    /// 4. `<exe_dir>/data/open/<open_name>` - open table next to an installed binary
    pub fn table_candidates(&self, names: &[&str], open_name: &str) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(p) = &self.private_dir {
            out.extend(names.iter().map(|n| p.join(n)));
        }
        out.extend(names.iter().map(|n| self.data_dir.join(n)));
        out.push(self.data_dir.join("open").join(open_name));
        if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf())) {
            out.push(exe_dir.join("data").join("open").join(open_name));
        }
        out
    }

    fn resolve_table(&self, names: &[&str], open_name: &str) -> Option<PathBuf> {
        self.table_candidates(names, open_name).into_iter().find(|p| p.is_file())
    }

    /// True when the loaded core table is the distributable open default table.
    pub fn uses_open_table(&self) -> bool {
        self.core_table_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().starts_with("heyu_open_"))
            .unwrap_or(false)
    }

    fn load_sp2_hints(&mut self) {
        self.sp2_hints.clear();
        let path = self.resolve_table(&["shuangpin_shape_hints.tsv"], "heyu_open_shape_hints.tsv");
        if let Some(content) = path.and_then(|p| fs::read_to_string(p).ok()) {
            for line in content.lines() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split('\t');
                if let (Some(sp2), Some(items_str)) = (parts.next(), parts.next()) {
                    let mut list = Vec::new();
                    for item in items_str.split('|') {
                        if let Some((ch, sfx)) = item.split_once(':') {
                            list.push(ShapeHint {
                                ch: ch.to_string(),
                                suffix: sfx.to_string(),
                            });
                        }
                    }
                    self.sp2_hints.insert(sp2.to_string(), list);
                }
            }
        }
    }

    fn load_turbo_and_ngrams(&mut self) {
        self.turbo_phrases.clear();
        let turbo_path = self.data_dir.join("zh_turbo_phrases.tsv");
        if let Ok(content) = fs::read_to_string(&turbo_path) {
            for line in content.lines() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split('\t');
                if let (Some(code4), Some(phrase)) = (parts.next(), parts.next()) {
                    self.turbo_phrases
                        .push((code4.to_string(), phrase.to_string()));
                }
            }
        }

        self.context_ngrams.clear();
        let ngram_path = self.data_dir.join("zh_context_ngrams.tsv");
        if let Ok(content) = fs::read_to_string(&ngram_path) {
            for line in content.lines() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split('\t');
                if let (Some(trig), Some(cands)) = (parts.next(), parts.next()) {
                    let list = cands.split('|').map(|s| s.to_string()).collect();
                    self.context_ngrams.insert(trig.to_string(), list);
                }
            }
        }
    }

    fn load_polyglot_file(
        path: &Path,
    ) -> (
        HashMap<String, Vec<PolyglotEntry>>,
        HashMap<String, Vec<PolyglotEntry>>,
    ) {
        let mut exact_map: HashMap<String, Vec<PolyglotEntry>> = HashMap::with_capacity(20000);
        let mut prefix_map: HashMap<String, Vec<PolyglotEntry>> = HashMap::with_capacity(15000);
        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split('\t');
                if let (Some(ascii_key), Some(word), Some(freq_str)) =
                    (parts.next(), parts.next(), parts.next())
                {
                    let freq: u64 = freq_str.parse().unwrap_or(1);
                    let gloss = parts.next().unwrap_or("").to_string();
                    let entry = PolyglotEntry {
                        word: word.to_string(),
                        freq,
                        gloss,
                    };
                    if !ascii_key.is_ascii() {
                        continue;
                    }
                    exact_map
                        .entry(ascii_key.to_string())
                        .or_default()
                        .push(entry.clone());

                    let char_len = ascii_key.len();
                    if char_len >= 4 {
                        for plen in 2..=usize::min(6, char_len - 1) {
                            let pfx = &ascii_key[..plen];
                            let list = prefix_map.entry(pfx.to_string()).or_default();
                            if list.len() < 6 && !list.iter().any(|e| e.word == entry.word) {
                                list.push(entry.clone());
                            }
                        }
                    }
                }
            }
        }
        (exact_map, prefix_map)
    }

    pub fn load_immersion_gloss(&mut self) {
        self.immersion_map.clear();
        let path = self.data_dir.join("zh_immersion_gloss.tsv");
        if let Ok(content) = fs::read_to_string(&path) {
            for line in content.lines() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split('\t');
                if let Some(zh) = parts.next() {
                    let en = parts.next().unwrap_or("").trim().to_string();
                    let it = parts.next().unwrap_or("").trim().to_string();
                    let es = parts.next().unwrap_or("").trim().to_string();
                    if !zh.is_empty() && (!en.is_empty() || !it.is_empty() || !es.is_empty()) {
                        self.immersion_map.insert(zh.to_string(), (en, it, es));
                    }
                }
            }
        }
    }

    fn first_gloss_token(s: &str) -> &str {
        s.split('/').next().unwrap_or(s).trim()
    }

    pub fn format_immersion_for_candidate(&self, zh_word: &str) -> String {
        let mode = self.config.immersion_mode.as_str();
        if mode.is_empty() || mode.eq_ignore_ascii_case("OFF") {
            return String::new();
        }
        let Some((en, it, es)) = self.immersion_map.get(zh_word) else {
            return String::new();
        };
        let en1 = Self::first_gloss_token(en);
        let it1 = Self::first_gloss_token(it);
        let es1 = Self::first_gloss_token(es);
        match mode {
            "EN" => {
                if !en1.is_empty() {
                    format!("\nEN {}", en1)
                } else {
                    String::new()
                }
            }
            "IT" => {
                if !it1.is_empty() {
                    format!("\nIT {}", it1)
                } else {
                    String::new()
                }
            }
            "ES" => {
                if !es1.is_empty() {
                    format!("\nES {}", es1)
                } else {
                    String::new()
                }
            }
            _ => {
                let mut parts = Vec::with_capacity(3);
                if !it1.is_empty() {
                    parts.push(format!("IT {}", it1));
                }
                if !es1.is_empty() {
                    parts.push(format!("ES {}", es1));
                }
                if !en1.is_empty() {
                    parts.push(format!("EN {}", en1));
                }
                if parts.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", parts.join("\n"))
                }
            }
        }
    }

    pub fn update_last_immersion_tip(&mut self, committed_text: &str) {
        let mode = self.config.immersion_mode.as_str();
        if mode.is_empty() || mode.eq_ignore_ascii_case("OFF") {
            self.last_immersion_tip.clear();
            return;
        }
        let trimmed = committed_text.trim();
        if trimmed.is_empty() {
            return;
        }
        // Try exact word first, then longest 2..4 char subword inside committed phrase
        let mut matched: Option<(&str, &(String, String, String))> = None;
        if let Some(entry) = self.immersion_map.get(trimmed) {
            matched = Some((trimmed, entry));
        } else {
            let chars: Vec<(usize, char)> = trimmed.char_indices().collect();
            'outer: for win_len in (1..=usize::min(4, chars.len())).rev() {
                for start in 0..=(chars.len() - win_len) {
                    let byte_start = chars[start].0;
                    let byte_end = if start + win_len < chars.len() {
                        chars[start + win_len].0
                    } else {
                        trimmed.len()
                    };
                    let sub = &trimmed[byte_start..byte_end];
                    if let Some(entry) = self.immersion_map.get(sub) {
                        matched = Some((sub, entry));
                        break 'outer;
                    }
                }
            }
        }
        if let Some((zh, (en, it, es))) = matched {
            let en1 = Self::first_gloss_token(en);
            let it1 = Self::first_gloss_token(it);
            let es1 = Self::first_gloss_token(es);
            let rhs = match mode {
                "EN" => en1.to_string(),
                "IT" => it1.to_string(),
                "ES" => es1.to_string(),
                _ => {
                    let mut v = Vec::new();
                    if !it1.is_empty() {
                        v.push(it1);
                    }
                    if !es1.is_empty() && es1 != it1 {
                        v.push(es1);
                    }
                    if !en1.is_empty() {
                        v.push(en1);
                    }
                    v.join("/")
                }
            };
            if !rhs.is_empty() {
                self.last_immersion_tip = format!("{}={}", zh, rhs);
            }
        }
    }

    pub fn load_config(&mut self) {
        let cfg_path = self.data_dir.join("config.json");
        if let Ok(txt) = fs::read_to_string(&cfg_path) {
            if let Some(v) = Self::extract_json_u64(&txt, "theme_index") {
                self.config.theme_index = (v as usize) % crate::hud::THEMES.len();
            }
            if let Some(v) = Self::extract_json_f32(&txt, "font_scale") {
                self.config.font_scale = v.clamp(0.75, 2.0);
            }
            if let Some(v) = Self::extract_json_bool(&txt, "show_shape_hints") {
                self.config.show_shape_hints = v;
            }
            if let Some(v) = Self::extract_json_bool(&txt, "auto_commit_4code") {
                self.config.auto_commit_4code = v;
            }
            if let Some(v) = Self::extract_json_bool(&txt, "auto_clear_empty_4code") {
                self.config.auto_clear_empty_4code = v;
            }
            if let Some(v) = Self::extract_json_bool(&txt, "enable_ghost_tab") {
                self.config.enable_ghost_tab = v;
            }
            if let Some(v) = Self::extract_json_u64(&txt, "page_size") {
                self.config.page_size = (v as usize).clamp(3, 10);
            }
            if let Some(v) = Self::extract_json_bool(&txt, "show_status_pill") {
                self.config.show_status_pill = v;
            }
            if let Some(v) = Self::extract_json_bool(&txt, "hide_candidate_hud") {
                self.config.hide_candidate_hud = v;
            }
            if let Some(imm) = Self::extract_json_str(&txt, "immersion_mode") {
                let u = imm.to_uppercase();
                if matches!(u.as_str(), "OFF" | "EN" | "IT" | "ES" | "ALL") {
                    self.config.immersion_mode = u;
                }
            }
            if let Some(sc) = Self::extract_json_str(&txt, "input_scheme") {
                let sl = sc.to_ascii_lowercase();
                if matches!(sl.as_str(), "yinxing" | "shuangpin" | "quanpin") {
                    self.config.input_scheme = sl;
                }
            }
            if let Some(v) = Self::extract_json_bool(&txt, "traditional_chinese") {
                self.config.traditional_chinese = v;
            }
            if let Some(v) = Self::extract_json_bool(&txt, "chinese_punctuation") {
                self.config.chinese_punctuation = v;
            }
            if let Some(v) = Self::extract_json_u64(&txt, "history_retention_mins") {
                self.config.history_retention_mins = v.min(1440);
            }
            if let Some(v) = Self::extract_json_u64(&txt, "history_max_items") {
                self.config.history_max_items = (v as usize).clamp(10, 5000);
            }
            if let Some(s) = Self::extract_json_str(&txt, "history_pin_salt") {
                self.config.history_pin_salt = s;
            }
            if let Some(s) = Self::extract_json_str(&txt, "history_pin_verifier") {
                self.config.history_pin_verifier = s;
            }
            if let Some(s) = Self::extract_json_str(&txt, "history_wrapped_key") {
                self.config.history_wrapped_key = s;
            }
            if let Some(s) = Self::extract_json_str(&txt, "user_name") {
                self.config.user_name = s.trim().to_string();
            }
            if let Some(v) = Self::extract_json_bool(&txt, "avatar_enabled") {
                self.config.avatar_enabled = v;
            }
            if let Some(v) = Self::extract_json_u64(&txt, "avatar_size") {
                self.config.avatar_size = (v as u32).clamp(16, 40);
            }
            if let Some(v) = Self::extract_json_f32(&txt, "pill_scale") {
                self.config.pill_scale = v.clamp(0.70, 1.65);
            }
            if let Some(s) = Self::extract_json_str(&txt, "pill_items") {
                let cleaned = s.trim().to_string();
                if !cleaned.is_empty() {
                    self.config.pill_items = cleaned;
                }
            }
            if let Some(s) = Self::extract_json_str(&txt, "master_pwd_salt") {
                self.config.master_pwd_salt = s;
            }
            if let Some(s) = Self::extract_json_str(&txt, "master_pwd_verifier") {
                self.config.master_pwd_verifier = s;
            }
            if let Some(s) = Self::extract_json_str(&txt, "master_wrapped_vault_key") {
                self.config.master_wrapped_vault_key = s;
            }
            if let Some(s) = Self::extract_json_str(&txt, "device_wrapped_vault_key") {
                self.config.device_wrapped_vault_key = s;
            }
            if let Some(s) = Self::extract_json_str(&txt, "authorized_device_id") {
                self.config.authorized_device_id = s;
            }
            if let Some(v) = Self::extract_json_u64(&txt, "last_password_verified_ts") {
                self.config.last_password_verified_ts = v;
            }
            if let Some(v) = Self::extract_json_u64(&txt, "last_lexicon_backup_ts") {
                self.config.last_lexicon_backup_ts = v;
            }
            if let Some(v) = Self::extract_json_i64(&txt, "pill_x") {
                self.config.pill_x = v as i32;
            }
            if let Some(v) = Self::extract_json_i64(&txt, "pill_y") {
                self.config.pill_y = v as i32;
            }
            if let Some(v) = Self::extract_json_i64(&txt, "hud_x") {
                self.config.hud_x = v as i32;
            }
            if let Some(v) = Self::extract_json_i64(&txt, "hud_y") {
                self.config.hud_y = v as i32;
            }
            if let Some(hk) = Self::extract_json_str(&txt, "quick_add_hotkey") {
                self.config.quick_add_hotkey = hk.trim().to_string();
            }
            if let Some(mode_str) = Self::extract_json_str(&txt, "default_mode") {
                match mode_str.as_str() {
                    "IT" => self.mode = LangMode::It,
                    "ES" => self.mode = LangMode::Es,
                    "EN" => self.mode = LangMode::En,
                    "DIRECT" => self.mode = LangMode::Direct,
                    _ => self.mode = LangMode::ZhFlypy,
                }
            }
        }
    }

    pub fn save_config(&self) {
        let mode_str = match self.mode {
            LangMode::ZhFlypy => "ZH",
            LangMode::It => "IT",
            LangMode::Es => "ES",
            LangMode::En => "EN",
            LangMode::Direct => "DIRECT",
        };
        let json = format!(
            "{{\n  \"theme_index\": {},\n  \"font_scale\": {:.2},\n  \"show_shape_hints\": {},\n  \"auto_commit_4code\": {},\n  \"auto_clear_empty_4code\": {},\n  \"enable_ghost_tab\": {},\n  \"page_size\": {},\n  \"show_status_pill\": {},\n  \"hide_candidate_hud\": {},\n  \"immersion_mode\": \"{}\",\n  \"input_scheme\": \"{}\",\n  \"traditional_chinese\": {},\n  \"chinese_punctuation\": {},\n  \"pill_x\": {},\n  \"pill_y\": {},\n  \"hud_x\": {},\n  \"hud_y\": {},\n  \"default_mode\": \"{}\",\n  \"quick_add_hotkey\": \"{}\",\n  \"history_retention_mins\": {},\n  \"history_max_items\": {},\n  \"history_pin_salt\": \"{}\",\n  \"history_pin_verifier\": \"{}\",\n  \"history_wrapped_key\": \"{}\",\n  \"user_name\": \"{}\",\n  \"avatar_enabled\": {},\n  \"avatar_size\": {},\n  \"pill_scale\": {:.2},\n  \"pill_items\": \"{}\",\n  \"master_pwd_salt\": \"{}\",\n  \"master_pwd_verifier\": \"{}\",\n  \"master_wrapped_vault_key\": \"{}\",\n  \"device_wrapped_vault_key\": \"{}\",\n  \"authorized_device_id\": \"{}\",\n  \"last_password_verified_ts\": {},\n  \"last_lexicon_backup_ts\": {}\n}}\n",
            self.config.theme_index,
            self.config.font_scale,
            self.config.show_shape_hints,
            self.config.auto_commit_4code,
            self.config.auto_clear_empty_4code,
            self.config.enable_ghost_tab,
            self.config.page_size,
            self.config.show_status_pill,
            self.config.hide_candidate_hud,
            self.config.immersion_mode,
            self.config.input_scheme,
            self.config.traditional_chinese,
            self.config.chinese_punctuation,
            self.config.pill_x,
            self.config.pill_y,
            self.config.hud_x,
            self.config.hud_y,
            mode_str,
            self.config.quick_add_hotkey.replace(['"', '\\'], ""),
            self.config.history_retention_mins,
            self.config.history_max_items,
            self.config.history_pin_salt.replace(['"', '\\'], ""),
            self.config.history_pin_verifier.replace(['"', '\\'], ""),
            self.config.history_wrapped_key.replace(['"', '\\'], ""),
            self.config.user_name.replace(['"', '\\'], ""),
            self.config.avatar_enabled,
            self.config.avatar_size,
            self.config.pill_scale,
            self.config.pill_items.replace(['"', '\\'], ""),
            self.config.master_pwd_salt.replace(['"', '\\'], ""),
            self.config.master_pwd_verifier.replace(['"', '\\'], ""),
            self.config.master_wrapped_vault_key.replace(['"', '\\'], ""),
            self.config.device_wrapped_vault_key.replace(['"', '\\'], ""),
            self.config.authorized_device_id.replace(['"', '\\'], ""),
            self.config.last_password_verified_ts,
            self.config.last_lexicon_backup_ts
        );
        let _ = fs::write(self.data_dir.join("config.json"), json);
    }

    pub fn load_telemetry(&mut self) {
        let mut path = self.data_dir.join("telemetry.json");
        let is_portable = self.data_dir.parent().map(|b| b.join("portable.flag").exists()).unwrap_or(false);
        if !is_portable {
            if let Some(app_root) = std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("HEYU").join("data").join("telemetry.json")) {
                let use_app = match (fs::metadata(&path), fs::metadata(&app_root)) {
                    (Ok(m1), Ok(m2)) => m2.modified().ok() > m1.modified().ok(),
                    (Err(_), Ok(_)) => true,
                    _ => false,
                };
                if use_app {
                    path = app_root;
                }
            }
        }
        if let Ok(txt) = fs::read_to_string(&path) {
            if let Some(v) = Self::extract_json_u64(&txt, "peak_cpm") {
                self.peak_cpm = v as u32;
            }
            if let Some(v) = Self::extract_json_u64(&txt, "total_chars") {
                self.total_chars_typed = v;
            }
            if let Some(v) = Self::extract_json_u64(&txt, "total_keystrokes") {
                self.total_keystrokes = v;
            }
            if let Some(v) = Self::extract_json_u64(&txt, "saved_keystrokes") {
                self.saved_keystrokes = v;
            }
        }
    }

    pub fn save_telemetry(&self) {
        let cpm = self.current_cpm();
        let avg_code = if self.total_chars_typed > 0 {
            (self.total_keystrokes as f64) / (self.total_chars_typed as f64)
        } else {
            0.0
        };
        let json = format!(
            "{{\n  \"current_cpm\": {},\n  \"peak_cpm\": {},\n  \"total_chars\": {},\n  \"total_keystrokes\": {},\n  \"saved_keystrokes\": {},\n  \"avg_code_length\": {:.2},\n  \"mode\": \"{}\"\n}}\n",
            cpm,
            self.peak_cpm,
            self.total_chars_typed,
            self.total_keystrokes,
            self.saved_keystrokes,
            avg_code,
            self.mode.badge_text()
        );
        let primary = self.data_dir.join("telemetry.json");
        let _ = fs::write(&primary, &json);
        let is_portable = self.data_dir.parent().map(|b| b.join("portable.flag").exists()).unwrap_or(false);
        if !is_portable {
            if let Some(app_data) = std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("HEYU").join("data")) {
                if app_data.exists() && app_data != self.data_dir {
                    let _ = fs::write(app_data.join("telemetry.json"), &json);
                }
            }
        }
    }

    fn extract_json_u64(txt: &str, key: &str) -> Option<u64> {
        let pat = format!("\"{}\"", key);
        let idx = txt.find(&pat)?;
        let rest = &txt[idx + pat.len()..];
        let colon = rest.find(':')?;
        let val_part = rest[colon + 1..].trim_start();
        let num_str: String = val_part
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        num_str.parse().ok()
    }

    fn extract_json_f32(txt: &str, key: &str) -> Option<f32> {
        let pat = format!("\"{}\"", key);
        let idx = txt.find(&pat)?;
        let rest = &txt[idx + pat.len()..];
        let colon = rest.find(':')?;
        let val_part = rest[colon + 1..].trim_start();
        let num_str: String = val_part
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
            .collect();
        num_str.parse().ok()
    }

    fn extract_json_i64(txt: &str, key: &str) -> Option<i64> {
        let pat = format!("\"{}\"", key);
        let idx = txt.find(&pat)?;
        let rest = &txt[idx + pat.len()..];
        let colon = rest.find(':')?;
        let val_part = rest[colon + 1..].trim_start();
        let num_str: String = val_part
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        num_str.parse().ok()
    }

    fn extract_json_bool(txt: &str, key: &str) -> Option<bool> {
        let pat = format!("\"{}\"", key);
        let idx = txt.find(&pat)?;
        let rest = &txt[idx + pat.len()..];
        let colon = rest.find(':')?;
        let val_part = rest[colon + 1..].trim_start();
        if val_part.starts_with("true") {
            Some(true)
        } else if val_part.starts_with("false") {
            Some(false)
        } else {
            None
        }
    }

    fn extract_json_str(txt: &str, key: &str) -> Option<String> {
        let pat = format!("\"{}\"", key);
        let idx = txt.find(&pat)?;
        let rest = &txt[idx + pat.len()..];
        let colon = rest.find(':')?;
        let after_colon = rest[colon + 1..].trim_start();
        if !after_colon.starts_with('"') {
            return None;
        }
        let inner = &after_colon[1..];
        let end_q = inner.find('"')?;
        Some(inner[..end_q].to_string())
    }

    // ========================================================================
    // TELEMETRY & CPM CALCULATION
    // ========================================================================
    pub fn record_keystroke(&mut self) {
        let now = Instant::now();
        self.total_keystrokes += 1;
        self.keystroke_timestamps.push(now);
        self.prune_telemetry(now);
    }

    pub fn record_commit(&mut self, text: &str, keys_used: usize) {
        let char_count = text.chars().count();
        if char_count == 0 {
            return;
        }
        let now = Instant::now();
        self.total_chars_typed += char_count as u64;
        // Standard pinyin / typing baseline is ~3.2 keys per char; track saved keys
        let baseline_keys = char_count * 3;
        if baseline_keys > keys_used {
            self.saved_keystrokes += (baseline_keys - keys_used) as u64;
        }
        self.char_timestamps.push((now, char_count));
        self.prune_telemetry(now);
        let cpm = self.current_cpm();
        if cpm > self.peak_cpm {
            self.peak_cpm = cpm;
        }

        // Update contextual ghost predictions, dynamic word frequency boost & learn user bigrams
        if self.mode == LangMode::ZhFlypy {
            let clean = text.trim().to_string();
            if !clean.is_empty() {
                let entry = self.user_freq_boost.entry(clean.clone()).or_insert(0);
                *entry = (*entry + 15_000).min(120_000);
                if !self.last_committed.is_empty() {
                    let prev = self.last_committed.clone();
                    let list = self.user_bigrams.entry(prev).or_default();
                    if !list.contains(&clean) {
                        list.insert(0, clean);
                        if list.len() > 3 {
                            list.pop();
                        }
                    }
                }
            }
        }
        self.last_committed = text.to_string();
        self.commit_stack.push(text.to_string());
        if self.commit_stack.len() > 30 {
            self.commit_stack.remove(0);
        }
        if self.mode == LangMode::ZhFlypy {
            self.update_last_immersion_tip(text);
        }
        self.update_post_commit_ghosts(text);
        self.record_history_entry(text, "键盘");
        self.save_telemetry();
    }

    pub fn push_external_commit(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.last_committed = text.to_string();
        self.commit_stack.push(text.to_string());
        if self.commit_stack.len() > 30 {
            self.commit_stack.remove(0);
        }
    }

    pub fn record_history_entry(&self, text: &str, mode_label: &str) {
        #[cfg(test)]
        if self.data_dir == std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data") {
            return;
        }
        let clean = text.trim();
        if clean.is_empty() {
            return;
        }
        // Default privacy policy: `history_retention_mins == 0` (Burn-After-Use / 用完即毁).
        // Never write any typing history to disk unless the user explicitly enabled retention!
        if self.config.history_retention_mins == 0 {
            let enc = self.data_dir.join("input_history.enc");
            if enc.exists() {
                crate::vault::secure_wipe_file(&enc);
            }
            return;
        }
        let key = crate::vault::resolve_history_key(
            &self.data_dir,
            &self.config.history_pin_salt,
            &self.config.history_wrapped_key,
        );
        let dirs = vec![self.data_dir.clone()];
        let now_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cutoff = now_s.saturating_sub(self.config.history_retention_mins * 60);
        let mut items = crate::vault::read_history_items(&self.data_dir, &key, cutoff);
        if items.first().map(|it| it.text.as_str()) == Some(clean) {
            items[0].timestamp = now_s;
        } else {
            items.insert(
                0,
                crate::vault::HistoryItem {
                    timestamp: now_s,
                    source: mode_label.to_string(),
                    text: clean.to_string(),
                },
            );
        }
        items.truncate(self.config.history_max_items.max(1));
        crate::vault::write_history_items(&dirs, &key, &items);
    }

    pub fn prune_and_save_history(&self) {
        let key = crate::vault::resolve_history_key(
            &self.data_dir,
            &self.config.history_pin_salt,
            &self.config.history_wrapped_key,
        );
        let dirs = vec![self.data_dir.clone()];
        if self.config.history_retention_mins == 0 {
            crate::vault::write_history_items(&dirs, &key, &[]);
            return;
        }
        let now_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cutoff = now_s.saturating_sub(self.config.history_retention_mins * 60);
        let mut items = crate::vault::read_history_items(&self.data_dir, &key, cutoff);
        items.truncate(self.config.history_max_items.max(1));
        crate::vault::write_history_items(&dirs, &key, &items);
    }

    fn prune_telemetry(&mut self, now: Instant) {
        let window = std::time::Duration::from_secs(15);
        self.keystroke_timestamps
            .retain(|t| now.duration_since(*t) <= window);
        self.char_timestamps
            .retain(|(t, _)| now.duration_since(*t) <= window);
    }

    pub fn current_cpm(&self) -> u32 {
        if self.char_timestamps.is_empty() {
            return 0;
        }
        let now = Instant::now();
        let window = std::time::Duration::from_secs(12);
        let mut total_chars = 0usize;
        let mut earliest = now;
        for (t, cnt) in &self.char_timestamps {
            if now.duration_since(*t) <= window {
                total_chars += *cnt;
                if *t < earliest {
                    earliest = *t;
                }
            }
        }
        if total_chars == 0 {
            return 0;
        }
        let elapsed = now.duration_since(earliest).as_secs_f64().max(1.8);
        ((total_chars as f64 / elapsed) * 60.0).round() as u32
    }

    fn update_post_commit_ghosts(&mut self, committed: &str) {
        self.post_commit_ghosts.clear();
        if !self.config.enable_ghost_tab || self.mode != LangMode::ZhFlypy {
            return;
        }
        let trimmed = committed.trim();
        if trimmed.is_empty()
            || trimmed.ends_with(|c: char| {
                matches!(
                    c,
                    '，' | '。' | '？' | '！' | '：' | '；' | '、' | ',' | '.' | '?' | '!' | ':' | ';'
                )
            })
        {
            return;
        }
        // 1. Check user's personal learned bigrams first
        if let Some(u_list) = self.user_bigrams.get(trimmed) {
            for item in u_list {
                if !self.post_commit_ghosts.contains(item) {
                    self.post_commit_ghosts.push(item.clone());
                }
            }
        }
        // 2. Check exact match in built-in contextual N-grams
        if let Some(c_list) = self.context_ngrams.get(trimmed) {
            for item in c_list {
                if !self.post_commit_ghosts.contains(item) {
                    self.post_commit_ghosts.push(item.clone());
                }
            }
        }
        // 3. Check last 2 characters of committed phrase
        let chars: Vec<char> = trimmed.chars().collect();
        if chars.len() >= 2 {
            let tail2: String = chars[chars.len() - 2..].iter().collect();
            if let Some(c_list) = self.context_ngrams.get(&tail2) {
                for item in c_list {
                    if !self.post_commit_ghosts.contains(item) {
                        self.post_commit_ghosts.push(item.clone());
                    }
                }
            }
        }
        self.post_commit_ghosts.truncate(3);
        if self.config.traditional_chinese {
            for g in &mut self.post_commit_ghosts {
                *g = Self::to_traditional_chinese(g);
            }
        }
    }

    // ========================================================================
    // STATE RESET & MODE SWITCHING
    // ========================================================================
    pub fn clear_composition(&mut self) {
        self.input_buf.clear();
        self.page = 0;
        self.candidates.clear();
        self.candidate_consumed.clear();
        self.preedit_segmented.clear();
        self.active_shape_hints.clear();
        self.ghost_tab_text = None;
    }

    /// Segmented preedit string for HUD / TSF display (e.g. `wo'men'jin'tian` in Quanpin mode).
    pub fn preedit_display(&self) -> String {
        if !self.preedit_segmented.is_empty() {
            self.preedit_segmented.clone()
        } else {
            self.input_buf.clone()
        }
    }

    /// Active Chinese scheme badge `(short_chip, long_name)` for HUD & Status Pill.
    pub fn chinese_scheme_badge(&self) -> (&'static str, &'static str) {
        match self.config.input_scheme.as_str() {
            "quanpin" => ("全拼", "智能全拼"),
            "shuangpin" => ("双拼", "鹤羽双拼"),
            _ => ("鹤", "鹤羽音形"),
        }
    }

    pub fn toggle_direct_mode(&mut self) -> Option<String> {
        let pending_raw = if self.mode != LangMode::Direct && !self.input_buf.is_empty() {
            let raw = self.input_buf.trim_start_matches('`').to_string();
            if !raw.is_empty() {
                let keys_used = self.input_buf.len();
                self.record_commit(&raw, keys_used);
                Some(raw)
            } else {
                None
            }
        } else {
            None
        };
        self.clear_composition();
        self.post_commit_ghosts.clear();
        if self.mode == LangMode::Direct {
            self.mode = self.mode_before_direct;
        } else {
            self.mode_before_direct = self.mode;
            self.mode = LangMode::Direct;
        }
        self.save_config();
        pending_raw
    }

    pub fn cycle_language_mode(&mut self) {
        self.clear_composition();
        self.post_commit_ghosts.clear();
        self.mode = self.mode.next_cycle();
        self.mode_before_direct = self.mode;
        self.save_config();
    }

    pub fn is_hud_visible(&self) -> bool {
        !self.input_buf.is_empty()
    }

    pub fn current_page_candidates(&self) -> &[Candidate] {
        let ps = self.config.page_size.max(1);
        let start = self.page * ps;
        if start >= self.candidates.len() {
            &[]
        } else {
            let end = usize::min(start + ps, self.candidates.len());
            &self.candidates[start..end]
        }
    }

    pub fn total_pages(&self) -> usize {
        let ps = self.config.page_size.max(1);
        if self.candidates.is_empty() {
            0
        } else {
            (self.candidates.len() + ps - 1) / ps
        }
    }

    // ========================================================================
    // CORE KEY PROCESSING ENTRYPOINT
    // ========================================================================
    pub fn process_key(&mut self, ch: char, shift_held: bool) -> KeyAction {
        if self.mode == LangMode::Direct {
            return KeyAction::PassThrough;
        }

        match self.mode {
            LangMode::ZhFlypy => self.process_zh_flypy_key(ch, shift_held),
            LangMode::It => self.process_polyglot_key(ch, LangMode::It),
            LangMode::Es => self.process_polyglot_key(ch, LangMode::Es),
            LangMode::En => self.process_polyglot_key(ch, LangMode::En),
            LangMode::Direct => KeyAction::PassThrough,
        }
    }

    /// Delete the last committed word/phrase (`ocx` macro) in a single action without pressing Backspace repeatedly!
    pub fn undo_last_commit(&mut self) -> KeyAction {
        let prev = self
            .commit_stack
            .pop()
            .unwrap_or_else(|| std::mem::take(&mut self.last_committed));
        self.last_committed = self.commit_stack.last().cloned().unwrap_or_default();
        self.clear_composition();
        self.post_commit_ghosts.clear();
        let n = prev.chars().count();
        if n == 0 {
            KeyAction::Consumed
        } else {
            self.pending_undo_backspaces += n;
            KeyAction::Commit("\u{8}".repeat(n))
        }
    }

    /// Directly toggle microphone voice input ON/OFF (`oiu` macro in Chinese mode, or `Ctrl+Shift+V` / `Ctrl+Alt+V` in any mode)!
    pub fn trigger_mic_voice_toggle(&mut self) -> KeyAction {
        self.clear_composition();
        self.post_commit_ghosts.clear();
        self.pending_mic_toggle = true;
        KeyAction::Consumed
    }

    /// Execute the formal combination hotkey equivalent of the `o`-prefixed macros (`orq`, `ouj`, `oxq`, `ors`)
    /// in any language mode (including Italian, Spanish, English, Direct, or Chinese) without typing collisions!
    pub fn execute_formal_macro_hotkey(&mut self, kind: &str) -> KeyAction {
        self.clear_composition();
        self.post_commit_ghosts.clear();
        let text = match kind {
            "date" => {
                let cands = Self::current_date_candidates();
                match self.mode {
                    LangMode::It => cands.get(7).or_else(|| cands.get(1)).cloned().unwrap_or_default(),
                    LangMode::Es => cands.get(8).or_else(|| cands.get(1)).cloned().unwrap_or_default(),
                    LangMode::En | LangMode::Direct => cands.get(1).cloned().unwrap_or_default(),
                    LangMode::ZhFlypy => cands.first().cloned().unwrap_or_default(),
                }
            }
            "time" => Self::current_time_candidates().into_iter().next().unwrap_or_default(),
            "weekday" => {
                let cands = Self::current_weekday_candidates();
                match self.mode {
                    LangMode::It => cands.get(2).cloned().unwrap_or_default(),
                    LangMode::Es => cands.get(3).cloned().unwrap_or_default(),
                    LangMode::En | LangMode::Direct => cands.get(4).cloned().unwrap_or_default(),
                    LangMode::ZhFlypy => cands.first().cloned().unwrap_or_default(),
                }
            }
            "datetime" => Self::current_datetime_candidates().into_iter().next().unwrap_or_default(),
            _ => String::new(),
        };
        if text.is_empty() {
            KeyAction::Consumed
        } else {
            self.record_commit(&text, 2);
            KeyAction::Commit(text)
        }
    }

    pub fn process_special_key(&mut self, vk: u16, alt_held: bool) -> KeyAction {
        const VK_BACK: u16 = 0x08;
        const VK_TAB: u16 = 0x09;
        const VK_RETURN: u16 = 0x0D;
        const VK_ESCAPE: u16 = 0x1B;
        const VK_SPACE: u16 = 0x20;
        const VK_PRIOR: u16 = 0x21; // PageUp
        const VK_NEXT: u16 = 0x22; // PageDown

        if self.mode == LangMode::Direct {
            return KeyAction::PassThrough;
        }

        // Alt + 1..6 selects directly from the Shuangpin Shape Memory Recovery bar!
        if alt_held && !self.active_shape_hints.is_empty() && (0x31..=0x36).contains(&vk) {
            let idx = (vk - 0x31) as usize;
            if let Some(hint) = self.active_shape_hints.get(idx).cloned() {
                let keys_used = self.input_buf.len() + 1;
                self.clear_composition();
                self.record_commit(&hint.ch, keys_used);
                return KeyAction::Commit(hint.ch);
            }
        }

        match vk {
            VK_ESCAPE => {
                if !self.input_buf.is_empty() || !self.post_commit_ghosts.is_empty() {
                    self.clear_composition();
                    self.post_commit_ghosts.clear();
                    return KeyAction::Consumed;
                }
                KeyAction::PassThrough
            }
            VK_BACK => {
                if !self.input_buf.is_empty() {
                    self.input_buf.pop();
                    self.page = 0;
                    if self.input_buf.is_empty() {
                        self.clear_composition();
                    } else {
                        self.refresh_candidates();
                    }
                    return KeyAction::Consumed;
                }
                if !self.post_commit_ghosts.is_empty() {
                    self.post_commit_ghosts.clear();
                }
                KeyAction::PassThrough
            }
            VK_SPACE => {
                if !self.input_buf.is_empty() {
                    return self.commit_candidate_at_page_offset(0, true);
                }
                if !self.post_commit_ghosts.is_empty() {
                    self.post_commit_ghosts.clear();
                }
                KeyAction::PassThrough
            }
            VK_RETURN => {
                if !self.input_buf.is_empty() {
                    // Enter commits the raw ASCII letters in the buffer!
                    let raw = self.input_buf.trim_start_matches('`').to_string();
                    let keys_used = self.input_buf.len();
                    self.clear_composition();
                    self.post_commit_ghosts.clear();
                    self.record_commit(&raw, keys_used);
                    return KeyAction::Commit(raw);
                }
                if !self.post_commit_ghosts.is_empty() {
                    self.post_commit_ghosts.clear();
                }
                KeyAction::PassThrough
            }
            VK_TAB => {
                self.record_keystroke();
                // 1. If we are currently composing and have a Ghost [Tab] completion:
                if !self.input_buf.is_empty() {
                    if let Some((ghost_word, _)) = self.ghost_tab_text.clone() {
                        let keys_used = self.input_buf.len() + 1;
                        let out = if matches!(self.mode, LangMode::En | LangMode::It | LangMode::Es)
                        {
                            format!("{} ", ghost_word)
                        } else {
                            ghost_word
                        };
                        self.clear_composition();
                        self.record_commit(&out, keys_used);
                        return KeyAction::Commit(out);
                    }
                    // Otherwise Tab cycles pages if multiple pages exist
                    if self.total_pages() > 1 {
                        self.page = (self.page + 1) % self.total_pages();
                        return KeyAction::Consumed;
                    }
                    return KeyAction::Consumed;
                }
                // 2. If buffer is empty, check Post-Commit Contextual Ghost Continuation!
                if !self.post_commit_ghosts.is_empty() {
                    let ghost = self.post_commit_ghosts[0].clone();
                    self.post_commit_ghosts.clear();
                    self.record_commit(&ghost, 1);
                    return KeyAction::Commit(ghost);
                }
                KeyAction::PassThrough
            }
            VK_PRIOR => {
                if !self.input_buf.is_empty() && self.page > 0 {
                    self.page -= 1;
                    return KeyAction::Consumed;
                }
                KeyAction::PassThrough
            }
            VK_NEXT => {
                if !self.input_buf.is_empty() && self.page + 1 < self.total_pages() {
                    self.page += 1;
                    return KeyAction::Consumed;
                }
                KeyAction::PassThrough
            }
            _ => KeyAction::PassThrough,
        }
    }

    fn commit_candidate_at_page_offset(&mut self, offset: usize, from_space: bool) -> KeyAction {
        let ps = self.config.page_size.max(1);
        let idx = self.page * ps + offset;
        let is_poly_mode = matches!(self.mode, LangMode::En | LangMode::It | LangMode::Es)
            || self
                .input_buf
                .chars()
                .next()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false);

        if let Some(cand) = self.candidates.get(idx).cloned() {
            let consumed = self
                .candidate_consumed
                .get(&cand.text)
                .copied()
                .unwrap_or(self.input_buf.len());
            if !is_poly_mode && consumed > 0 && consumed < self.input_buf.len() {
                // Progressive Prefix Selection (渐进分词选词): commit only the selected prefix word/char
                // and retain the remaining unconsumed syllables in `self.input_buf`!
                let remain = self.input_buf[consumed..].trim_start_matches('\'').to_string();
                let out = cand.text;
                self.record_commit(&out, consumed);
                self.input_buf = remain;
                self.page = 0;
                if self.input_buf.is_empty() {
                    self.clear_composition();
                } else {
                    self.refresh_candidates();
                }
                return KeyAction::Commit(out);
            }
            let keys_used = self.input_buf.len() + 1;
            self.clear_composition();
            let out = if is_poly_mode && from_space {
                format!("{} ", cand.text)
            } else {
                cand.text
            };
            self.record_commit(&out, keys_used);
            return KeyAction::Commit(out);
        } else if !self.input_buf.is_empty() {
            if is_poly_mode {
                let raw = if from_space {
                    format!("{} ", self.input_buf)
                } else {
                    self.input_buf.clone()
                };
                let keys_used = self.input_buf.len() + 1;
                self.clear_composition();
                self.record_commit(&raw, keys_used);
                return KeyAction::Commit(raw);
            }
            // Empty code in Chinese mode -> clear
            self.clear_composition();
            return KeyAction::Consumed;
        }
        KeyAction::PassThrough
    }

    // ========================================================================
    // CHINESE XIAOHE YINXING / SHUANGPIN / QUANPIN + MIXED POLYGLOT STATE MACHINE
    // ========================================================================
    fn process_zh_flypy_key(&mut self, ch: char, _shift_held: bool) -> KeyAction {
        self.record_keystroke();

        // Inline Expression Calculator mode (`=` prefix, e.g. `=125*38`, `=(100+250)/7`, `=2^10`)
        if self.input_buf.is_empty() && ch == '=' {
            self.post_commit_ghosts.clear();
            self.input_buf.push('=');
            self.page = 0;
            self.refresh_candidates();
            return KeyAction::Consumed;
        }
        if self.input_buf.starts_with('=') {
            if ch.is_ascii_digit() || matches!(ch, '+' | '-' | '*' | '/' | '%' | '^' | '(' | ')' | '.') {
                self.input_buf.push(ch);
                self.page = 0;
                self.refresh_candidates();
                return KeyAction::Consumed;
            }
            if ch == '=' {
                if self.input_buf == "=" {
                    self.clear_composition();
                    self.record_commit("=", 2);
                    return KeyAction::Commit("=".to_string());
                }
                return self.commit_candidate_at_page_offset(0, false);
            }
            if ch == ';' {
                let target_offset = if self.current_page_candidates().len() >= 2 { 1 } else { 0 };
                return self.commit_candidate_at_page_offset(target_offset, false);
            }
            if ch == '\'' {
                let target_offset = if self.current_page_candidates().len() >= 3 {
                    2
                } else if self.current_page_candidates().len() >= 2 {
                    1
                } else {
                    0
                };
                return self.commit_candidate_at_page_offset(target_offset, false);
            }
        }

        // Pagination keys `-` / `=` (and `[` / `]` when multiple pages exist) when composing
        if !self.input_buf.is_empty() {
            if ch == '-' || (ch == '[' && self.total_pages() > 1) {
                if self.page > 0 {
                    self.page -= 1;
                }
                return KeyAction::Consumed;
            }
            if ch == '=' || (ch == ']' && self.total_pages() > 1) {
                if self.page + 1 < self.total_pages() {
                    self.page += 1;
                }
                return KeyAction::Consumed;
            }
            // Number selection 1..9 and 0 (for 10th candidate) when composing
            if ('0'..='9').contains(&ch) {
                let offset = if ch == '0' { 9 } else { (ch as usize) - ('1' as usize) };
                if offset < self.current_page_candidates().len() {
                    return self.commit_candidate_at_page_offset(offset, false);
                }
                return KeyAction::Consumed;
            }
        } else if !self.post_commit_ghosts.is_empty() {
            // Number selection 1..9 and 0 in Post-Commit Ghost Continuation ("续写 · 数字选择")
            if ('0'..='9').contains(&ch) {
                let idx = if ch == '0' { 9 } else { (ch as usize) - ('1' as usize) };
                if let Some(ghost) = self.post_commit_ghosts.get(idx).cloned() {
                    self.post_commit_ghosts.clear();
                    self.record_commit(&ghost, 1);
                    return KeyAction::Commit(ghost);
                }
            }
            // Any other normal key clears post-commit ghost and starts the new input
            self.post_commit_ghosts.clear();
        }

        // Semicolon `;` handling:
        // - If buffer has candidates: `;` commits Candidate #2 (分号次选上屏)!
        // - If buffer empty in `quanpin` / `shuangpin`: directly commits Chinese semicolon `；`!
        // - If buffer empty in `yinxing`: starts `;` Fast Symbol / History / Clipboard mode (`;;` or Space commits `；`)
        if ch == ';' {
            if self.input_buf.is_empty() {
                if self.config.input_scheme == "yinxing" {
                    self.input_buf.push(';');
                    self.refresh_candidates();
                    return KeyAction::Consumed;
                } else {
                    self.record_commit("；", 1);
                    return KeyAction::Commit("；".to_string());
                }
            } else if self.input_buf == ";" {
                self.clear_composition();
                self.record_commit("；", 2);
                return KeyAction::Commit("；".to_string());
            } else {
                // Commit 2nd candidate on current page (or 1st if only 1 exists)
                let target_offset = if self.current_page_candidates().len() >= 2 {
                    1
                } else {
                    0
                };
                return self.commit_candidate_at_page_offset(target_offset, false);
            }
        }

        // Apostrophe `'` handling:
        // - When buffer has candidates in ZhFlypy mode: commits Candidate #3 (单引号三选上屏)!
        // - When buffer is empty: falls through to `map_chinese_punctuation` to output alternating `‘` / `’`!
        if ch == '\'' && !self.input_buf.is_empty() && !self.input_buf.starts_with(';') {
            let is_capital_poly = self
                .input_buf
                .chars()
                .next()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false);
            if !is_capital_poly {
                let target_offset = if self.current_page_candidates().len() >= 3 {
                    2
                } else if self.current_page_candidates().len() >= 2 {
                    1
                } else {
                    0
                };
                return self.commit_candidate_at_page_offset(target_offset, false);
            }
        }

        // Backtick `` ` `` Reverse Lookup starter
        if ch == '`' && self.input_buf.is_empty() {
            self.input_buf.push('`');
            self.refresh_candidates();
            return KeyAction::Consumed;
        }

        // If currently in `;` Fast Symbol mode (`;a` .. `;z`):
        if self.input_buf == ";" && ch.is_ascii_lowercase() {
            if ch == 'f' {
                // `;f` repeats last committed text!
                let rep = self.last_committed.clone();
                self.clear_composition();
                if !rep.is_empty() {
                    self.record_commit(&rep, 2);
                    return KeyAction::Commit(rep);
                }
                return KeyAction::Consumed;
            }
            if ch == 'h' || ch == 'v' {
                // `;h` (Recent Encrypted History) & `;v` (Clipboard Quick Paste) stay open in HUD for 1..9 / Space selection!
                self.input_buf.push(ch);
                self.page = 0;
                self.refresh_candidates();
                return KeyAction::Consumed;
            }
            self.input_buf.push(ch);
            self.refresh_candidates();
            if let Some(first) = self.candidates.first().cloned() {
                self.clear_composition();
                self.record_commit(&first.text, 2);
                return KeyAction::Commit(first.text);
            }
            self.clear_composition();
            return KeyAction::Consumed;
        }

        // If starting with Uppercase A-Z (or already in Uppercase Polyglot word):
        // Note: `o`-prefixed macros (`oiu`, `ocx`, `orq`, `ouj`, `oxq`, `ors`) are NEVER active in
        // Uppercase Polyglot or non-Chinese modes so they never collide with foreign words!
        let in_upper_poly = self
            .input_buf
            .chars()
            .next()
            .map(|c| c.is_ascii_uppercase())
            .unwrap_or(false);
        if (self.input_buf.is_empty() && ch.is_ascii_uppercase())
            || (in_upper_poly && (ch.is_ascii_alphabetic() || ch == '\'' || ch == '`'))
        {
            self.input_buf.push(ch);
            self.page = 0;
            self.refresh_candidates();
            return KeyAction::Consumed;
        }

        // If in Reverse Lookup mode (starts with `` ` ``)
        if self.input_buf.starts_with('`') && ch.is_ascii_lowercase() {
            self.input_buf.push(ch);
            self.page = 0;
            self.refresh_candidates();
            return KeyAction::Consumed;
        }

        // Standard Flypy / Shuangpin / Quanpin lowercase a-z input (Chinese mode only)!
        if ch.is_ascii_lowercase() {
            let mut prefix_committed = String::new();
            let is_quanpin = self.config.input_scheme == "quanpin";
            let is_shuangpin = self.config.input_scheme == "shuangpin";
            let is_yinxing = !is_quanpin && !is_shuangpin;
            let max_comp_len = if is_quanpin {
                40
            } else if is_shuangpin {
                24
            } else {
                4
            };

            // Rule: 5th-Key Push-Commit (第五码顶字上屏 in YinXing, or buffer-full push in Quanpin/Shuangpin)!
            if (self.input_buf.len() >= max_comp_len && !self.input_buf.starts_with('`') && !self.input_buf.starts_with('='))
                || (self.input_buf.starts_with(';') && self.input_buf.len() >= 2)
            {
                if let Some(first) = self.candidates.first().cloned() {
                    prefix_committed.push_str(&first.text);
                    self.record_commit(&first.text, self.input_buf.len());
                }
                self.clear_composition();
            }

            self.input_buf.push(ch);
            self.page = 0;

            // Chinese-only `o`-prefix instant action macros (`ocx` undo, `oiu` microphone toggle):
            if self.input_buf == "ocx" {
                return self.undo_last_commit();
            }
            if self.input_buf == "oiu" {
                return self.trigger_mic_voice_toggle();
            }

            self.refresh_candidates();

            // Check User Custom `直出` Auto-Commit & 4-Code Unique Auto-Commit (四码唯一自动上屏)!
            let has_longer_user_prefix = self
                .user_map
                .keys()
                .any(|k| k.len() > self.input_buf.len() && k.starts_with(&self.input_buf));
            let user_force_auto = !has_longer_user_prefix
                && self
                    .candidates
                    .first()
                    .map(|c| {
                        c.layer == "user"
                            && (c.comment.contains("直出")
                                || c.comment.contains("独占")
                                || c.comment.eq_ignore_ascii_case("auto"))
                    })
                    .unwrap_or(false);

            if user_force_auto && self.input_buf.len() >= 2 {
                let word = self.candidates[0].text.clone();
                let klen = self.input_buf.len();
                self.clear_composition();
                self.record_commit(&word, klen);
                prefix_committed.push_str(&word);
                return KeyAction::Commit(prefix_committed);
            }

            // 4-Code Unique Auto-Commit applies ONLY in 4-code YinXing (`"yinxing"`) mode!
            // In `"quanpin"` and `"shuangpin"` modes, users can type multi-word sentences of up to 24-40 keys.
            if is_yinxing && self.input_buf.len() == 4 && self.input_buf.chars().all(|c| c.is_ascii_lowercase()) {
                let allow_4code_auto = self.candidates.len() == 1 && self.config.auto_commit_4code;
                if allow_4code_auto {
                    let word = self.candidates[0].text.clone();
                    self.clear_composition();
                    self.record_commit(&word, 4);
                    prefix_committed.push_str(&word);
                    return KeyAction::Commit(prefix_committed);
                } else if self.candidates.is_empty() && self.config.auto_clear_empty_4code {
                    self.clear_composition();
                    if !prefix_committed.is_empty() {
                        return KeyAction::Commit(prefix_committed);
                    }
                    return KeyAction::Consumed;
                }
            }

            if !prefix_committed.is_empty() {
                return KeyAction::Commit(prefix_committed);
            }
            return KeyAction::Consumed;
        }

        // Chinese Punctuation Mapping & Push-Commit (标点顶字上屏: Fullwidth Chinese Punctuation)
        if let Some(zh_punct) = self.map_chinese_punctuation(ch) {
            let mut out = String::new();
            if !self.input_buf.is_empty() {
                if let Some(first) = self.candidates.first().cloned() {
                    out.push_str(&first.text);
                }
                let keys_used = self.input_buf.len() + 1;
                self.clear_composition();
                out.push_str(zh_punct);
                self.record_commit(&out, keys_used);
                return KeyAction::Commit(out);
            } else {
                self.record_commit(zh_punct, 1);
                return KeyAction::Commit(zh_punct.to_string());
            }
        }

        KeyAction::PassThrough
    }

    fn map_chinese_punctuation(&mut self, ch: char) -> Option<&'static str> {
        if !self.config.chinese_punctuation {
            return None;
        }
        match ch {
            ',' => Some("，"),
            '.' => Some("。"),
            '?' => Some("？"),
            '!' => Some("！"),
            ':' => Some("："),
            ';' => Some("；"),
            '(' => Some("（"),
            ')' => Some("）"),
            '[' => Some("【"),
            ']' => Some("】"),
            '{' => Some("｛"),
            '}' => Some("｝"),
            '<' => Some("《"),
            '>' => Some("》"),
            '\\' => Some("、"),
            '^' => Some("……"),
            '_' => Some("——"),
            '~' => Some("～"),
            '$' => Some("￥"),
            '"' => {
                let s = if self.quote_double_open { "”" } else { "“" };
                self.quote_double_open = !self.quote_double_open;
                Some(s)
            }
            '\'' => {
                let s = if self.quote_single_open { "’" } else { "‘" };
                self.quote_single_open = !self.quote_single_open;
                Some(s)
            }
            _ => None,
        }
    }

    // ========================================================================
    // POLYGLOT (ITALIANO / ESPAÑOL / ENGLISH) SMART ACCENT & COMPLETION ENGINE
    // ========================================================================
    fn process_polyglot_key(&mut self, ch: char, lang: LangMode) -> KeyAction {
        self.record_keystroke();

        // Number selection 1..9 and 0 when composing in Polyglot mode
        if !self.input_buf.is_empty() && ('0'..='9').contains(&ch) {
            let offset = if ch == '0' { 9 } else { (ch as usize) - ('1' as usize) };
            if offset < self.current_page_candidates().len() {
                return self.commit_candidate_at_page_offset(offset, true);
            }
        }

        // Semicolon `;` in Polyglot mode selects Candidate #2 (or #1 accented form)
        if !self.input_buf.is_empty() && ch == ';' {
            let offset = if self.current_page_candidates().len() >= 2 {
                1
            } else {
                0
            };
            return self.commit_candidate_at_page_offset(offset, true);
        }

        // Spanish double-tap inverted punctuation shortcut (`??` -> `¿`, `!!` -> `¡`)
        if lang == LangMode::Es && self.input_buf.is_empty() {
            if ch == '?' && self.last_committed == "?" {
                self.last_committed = "¿".to_string();
                self.es_question_open = true;
                return KeyAction::Commit("\x08¿".to_string());
            }
            if ch == '!' && self.last_committed == "!" {
                self.last_committed = "¡".to_string();
                self.es_exclam_open = true;
                return KeyAction::Commit("\x08¡".to_string());
            }
        }

        // Explicit dead-key accent modifiers while typing in IT / ES mode:
        // `'` -> acute accent on last vowel (`a'` -> `á`, `e'` -> `é`, `n'` -> `ñ`),
        //        or Italian elision apostrophe `’` after consonant (`l'`, `un'`, `d'`, `c'`, `quest'`)!
        // `` ` `` -> grave accent on last vowel (`` a` `` -> `à`, `` e` `` -> `è`)
        // `~` -> tilde on `n` (`n~` -> `ñ`)
        if !self.input_buf.is_empty() && (ch == '\'' || ch == '`' || ch == '~') {
            if let Some(last_ch) = self.input_buf.pop() {
                if let Some(accented) = Self::apply_explicit_accent(last_ch, ch, lang) {
                    self.input_buf.push(accented);
                    self.refresh_candidates();
                    return KeyAction::Consumed;
                } else {
                    self.input_buf.push(last_ch);
                    if lang == LangMode::It && ch == '\'' {
                        let word = self
                            .candidates
                            .first()
                            .map(|c| c.text.clone())
                            .unwrap_or_else(|| self.input_buf.clone());
                        let out = format!("{}’", word);
                        let keys_used = self.input_buf.len() + 1;
                        self.clear_composition();
                        self.record_commit(&out, keys_used);
                        return KeyAction::Commit(out);
                    }
                }
            }
        }

        if ch.is_ascii_alphabetic() {
            self.input_buf.push(ch);
            self.page = 0;
            self.refresh_candidates();
            return KeyAction::Consumed;
        }

        // Native language-aware punctuation in IT / ES / EN:
        // Commits active word (with automatic accents!) + native punctuation, or commits standalone native punctuation.
        let has_preedit = !self.input_buf.is_empty();
        if let Some((punct_str, intercept_when_empty)) = self.map_polyglot_punctuation(ch, lang, has_preedit) {
            if has_preedit {
                let word = self
                    .candidates
                    .first()
                    .map(|c| c.text.clone())
                    .unwrap_or_else(|| self.input_buf.clone());
                let out = format!("{}{}", word, punct_str);
                let keys_used = self.input_buf.len() + 1;
                self.clear_composition();
                self.record_commit(&out, keys_used);
                return KeyAction::Commit(out);
            } else if intercept_when_empty {
                self.record_commit(punct_str, 1);
                return KeyAction::Commit(punct_str.to_string());
            }
        }

        KeyAction::PassThrough
    }

    fn map_polyglot_punctuation(&mut self, ch: char, lang: LangMode, has_preedit: bool) -> Option<(&'static str, bool)> {
        match ch {
            '"' => {
                let s = if self.quote_double_open { "”" } else { "“" };
                self.quote_double_open = !self.quote_double_open;
                Some((s, true))
            }
            '<' if matches!(lang, LangMode::It | LangMode::Es) => Some(("«", true)),
            '>' if matches!(lang, LangMode::It | LangMode::Es) => Some(("»", true)),
            '$' if matches!(lang, LangMode::It | LangMode::Es) => Some(("€", true)),
            '\'' if lang == LangMode::It && !has_preedit => Some(("’", true)),
            '?' if lang == LangMode::Es => {
                if !has_preedit && !self.es_question_open {
                    self.es_question_open = true;
                    Some(("¿", true))
                } else {
                    self.es_question_open = false;
                    Some(("?", true))
                }
            }
            '!' if lang == LangMode::Es => {
                if !has_preedit && !self.es_exclam_open {
                    self.es_exclam_open = true;
                    Some(("¡", true))
                } else {
                    self.es_exclam_open = false;
                    Some(("!", true))
                }
            }
            ',' => Some((",", false)),
            '.' => Some((".", false)),
            '?' => Some(("?", false)),
            '!' => Some(("!", false)),
            ':' => Some((":", false)),
            ';' => Some((";", false)),
            '(' => Some(("(", false)),
            ')' => Some((")", false)),
            '[' => Some(("[", false)),
            ']' => Some(("]", false)),
            '{' => Some(("{", false)),
            '}' => Some(("}", false)),
            _ => None,
        }
    }

    fn apply_explicit_accent(base: char, modifier: char, lang: LangMode) -> Option<char> {
        match (base, modifier) {
            ('a', '\'') => Some('á'),
            ('e', '\'') => Some('é'),
            ('i', '\'') => Some(if lang == LangMode::It { 'ì' } else { 'í' }),
            ('o', '\'') => Some(if lang == LangMode::It { 'ò' } else { 'ó' }),
            ('u', '\'') => Some(if lang == LangMode::It { 'ù' } else { 'ú' }),
            ('n', '\'') | ('n', '~') => Some('ñ'),
            ('A', '\'') => Some('Á'),
            ('E', '\'') => Some('É'),
            ('I', '\'') => Some('Í'),
            ('O', '\'') => Some('Ó'),
            ('U', '\'') => Some('Ú'),
            ('N', '\'') | ('N', '~') => Some('Ñ'),
            ('a', '`') => Some('à'),
            ('e', '`') => Some('è'),
            ('i', '`') => Some('ì'),
            ('o', '`') => Some('ò'),
            ('u', '`') => Some('ù'),
            ('A', '`') => Some('À'),
            ('E', '`') => Some('È'),
            ('I', '`') => Some('Ì'),
            ('O', '`') => Some('Ò'),
            ('U', '`') => Some('Ù'),
            _ => None,
        }
    }

    // ========================================================================
    // CANDIDATE REFRESH & GHOST [TAB] PREDICTION ENGINE
    // ========================================================================
    pub fn refresh_candidates(&mut self) {
        self.candidates.clear();
        self.candidate_consumed.clear();
        self.preedit_segmented.clear();
        self.active_shape_hints.clear();
        self.ghost_tab_text = None;

        if self.input_buf.is_empty() {
            return;
        }

        // Case 1: Reverse Lookup Mode (`` ` `` prefix)
        if let Some(query) = self.input_buf.strip_prefix('`') {
            if query.is_empty() {
                self.candidates.push(Candidate {
                    text: "输入双拼查看鹤形四码拆字 (例: `he)".to_string(),
                    comment: "反查".to_string(),
                    layer: "hint".to_string(),
                });
                return;
            }
            for (code, list) in &self.flypy_map {
                if code.len() >= 3 && code.starts_with(query) && code.chars().all(|c| c.is_ascii_lowercase()) {
                    for cand in list {
                        if cand.text.chars().count() == 1 {
                            self.candidates.push(Candidate {
                                text: cand.text.clone(),
                                comment: format!("[{}]", code),
                                layer: "reverse".to_string(),
                            });
                        }
                    }
                }
            }
            self.candidates.sort_by(|a, b| a.comment.cmp(&b.comment));
            self.candidates.dedup_by(|a, b| a.text == b.text && a.comment == b.comment);
            self.candidates.truncate(30);
            return;
        }

        // Case 1b: Inline Expression Calculator (`=` prefix, e.g. `=125*38`, `=(100+250)/7`, `=2^10`)
        if let Some(expr) = self.input_buf.strip_prefix('=') {
            let trimmed = expr.trim();
            if trimmed.is_empty() {
                self.candidates.push(Candidate {
                    text: "输入算式实时计算 (例: =125*38 · 空格结果 · 分号含算式 · 单引号大写金额)".to_string(),
                    comment: "计算器".to_string(),
                    layer: "calc_hint".to_string(),
                });
                return;
            }
            if let Some(val) = Self::eval_math_expr(trimmed) {
                let num_str = Self::format_math_number(val);
                let eq_str = format!("{}={}", trimmed, num_str);
                let rmb_str = Self::to_chinese_financial_upper(val);
                self.candidates.push(Candidate {
                    text: num_str,
                    comment: "计算结果".to_string(),
                    layer: "calc".to_string(),
                });
                self.candidates.push(Candidate {
                    text: eq_str,
                    comment: "; 算式".to_string(),
                    layer: "calc".to_string(),
                });
                self.candidates.push(Candidate {
                    text: rmb_str,
                    comment: "' 大写金额".to_string(),
                    layer: "calc".to_string(),
                });
            } else {
                self.candidates.push(Candidate {
                    text: trimmed.to_string(),
                    comment: "继续输入算式…".to_string(),
                    layer: "calc_hint".to_string(),
                });
            }
            return;
        }

        // Case 1c: `;` / `;h` (Encrypted Recent Input History Recovery) & `;v` (Clipboard Quick Paste)
        if self.input_buf == ";" {
            self.candidates.push(Candidate {
                text: "；".to_string(),
                comment: "分号 (空格/分号上屏 · h历史 · v剪贴板 · f重输)".to_string(),
                layer: "punct".to_string(),
            });
            return;
        }
        if self.input_buf == ";h" {
            let items = if self.config.history_retention_mins == 0 {
                Vec::new()
            } else {
                let key = crate::vault::resolve_history_key(
                    &self.data_dir,
                    &self.config.history_pin_salt,
                    &self.config.history_wrapped_key,
                );
                let now_s = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let cutoff = now_s.saturating_sub(self.config.history_retention_mins * 60);
                crate::vault::read_history_items(&self.data_dir, &key, cutoff)
            };
            for (idx, it) in items.into_iter().take(9).enumerate() {
                self.candidates.push(Candidate {
                    text: it.text,
                    comment: format!("历史#{}", idx + 1),
                    layer: "history".to_string(),
                });
            }
            if self.candidates.is_empty() {
                self.candidates.push(Candidate {
                    text: "暂无本地临时历史记录".to_string(),
                    comment: "空".to_string(),
                    layer: "hint".to_string(),
                });
            }
            return;
        }
        if self.input_buf == ";v" {
            if let Some(clip) = Self::read_clipboard_text() {
                let clean = clip.trim();
                if !clean.is_empty() {
                    self.candidates.push(Candidate {
                        text: clean.to_string(),
                        comment: "剪贴板".to_string(),
                        layer: "clipboard".to_string(),
                    });
                    return;
                }
            }
            self.candidates.push(Candidate {
                text: String::new(),
                comment: "剪贴板为空".to_string(),
                layer: "hint".to_string(),
            });
            return;
        }

        // Case 2: Uppercase Polyglot pass-through inside ZhFlypy mode, or explicit EN/IT/ES mode
        let is_upper_poly = self.mode == LangMode::ZhFlypy
            && self
                .input_buf
                .chars()
                .next()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false);

        if is_upper_poly || matches!(self.mode, LangMode::En | LangMode::It | LangMode::Es) {
            self.refresh_polyglot_candidates(is_upper_poly);
            return;
        }

        // Case 3: Dynamic `o`-prefix Date / Time / Weekday / Datetime / Undo / Mic Macros (`orq`, `ouj`, `oxq`, `ors`, `ocx`, `oiu`)
        if self.input_buf == "oc" && !self.last_committed.is_empty() {
            let short_prev: String = self.last_committed.chars().take(12).collect();
            self.candidates.push(Candidate {
                text: format!("撤回「{}」", short_prev),
                comment: "再按 x 直撤".to_string(),
                layer: "macro".to_string(),
            });
        } else if self.input_buf == "oi" {
            self.candidates.push(Candidate {
                text: "🎙 开关麦克风语音输入".to_string(),
                comment: "再按 u 立即启停".to_string(),
                layer: "macro".to_string(),
            });
        } else if self.input_buf == "orq" {
            for d_str in Self::current_date_candidates() {
                self.candidates.push(Candidate {
                    text: d_str,
                    comment: "日期".to_string(),
                    layer: "macro".to_string(),
                });
            }
        } else if self.input_buf == "ouj" {
            for t_str in Self::current_time_candidates() {
                self.candidates.push(Candidate {
                    text: t_str,
                    comment: "时间".to_string(),
                    layer: "macro".to_string(),
                });
            }
        } else if self.input_buf == "oxq" {
            for w_str in Self::current_weekday_candidates() {
                self.candidates.push(Candidate {
                    text: w_str,
                    comment: "星期".to_string(),
                    layer: "macro".to_string(),
                });
            }
        } else if self.input_buf == "ors" {
            for dt_str in Self::current_datetime_candidates() {
                self.candidates.push(Candidate {
                    text: dt_str,
                    comment: "时间戳".to_string(),
                    layer: "macro".to_string(),
                });
            }
        }

        let scheme = self.config.input_scheme.as_str();
        // Dedicated World-Class Intelligent Quanpin & Shuangpin Whole-Sentence Viterbi Lattice Engine!
        if (scheme == "quanpin" || scheme == "shuangpin")
            && self.input_buf.chars().all(|c| c.is_ascii_lowercase() || c == '\'')
        {
            self.refresh_pinyin_lattice_candidates();

            // Attach Foreign Language Immersion Gloss (EN / IT / ES / ALL) as vertical sub-lines
            if !self.config.immersion_mode.is_empty() && !self.config.immersion_mode.eq_ignore_ascii_case("OFF") {
                let imm_list: Vec<String> = self
                    .candidates
                    .iter()
                    .map(|c| self.format_immersion_for_candidate(&c.text))
                    .collect();
                for (cand, imm) in self.candidates.iter_mut().zip(imm_list.into_iter()) {
                    if !imm.is_empty() && !cand.comment.contains(&imm) {
                        cand.comment.push_str(&imm);
                    }
                }
            }

            // Convert candidates & ghost tab to Traditional Chinese when enabled
            if self.config.traditional_chinese && self.mode == LangMode::ZhFlypy {
                let mut trad_consumed = HashMap::new();
                for (k, v) in &self.candidate_consumed {
                    trad_consumed.insert(Self::to_traditional_chinese(k), *v);
                }
                self.candidate_consumed.extend(trad_consumed);
                for cand in &mut self.candidates {
                    cand.text = Self::to_traditional_chinese(&cand.text);
                }
                if let Some((gt, gl)) = &mut self.ghost_tab_text {
                    *gt = Self::to_traditional_chinese(gt);
                    *gl = Self::to_traditional_chinese(gl);
                }
            }
            return;
        }

        let lookup_codes: Vec<String> = vec![self.input_buf.clone()];

        // Case 4: User Custom Top Words (`user_custom.tsv`) with live template variable expansion
        for code in &lookup_codes {
            if let Some(u_cands) = self.user_map.get(code) {
                for c in u_cands {
                    let expanded = Self::expand_user_template(&c.text);
                    if !self.candidates.iter().any(|existing| existing.text == expanded) {
                        let mut cand = c.clone();
                        cand.text = expanded;
                        self.candidates.push(cand);
                    }
                }
            }
        }

        // Case 5: Core Clean-Room Heyu Dictionary (`heyu_sys.tsv`)
        for code in &lookup_codes {
            if let Some(sys_cands) = self.flypy_map.get(code) {
                if (scheme == "shuangpin" || (scheme == "quanpin" && self.input_buf.len() > 3)) && code.len() == 4 {
                    // In pure Shuangpin / multi-syllable Quanpin mode, 4-letter codes represent 2+ syllable phrases first!
                    for c in sys_cands.iter().filter(|c| c.text.chars().count() >= 2) {
                        if !self.candidates.iter().any(|existing| existing.text == c.text) {
                            self.candidates.push(Candidate {
                                text: c.text.clone(),
                                comment: String::new(),
                                layer: c.layer.clone(),
                            });
                        }
                    }
                    for c in sys_cands.iter().filter(|c| c.text.chars().count() == 1) {
                        if !self.candidates.iter().any(|existing| existing.text == c.text) {
                            self.candidates.push(Candidate {
                                text: c.text.clone(),
                                comment: String::new(),
                                layer: c.layer.clone(),
                            });
                        }
                    }
                } else {
                    for c in sys_cands {
                        if !self.candidates.iter().any(|existing| existing.text == c.text) {
                            let comment = if c.layer == "short2" {
                                ";".to_string()
                            } else {
                                String::new()
                            };
                            self.candidates.push(Candidate {
                                text: c.text.clone(),
                                comment,
                                layer: c.layer.clone(),
                            });
                        }
                    }
                }
            }
            // In pure Shuangpin or single-syllable Quanpin mode, expand 2-letter Shuangpin syllable to include all single characters from `sp2_hints` without requiring shape codes!
            if (scheme == "shuangpin" || scheme == "quanpin") && code.len() == 2 {
                if let Some(hints) = self.sp2_hints.get(code) {
                    for h in hints {
                        if !self.candidates.iter().any(|existing| existing.text == h.ch) {
                            self.candidates.push(Candidate {
                                text: h.ch.clone(),
                                comment: String::new(),
                                layer: "sp_char".to_string(),
                            });
                        }
                    }
                }
            }
        }

        // Case 5a: In `yinxing` mode at 2 letters, attach subtle shape-code hints to single-char candidates if `show_shape_hints` is enabled
        if scheme == "yinxing"
            && self.config.show_shape_hints
            && self.input_buf.len() == 2
            && self.input_buf.chars().all(|c| c.is_ascii_lowercase())
        {
            if let Some(hints) = self.sp2_hints.get(&self.input_buf) {
                for cand in &mut self.candidates {
                    if cand.text.chars().count() == 1 {
                        if let Some(h) = hints.iter().find(|h| h.ch == cand.text) {
                            let tag = format!("·{}", h.suffix);
                            if !cand.comment.contains(&tag) {
                                cand.comment.push_str(&tag);
                            }
                        }
                    }
                }
            }
        }

        // Case 5b: Attach Foreign Language Immersion Gloss (EN / IT / ES / ALL) as vertical sub-lines
        if !self.config.immersion_mode.is_empty() && !self.config.immersion_mode.eq_ignore_ascii_case("OFF") {
            let imm_list: Vec<String> = self
                .candidates
                .iter()
                .map(|c| self.format_immersion_for_candidate(&c.text))
                .collect();
            for (cand, imm) in self.candidates.iter_mut().zip(imm_list.into_iter()) {
                if !imm.is_empty() && !cand.comment.contains(&imm) {
                    cand.comment.push_str(&imm);
                }
            }
        }

        // Case 6: 2-Letter Shuangpin Shape-Code Memory Recovery Hints (`双形记忆唤醒提示`)
        if self.config.show_shape_hints
            && self.input_buf.len() == 2
            && self.input_buf.chars().all(|c| c.is_ascii_lowercase())
        {
            if let Some(hints) = self.sp2_hints.get(&self.input_buf) {
                self.active_shape_hints = hints.iter().take(6).cloned().collect();
            }
        }

        // Case 7: 220+ CPM Turbo Super-Phrases & Ghost [Tab] Completion!
        if self.config.enable_ghost_tab && self.input_buf.len() >= 2 && self.input_buf.len() <= 4 {
            for (code4, phrase) in &self.turbo_phrases {
                if self.input_buf.len() == 4 && code4 == &self.input_buf {
                    // Only inject into candidates if `code4` was empty in standard Heyu
                    // (so we NEVER break 4-code unique auto-commit for standard words!)
                    if self.candidates.is_empty() {
                        self.candidates.push(Candidate {
                            text: phrase.clone(),
                            comment: "极速长句".to_string(),
                            layer: "turbo".to_string(),
                        });
                    } else if self.ghost_tab_text.is_none() {
                        self.ghost_tab_text = Some((phrase.clone(), format!("⚡ [Tab] {}", phrase)));
                    }
                } else if self.input_buf.len() < 4 && code4.starts_with(&self.input_buf) {
                    if self.ghost_tab_text.is_none() {
                        self.ghost_tab_text =
                            Some((phrase.clone(), format!("⚡ [Tab] {} ({})", phrase, code4)));
                    }
                }
            }
        }

        // Case 8: Convert candidates, shape hints & ghost tab to Traditional Chinese when enabled
        if self.config.traditional_chinese && self.mode == LangMode::ZhFlypy {
            for cand in &mut self.candidates {
                cand.text = Self::to_traditional_chinese(&cand.text);
            }
            for h in &mut self.active_shape_hints {
                h.ch = Self::to_traditional_chinese(&h.ch);
            }
            if let Some((gt, gl)) = &mut self.ghost_tab_text {
                *gt = Self::to_traditional_chinese(gt);
                *gl = Self::to_traditional_chinese(gl);
            }
        }
    }

    /// High-speed, zero-dependency Simplified -> Traditional Chinese converter covering core characters.
    pub fn to_traditional_chinese(s: &str) -> String {
        const S2T: &str = concat!(
            "鹤鶴输輸国國语語电電脑腦软軟设設录錄历歷词詞库庫双雙码碼时時間間欢歡台臺湾灣门門开開关關东東车車马馬鸟鳥鱼魚龙龍凤鳳",
            "爱愛华華发發現現实實際際经經濟濟学學習習练練专專業業质質办辦会會議議记記编編輯輯删刪隐隱网網絡絡连連断斷线線听聽",
            "写寫说說話話读讀书書见見观觀认認識識选選擇擇确確错錯誤誤统統計計数數據據图圖报報处處应應显顯换換题題颜顏",
            "体體键鍵帮幫册冊万萬与與丑醜丛叢丝絲丢丟两兩严嚴丧喪个個丰豐临臨为為丽麗举舉么麼义義乌烏乐樂乔喬乡鄉买買乱亂争爭",
            "于於亏虧云雲亚亞产產亩畝亲親亿億仅僅从從仑侖仓倉仪儀们們价價众眾优優伙夥伟偉传傳伤傷伦倫伪偽余餘佣傭侠俠侣侶",
            "侥僥侦偵侧側侨僑侪儕侬儂俦儔俨儼俩倆俪儷俭儉债債倾傾偿償储儲儿兒兑兌党黨兰蘭兴興兹茲养養兽獸内內冈岡军軍农農",
            "冲衝决決况況冻凍净淨凄淒凉涼减減凑湊凛凜几幾凭憑凯凱击擊凿鑿划劃刘劉则則刚剛创創别別剂劑剑劍剥剝剧劇劝勸务務动動",
            "励勵劲勁劳勞势勢勋勳匀勻匮匱区區医醫协協单單卖賣卢盧卤鹵卧臥卫衛却卻厂廠厅廳厉厲压壓厌厭厕廁厢廂厦廈厨廚县縣参參",
            "变變叙敘叠疊只隻叶葉号號叹嘆吁籲后後吓嚇吕呂吗嗎吨噸员員呛嗆呜嗚咏詠咙嚨咸鹹响響哑啞哗嘩哟喲唤喚啧嘖啸嘯喷噴嘱囑",
            "嚣囂团團园園围圍圆圓圣聖场場坏壞块塊坚堅坛壇坝壩坞塢坟墳坠墜垄壟垒壘垦墾垫墊堑塹堕墮墙牆壮壯声聲壳殼壶壺备備",
            "复復够夠头頭夸誇夹夾夺奪奋奮奖獎奥奧妆妝妇婦妈媽姗姍娇嬌娱娛娴嫻婴嬰婶嬸孙孫孪孿宁寧宝寶宠寵审審宪憲宫宮宽寬宾賓",
            "寝寢寻尋导導寿壽将將尔爾尘塵尝嘗尧堯尴尷尸屍尽盡层層屉屜届屆属屬屡屢屿嶼岁歲岂豈岗崗岚嵐岛島岭嶺岳嶽峡峽",
            "崭嶄嵘嶸巅巔巩鞏币幣帅帥师師帐帳帘簾带帶帧幀帼幗幂冪干幹并並广廣庄莊庆慶庐廬庙廟庞龐废廢异異弃棄弯彎弹彈强強归歸",
            "当當彻徹径徑忆憶忏懺忧憂怀懷态態怂慫怅悵怆愴怜憐总總恋戀恳懇恶惡恺愷恻惻恼惱悦悅悬懸悯憫惊驚惧懼惨慘惩懲惫憊惬愜",
            "惭慚惮憚惯慣愤憤愿願慑懾虑慮懒懶戏戲战戰户戶扑撲执執扩擴扫掃扬揚扰擾抚撫抛拋抢搶护護担擔拟擬拢攏拣揀拥擁拦攔拧擰",
            "拨撥择擇挂掛挚摯挟挾挠撓挡擋挣掙挤擠挥揮捞撈损損捡撿捣搗据據掺摻搁擱搂摟搅攪携攜摄攝摆擺摇搖摊攤撑撐撵攆攒攢敌敵",
            "敛斂驱驅验驗骑騎骗騙骚騷骤驟驴驢驶駛驹駒驻駐驼駝驾駕驿驛骂罵骄驕骆駱骇駭骋騁骏駿鬓鬢松鬆闹鬧郁鬱鲜鮮鲤鯉鲨鯊鲫鯽",
            "鲸鯨鳄鱷鳍鰭鳖鱉鳞鱗鸠鳩鸡雞鸢鳶鸣鳴鸥鷗鸦鴉鸭鴨鸳鴛鸵鴕鸽鴿鸾鸞鸿鴻鹃鵑鹅鵝鹈鵜鹉鵡鹊鵲鹏鵬鹦鸚鹫鷲鹭鷺鹰鷹鹳鸛",
            "盐鹽麦麥黄黃点點齐齊齿齒龄齡龟龜测測试試"
        );
        if s.is_ascii() {
            return s.to_string();
        }
        let mut out = String::with_capacity(s.len());
        let chars: Vec<char> = S2T.chars().collect();
        for ch in s.chars() {
            if ch.is_ascii() {
                out.push(ch);
                continue;
            }
            let mut mapped = ch;
            let mut i = 0;
            while i + 1 < chars.len() {
                if chars[i] == ch {
                    mapped = chars[i + 1];
                    break;
                }
                i += 2;
            }
            out.push(mapped);
        }
        out
    }

    fn refresh_polyglot_candidates(&mut self, all_langs: bool) {
        let ascii_key: String = self
            .input_buf
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_lowercase();
        let first_upper = self
            .input_buf
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false);
        let all_upper = self.input_buf.len() > 1 && self.input_buf.chars().all(|c| c.is_uppercase());

        let apply_case = |word: &str| -> String {
            if all_upper {
                word.to_uppercase()
            } else if first_upper {
                let mut chs = word.chars();
                match chs.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chs.as_str(),
                    None => String::new(),
                }
            } else {
                word.to_string()
            }
        };

        let mut exact_matches: Vec<(String, u64, String)> = Vec::new();
        let mut prefix_matches: Vec<(String, u64, String)> = Vec::new();

        let dicts: Vec<(&str, &HashMap<String, Vec<PolyglotEntry>>, &HashMap<String, Vec<PolyglotEntry>>)> =
            if all_langs {
                vec![
                    ("IT", &self.poly_it, &self.prefix_it),
                    ("ES", &self.poly_es, &self.prefix_es),
                    ("EN", &self.poly_en, &self.prefix_en),
                ]
            } else {
                match self.mode {
                    LangMode::It => vec![
                        ("IT", &self.poly_it, &self.prefix_it),
                        ("EN", &self.poly_en, &self.prefix_en),
                    ],
                    LangMode::Es => vec![
                        ("ES", &self.poly_es, &self.prefix_es),
                        ("EN", &self.poly_en, &self.prefix_en),
                    ],
                    _ => vec![
                        ("EN", &self.poly_en, &self.prefix_en),
                        ("IT", &self.poly_it, &self.prefix_it),
                        ("ES", &self.poly_es, &self.prefix_es),
                    ],
                }
            };

        for (tag, exact_map, pfx_map) in &dicts {
            if let Some(list) = exact_map.get(&ascii_key) {
                for e in list {
                    let cased = apply_case(&e.word);
                    let comment = if !e.gloss.is_empty() {
                        format!("[{}] {}", tag, e.gloss)
                    } else {
                        format!("[{}]", tag)
                    };
                    exact_matches.push((cased, e.freq, comment));
                }
            }
            if ascii_key.len() >= 2 && ascii_key.len() <= 6 {
                if let Some(list) = pfx_map.get(&ascii_key) {
                    for e in list {
                        let cased = apply_case(&e.word);
                        let comment = if !e.gloss.is_empty() {
                            format!("[{}] {}", tag, e.gloss)
                        } else {
                            format!("[{}]", tag)
                        };
                        prefix_matches.push((cased, e.freq, comment));
                    }
                }
            }
        }

        // Sort exact matches so accented native matches (e.g. `perché`, `español`, `año`, `più`) win #1
        exact_matches.sort_by(|a, b| b.1.cmp(&a.1));
        for (w, _, comm) in exact_matches {
            if !self.candidates.iter().any(|c| c.text == w) {
                self.candidates.push(Candidate {
                    text: w,
                    comment: comm,
                    layer: "poly_exact".to_string(),
                });
            }
        }

        // Ensure raw typed string is always available (as #1 if no exact match, or #2 if accented match is #1)
        if !self.candidates.iter().any(|c| c.text == self.input_buf) {
            self.candidates.push(Candidate {
                text: self.input_buf.clone(),
                comment: "[原词]".to_string(),
                layer: "raw".to_string(),
            });
        }

        // Sort prefix completions by frequency and offer #1 as Ghost [Tab] completion!
        prefix_matches.sort_by(|a, b| b.1.cmp(&a.1));
        for (w, _, comm) in prefix_matches {
            if !self.candidates.iter().any(|c| c.text == w) {
                if self.ghost_tab_text.is_none() {
                    self.ghost_tab_text = Some((w.clone(), format!("⚡ [Tab] {} {}", w, comm)));
                }
                self.candidates.push(Candidate {
                    text: w,
                    comment: comm,
                    layer: "poly_prefix".to_string(),
                });
            }
        }
        self.candidates.truncate(15);
    }

    /// Local wall-clock time from the OS (respects time zone and DST).
    fn local_time() -> (u16, u16, u16, u16, u16, u16, u16) {
        #[repr(C)]
        #[derive(Default)]
        struct SystemTimeW {
            year: u16,
            month: u16,
            day_of_week: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            millis: u16,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetLocalTime(st: *mut SystemTimeW);
        }
        let mut st = SystemTimeW::default();
        unsafe { GetLocalTime(&mut st) };
        (st.year, st.month, st.day, st.day_of_week, st.hour, st.minute, st.second)
    }

    fn zh_upper_num(mut n: u16) -> String {
        let digits = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
        if n >= 1000 {
            let mut chars = Vec::new();
            while n > 0 {
                chars.push(digits[(n % 10) as usize]);
                n /= 10;
            }
            chars.reverse();
            return chars.into_iter().collect();
        }
        if n < 10 {
            return digits[n as usize].to_string();
        }
        let tens = n / 10;
        let ones = n % 10;
        let mut s = String::new();
        if tens > 1 {
            s.push(digits[tens as usize]);
        }
        s.push('十');
        if ones > 0 {
            s.push(digits[ones as usize]);
        }
        s
    }

    fn current_date_candidates() -> Vec<String> {
        let (y, m, d, _, _, _, _) = Self::local_time();
        let m_idx = (m.saturating_sub(1) as usize).min(11);
        let it_months = [
            "gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno",
            "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre",
        ];
        let es_months = [
            "enero", "febrero", "marzo", "abril", "mayo", "junio",
            "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre",
        ];
        vec![
            format!("{}年{}月{}日", y, m, d),
            format!("{:04}-{:02}-{:02}", y, m, d),
            format!("{:02}/{:02}/{:04}", d, m, y),
            format!("{:04}/{:02}/{:02}", y, m, d),
            format!("{:04}.{:02}.{:02}", y, m, d),
            format!("{:04}{:02}{:02}", y, m, d),
            format!("{}年{}月{}日", Self::zh_upper_num(y), Self::zh_upper_num(m), Self::zh_upper_num(d)),
            format!("{} {} {}", d, it_months[m_idx], y),
            format!("{} de {} de {}", d, es_months[m_idx], y),
        ]
    }

    fn current_time_candidates() -> Vec<String> {
        let (y, m, d, _, hh, mm, ss) = Self::local_time();
        vec![
            format!("{:02}:{:02}", hh, mm),
            format!("{}点{:02}分", hh, mm),
            format!("{:02}:{:02}:{:02}", hh, mm, ss),
            format!("{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, hh, mm),
        ]
    }

    fn current_weekday_candidates() -> Vec<String> {
        let (_, _, _, wd, _, _, _) = Self::local_time();
        let i = (wd as usize) % 7;
        let zh = ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"];
        let zh_short = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
        let it = ["domenica", "lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato"];
        let es = ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"];
        let en = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
        vec![
            zh[i].to_string(),
            zh_short[i].to_string(),
            it[i].to_string(),
            es[i].to_string(),
            en[i].to_string(),
        ]
    }

    fn current_datetime_candidates() -> Vec<String> {
        let (y, m, d, _, hh, mm, ss) = Self::local_time();
        vec![
            format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, hh, mm, ss),
            format!("{}年{}月{}日 {:02}:{:02}", y, m, d, hh, mm),
            format!("{:04}{:02}{:02}_{:02}{:02}{:02}", y, m, d, hh, mm, ss),
        ]
    }

    pub fn expand_user_template(raw: &str) -> String {
        if !raw.contains('{') {
            return raw.to_string();
        }
        let (y, m, d, wd, hh, mm, ss) = Self::local_time();
        let zh_wd = ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"][(wd as usize) % 7];
        raw.replace("{date}", &format!("{:04}-{:02}-{:02}", y, m, d))
            .replace("{date_zh}", &format!("{}年{}月{}日", y, m, d))
            .replace("{date_slash}", &format!("{:04}/{:02}/{:02}", y, m, d))
            .replace("{date_compact}", &format!("{:04}{:02}{:02}", y, m, d))
            .replace("{time}", &format!("{:02}:{:02}", hh, mm))
            .replace("{time_sec}", &format!("{:02}:{:02}:{:02}", hh, mm, ss))
            .replace("{datetime}", &format!("{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, hh, mm))
            .replace("{weekday}", zh_wd)
            .replace("{year}", &format!("{:04}", y))
            .replace("{month}", &format!("{:02}", m))
            .replace("{day}", &format!("{:02}", d))
    }

    pub fn read_clipboard_text() -> Option<String> {
        crate::win32::clipboard_text()
    }

    pub fn eval_math_expr(expr: &str) -> Option<f64> {
        struct Parser<'a> {
            b: &'a [u8],
            pos: usize,
        }
        impl<'a> Parser<'a> {
            fn skip_ws(&mut self) {
                while self.pos < self.b.len() && self.b[self.pos].is_ascii_whitespace() {
                    self.pos += 1;
                }
            }
            fn parse_expr(&mut self) -> Option<f64> {
                let mut v = self.parse_term()?;
                loop {
                    self.skip_ws();
                    if self.pos >= self.b.len() {
                        break;
                    }
                    match self.b[self.pos] {
                        b'+' => {
                            self.pos += 1;
                            v += self.parse_term()?;
                        }
                        b'-' => {
                            self.pos += 1;
                            v -= self.parse_term()?;
                        }
                        _ => break,
                    }
                }
                Some(v)
            }
            fn parse_term(&mut self) -> Option<f64> {
                let mut v = self.parse_power()?;
                loop {
                    self.skip_ws();
                    if self.pos >= self.b.len() {
                        break;
                    }
                    match self.b[self.pos] {
                        b'*' => {
                            self.pos += 1;
                            v *= self.parse_power()?;
                        }
                        b'/' => {
                            self.pos += 1;
                            let d = self.parse_power()?;
                            if d.abs() < 1e-15 {
                                return None;
                            }
                            v /= d;
                        }
                        b'%' => {
                            self.pos += 1;
                            let d = self.parse_power()?;
                            if d.abs() < 1e-15 {
                                return None;
                            }
                            v %= d;
                        }
                        _ => break,
                    }
                }
                Some(v)
            }
            fn parse_power(&mut self) -> Option<f64> {
                let base = self.parse_unary()?;
                self.skip_ws();
                if self.pos < self.b.len() && self.b[self.pos] == b'^' {
                    self.pos += 1;
                    let exp = self.parse_power()?;
                    Some(base.powf(exp))
                } else {
                    Some(base)
                }
            }
            fn parse_unary(&mut self) -> Option<f64> {
                self.skip_ws();
                if self.pos < self.b.len() {
                    if self.b[self.pos] == b'+' {
                        self.pos += 1;
                        return self.parse_unary();
                    }
                    if self.b[self.pos] == b'-' {
                        self.pos += 1;
                        return self.parse_unary().map(|v| -v);
                    }
                }
                self.parse_atom()
            }
            fn parse_atom(&mut self) -> Option<f64> {
                self.skip_ws();
                if self.pos >= self.b.len() {
                    return None;
                }
                if self.b[self.pos] == b'(' {
                    self.pos += 1;
                    let v = self.parse_expr()?;
                    self.skip_ws();
                    if self.pos >= self.b.len() || self.b[self.pos] != b')' {
                        return None;
                    }
                    self.pos += 1;
                    return Some(v);
                }
                let start = self.pos;
                let mut seen_dot = false;
                while self.pos < self.b.len() {
                    let c = self.b[self.pos];
                    if c.is_ascii_digit() {
                        self.pos += 1;
                    } else if c == b'.' && !seen_dot {
                        seen_dot = true;
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                if self.pos == start {
                    return None;
                }
                let s = std::str::from_utf8(&self.b[start..self.pos]).ok()?;
                s.parse::<f64>().ok()
            }
        }
        let mut p = Parser {
            b: expr.as_bytes(),
            pos: 0,
        };
        let v = p.parse_expr()?;
        p.skip_ws();
        if p.pos != p.b.len() || !v.is_finite() {
            return None;
        }
        Some(v)
    }

    pub fn format_math_number(val: f64) -> String {
        if (val - val.round()).abs() < 1e-9 && val.abs() < 1e15 {
            format!("{}", val.round() as i64)
        } else {
            let s = format!("{:.8}", val);
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        }
    }

    pub fn to_chinese_financial_upper(val: f64) -> String {
        const DIGITS: [char; 10] = ['零', '壹', '贰', '叁', '肆', '伍', '陆', '柒', '捌', '玖'];
        const UNITS: [&str; 4] = ["", "拾", "佰", "仟"];
        const BIG_UNITS: [&str; 4] = ["", "万", "亿", "兆"];

        if !val.is_finite() || val.abs() >= 1e15 {
            return Self::format_math_number(val);
        }
        let neg = val < 0.0;
        let abs_v = val.abs();
        let total_cents = (abs_v * 100.0).round() as u64;
        if total_cents == 0 {
            return "零元整".to_string();
        }
        let int_part = total_cents / 100;
        let jiao = ((total_cents % 100) / 10) as usize;
        let fen = (total_cents % 10) as usize;

        let mut out = String::new();
        if neg {
            out.push('负');
        }

        if int_part > 0 {
            let mut groups = Vec::new();
            let mut rem = int_part;
            while rem > 0 {
                groups.push((rem % 10000) as u16);
                rem /= 10000;
            }
            let mut need_zero = false;
            for (g_idx, &g) in groups.iter().enumerate().rev() {
                if g == 0 {
                    need_zero = true;
                    continue;
                }
                if need_zero || (g < 1000 && g_idx < groups.len() - 1) {
                    if !out.ends_with('零') && !out.is_empty() && out != "负" {
                        out.push('零');
                    }
                }
                let d = [
                    (g / 1000) as usize,
                    ((g / 100) % 10) as usize,
                    ((g / 10) % 10) as usize,
                    (g % 10) as usize,
                ];
                let mut prev_zero = false;
                let mut started = false;
                for i in 0..4 {
                    if d[i] == 0 {
                        if started {
                            prev_zero = true;
                        }
                    } else {
                        if prev_zero {
                            out.push('零');
                            prev_zero = false;
                        }
                        out.push(DIGITS[d[i]]);
                        out.push_str(UNITS[3 - i]);
                        started = true;
                    }
                }
                out.push_str(BIG_UNITS[g_idx.min(3)]);
                need_zero = false;
            }
            out.push('元');
        }

        if jiao == 0 && fen == 0 {
            out.push('整');
        } else {
            if jiao > 0 {
                out.push(DIGITS[jiao]);
                out.push('角');
                if fen > 0 {
                    out.push(DIGITS[fen]);
                    out.push('分');
                }
            } else {
                if int_part > 0 {
                    out.push('零');
                }
                out.push(DIGITS[fen]);
                out.push('分');
            }
        }
        out
    }
}

// ============================================================================
// WORLD-CLASS INTELLIGENT QUANPIN & SHUANGPIN LATTICE + VITERBI SENTENCE ENGINE
// ============================================================================

/// Convert a single Full Pinyin syllable (e.g. `"shuang"`, `"he"`, `"zhong"`, `"ang"`) into its
/// exact 2-letter Xiaohe Shuangpin code, validating Mandarin `(initial, final)` compatibility
/// so invalid combinations (like `"jua"` or `"bia"` or `"din"`) are rejected during segmentation.
pub fn pinyin_syllable_to_sp2(syl: &str) -> Option<String> {
    match syl {
        "a" => return Some("aa".to_string()),
        "o" => return Some("oo".to_string()),
        "e" => return Some("ee".to_string()),
        "ai" | "ei" | "ao" | "ou" | "an" | "en" | "er" => return Some(syl.to_string()),
        "ang" => return Some("ah".to_string()),
        "eng" => return Some("eg".to_string()),
        // Strict `y*` valid Mandarin syllables
        "ya" => return Some("ya".to_string()),
        "yao" => return Some("yc".to_string()),
        "yan" => return Some("yj".to_string()),
        "yang" => return Some("yh".to_string()),
        "ye" => return Some("ye".to_string()),
        "yi" => return Some("yi".to_string()),
        "yin" => return Some("yb".to_string()),
        "ying" => return Some("yk".to_string()),
        "yo" => return Some("yo".to_string()),
        "yong" => return Some("ys".to_string()),
        "you" => return Some("yz".to_string()),
        "yu" | "yv" => return Some("yu".to_string()),
        "yuan" | "yvan" => return Some("yr".to_string()),
        "yue" | "yve" => return Some("yt".to_string()),
        "yun" | "yvn" => return Some("yy".to_string()),
        // Strict `w*` valid Mandarin syllables
        "wa" => return Some("wa".to_string()),
        "wai" => return Some("wd".to_string()),
        "wan" => return Some("wj".to_string()),
        "wang" => return Some("wh".to_string()),
        "wei" => return Some("ww".to_string()),
        "wen" => return Some("wf".to_string()),
        "weng" => return Some("wg".to_string()),
        "wo" | "wuo" => return Some("wo".to_string()),
        "wu" => return Some("wu".to_string()),
        _ => {}
    }
    if syl.starts_with('y') || syl.starts_with('w') {
        return None;
    }
    let (init_code, fin) = if let Some(r) = syl.strip_prefix("zh") {
        ('v', r)
    } else if let Some(r) = syl.strip_prefix("ch") {
        ('i', r)
    } else if let Some(r) = syl.strip_prefix("sh") {
        ('u', r)
    } else {
        let first = syl.chars().next()?;
        if matches!(
            first,
            'b' | 'p' | 'm' | 'f' | 'd' | 't' | 'n' | 'l' | 'g' | 'k' | 'h' | 'j' | 'q' | 'x' | 'r' | 'z' | 'c' | 's'
        ) {
            (first, &syl[1..])
        } else {
            return None;
        }
    };
    let is_jqx = matches!(init_code, 'j' | 'q' | 'x');
    let is_bpmf = matches!(init_code, 'b' | 'p' | 'm' | 'f');
    let is_gkh_zh_r_zcs = matches!(init_code, 'g' | 'k' | 'h' | 'v' | 'i' | 'u' | 'r' | 'z' | 'c' | 's');

    let fin_code = match fin {
        "a" if !is_jqx => 'a',
        "o" if is_bpmf || init_code == 'l' => 'o',
        "uo" if !is_jqx => 'o',
        "e" if !is_jqx && !matches!(init_code, 'b' | 'p' | 'f') => 'e',
        "i" if !matches!(init_code, 'f' | 'g' | 'k' | 'h') => 'i',
        "u" => 'u',
        "v" if matches!(init_code, 'n' | 'l') => 'v',
        "v" if is_jqx => 'u',
        "ai" if !is_jqx && !matches!(init_code, 'f' | 'r') => 'd',
        "ei" if !is_jqx && !matches!(init_code, 't' | 'c' | 's' | 'r') => 'w',
        "ui" | "uei" if matches!(init_code, 'd' | 't' | 'g' | 'k' | 'h' | 'v' | 'i' | 'u' | 'r' | 'z' | 'c' | 's') => 'v',
        "ao" if !is_jqx && init_code != 'f' => 'c',
        "ou" if !is_jqx && !matches!(init_code, 'b') => 'z',
        "iu" | "iou" if matches!(init_code, 'm' | 'd' | 'n' | 'l' | 'j' | 'q' | 'x') => 'q',
        "ie" if matches!(init_code, 'b' | 'p' | 'm' | 'd' | 't' | 'n' | 'l' | 'j' | 'q' | 'x') => 'p',
        "ve" | "ue" if matches!(init_code, 'n' | 'l' | 'j' | 'q' | 'x') => 't',
        "an" if !is_jqx => 'j',
        "en" if !is_jqx && !matches!(init_code, 't' | 'l') => 'f',
        "in" if matches!(init_code, 'b' | 'p' | 'm' | 'n' | 'l' | 'j' | 'q' | 'x') => 'b',
        "un" | "uen" if matches!(init_code, 'd' | 't' | 'l' | 'g' | 'k' | 'h' | 'v' | 'i' | 'u' | 'r' | 'z' | 'c' | 's' | 'j' | 'q' | 'x') => 'y',
        "ang" if !is_jqx => 'h',
        "eng" if !is_jqx => 'g',
        "ing" if matches!(init_code, 'b' | 'p' | 'm' | 'd' | 't' | 'n' | 'l' | 'j' | 'q' | 'x') => 'k',
        "uai" if matches!(init_code, 'g' | 'k' | 'h' | 'v' | 'i' | 'u' | 'r') => 'k',
        "ong" if matches!(init_code, 'd' | 't' | 'n' | 'l' | 'g' | 'k' | 'h' | 'v' | 'i' | 'r' | 'z' | 'c' | 's') => 's',
        "iong" if is_jqx => 's',
        "ia" if matches!(init_code, 'd' | 'l' | 'j' | 'q' | 'x') => 'x',
        "ua" if matches!(init_code, 'g' | 'k' | 'h' | 'v' | 'i' | 'u' | 'r') => 'x',
        "uan" if !is_bpmf => 'r',
        "uang" if is_gkh_zh_r_zcs => 'l',
        "iang" if matches!(init_code, 'n' | 'l' | 'j' | 'q' | 'x') => 'l',
        "iao" if matches!(init_code, 'b' | 'p' | 'm' | 'd' | 't' | 'n' | 'l' | 'j' | 'q' | 'x') => 'n',
        "ian" if matches!(init_code, 'b' | 'p' | 'm' | 'd' | 't' | 'n' | 'l' | 'j' | 'q' | 'x') => 'm',
        _ => return None,
    };
    let mut s = String::with_capacity(2);
    s.push(init_code);
    s.push(fin_code);
    Some(s)
}

/// Auto-correct common high-speed Full Pinyin keyboard transpositions (`ign->ing`, `unag->uang`,
/// `inag->iang`, `agn->ang`, `egn->eng`, `ogn->ong`, `on->ong`, `gou->guo`, `abn->ban`, etc.).
fn typo_pinyin_to_sp2(syl: &str) -> Option<String> {
    if syl.len() < 2 {
        return None;
    }
    let fixed: Option<String> = if let Some(pfx) = syl.strip_suffix("ign") {
        Some(format!("{}ing", pfx))
    } else if let Some(pfx) = syl.strip_suffix("agn") {
        Some(format!("{}ang", pfx))
    } else if let Some(pfx) = syl.strip_suffix("egn") {
        Some(format!("{}eng", pfx))
    } else if let Some(pfx) = syl.strip_suffix("ogn") {
        Some(format!("{}ong", pfx))
    } else if let Some(pfx) = syl.strip_suffix("unag") {
        Some(format!("{}uang", pfx))
    } else if let Some(pfx) = syl.strip_suffix("uagn") {
        Some(format!("{}uang", pfx))
    } else if let Some(pfx) = syl.strip_suffix("inag") {
        Some(format!("{}iang", pfx))
    } else if let Some(pfx) = syl.strip_suffix("iagn") {
        Some(format!("{}iang", pfx))
    } else if let Some(pfx) = syl.strip_suffix("on") {
        Some(format!("{}ong", pfx))
    } else if let Some(pfx) = syl.strip_suffix("gou") {
        Some(format!("{}guo", pfx))
    } else if let Some(pfx) = syl.strip_suffix("abn") {
        Some(format!("{}ban", pfx))
    } else if let Some(pfx) = syl.strip_suffix("ina") {
        Some(format!("{}ian", pfx))
    } else if let Some(pfx) = syl.strip_suffix("una") {
        Some(format!("{}uan", pfx))
    } else {
        None
    };
    fixed.and_then(|f| pinyin_syllable_to_sp2(&f))
}

/// Incomplete mid-typing Pinyin prefix expansion (e.g. `"zho"`, `"zhon"`, `"xio"`, `"xion"`, `"shua"`)
/// so the candidate bar never goes blank while typing a syllable.
fn prefix_pinyin_to_sp2_list(pfx: &str) -> Vec<String> {
    const COMMON_SUFFIXES: &[&str] = &["g", "ng", "n", "i", "u", "o", "a", "e", "ang", "ing", "ong", "uan", "uang", "ian", "iang", "iao"];
    let mut out = Vec::new();
    for sfx in COMMON_SUFFIXES {
        let trial = format!("{}{}", pfx, sfx);
        if let Some(sp2) = pinyin_syllable_to_sp2(&trial) {
            if !out.contains(&sp2) {
                out.push(sp2);
            }
        }
    }
    out
}

/// Generate Fuzzy Pinyin (`模糊音`) 2-letter `sp2` variants (`z<->zh`, `c<->ch`, `s<->sh`, `n<->l`,
/// `in<->ing`, `en<->eng`, `an<->ang`, `ian<->iang`, `uan<->uang`, `lu<->lv`, `nu<->nv`).
fn fuzzy_sp2_variants(sp2: &str) -> Vec<String> {
    if sp2.len() != 2 {
        return Vec::new();
    }
    let bytes = sp2.as_bytes();
    let i0 = bytes[0] as char;
    let f0 = bytes[1] as char;

    let mut inits = vec![i0];
    match i0 {
        'z' => inits.push('v'),
        'v' => inits.push('z'),
        'c' => inits.push('i'),
        'i' => inits.push('c'),
        's' => inits.push('u'),
        'u' => inits.push('s'),
        'n' => inits.push('l'),
        'l' => inits.push('n'),
        _ => {}
    }

    let mut fins = vec![f0];
    match f0 {
        'b' => fins.push('k'), // in <-> ing
        'k' if !matches!(i0, 'g' | 'k' | 'h' | 'v' | 'i' | 'u' | 'r') => fins.push('b'),
        'f' => fins.push('g'), // en <-> eng
        'g' => fins.push('f'),
        'j' => fins.push('h'), // an <-> ang
        'h' => fins.push('j'),
        'u' if matches!(i0, 'n' | 'l') => fins.push('v'), // lu <-> lv, nu <-> nv
        'v' if matches!(i0, 'n' | 'l') => fins.push('u'),
        _ => {}
    }

    let mut out = Vec::new();
    for &ic in &inits {
        for &fc in &fins {
            if ic == i0 && fc == f0 {
                continue;
            }
            let mut s = String::with_capacity(2);
            s.push(ic);
            s.push(fc);
            out.push(s);
        }
    }
    out
}

/// Convert a Full Pinyin (`quanpin`) input string into candidate 2-letter or 4-letter Xiaohe Shuangpin codes.
/// Examples:
/// * `"shuang"` -> `["ul"]`
/// * `"zhongguo"` -> `["vsgo"]`
/// * `"shuangpin"` -> `["ulpb"]`
/// * `"heyu"` -> `["heyu"]`
pub fn quanpin_to_shuangpin_codes(qp: &str) -> Vec<String> {
    let s = qp.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    // 1. Direct single syllable match
    if let Some(sp2) = pinyin_syllable_to_sp2(&s) {
        out.push(sp2);
    }
    // 2. Multi-syllable segmentation (longest-match with backtracking)
    fn segment(rem: &str, cur: &mut Vec<String>, results: &mut Vec<Vec<String>>) {
        if results.len() >= 4 {
            return;
        }
        if rem.is_empty() {
            if cur.len() >= 2 {
                results.push(cur.clone());
            }
            return;
        }
        if cur.len() >= 8 {
            return;
        }
        let max_len = rem.len().min(6);
        for len in (1..=max_len).rev() {
            let pfx = &rem[..len];
            if let Some(sp2) = pinyin_syllable_to_sp2(pfx) {
                cur.push(sp2);
                segment(&rem[len..], cur, results);
                cur.pop();
            }
        }
    }
    let mut segs = Vec::new();
    let mut cur = Vec::new();
    segment(&s, &mut cur, &mut segs);
    for seg in segs {
        let code = match seg.len() {
            0 => continue,
            1 => seg[0].clone(),
            2 => format!("{}{}", seg[0], seg[1]),
            3 => format!("{}{}{}", &seg[0][..1], &seg[1][..1], seg[2]),
            n => format!("{}{}{}{}", &seg[0][..1], &seg[1][..1], &seg[2][..1], &seg[n - 1][..1]),
        };
        if !out.contains(&code) {
            out.push(code);
        }
    }
    out
}

#[derive(Clone, Debug)]
struct SylEdge {
    end: usize,
    raw: String,
    sp2_cands: Vec<(String, f64)>,
    init_char: Option<char>,
    is_full: bool,
}

impl HeyuEngine {
    fn insert_sp_char(&mut self, sp2: &str, ch: &str, score: u32) {
        if sp2.len() != 2 || ch.is_empty() {
            return;
        }
        let list = self.sp_char_map.entry(sp2.to_string()).or_default();
        if let Some(existing) = list.iter_mut().find(|(c, _)| c == ch) {
            if score > existing.1 {
                existing.1 = score;
            }
        } else {
            list.push((ch.to_string(), score));
        }
    }

    fn insert_sp_word(&mut self, sp_seq: &str, word: &str, score: u32) {
        if sp_seq.len() < 4 || sp_seq.len() % 2 != 0 || word.chars().count() < 2 {
            return;
        }
        let list = self.sp_word_map.entry(sp_seq.to_string()).or_default();
        if let Some(existing) = list.iter_mut().find(|(w, _)| w == word) {
            if score > existing.1 {
                existing.1 = score;
            }
        } else {
            list.push((word.to_string(), score));
        }

        // Also index into `init_word_map` under both Shuangpin initials (`v/i/u`) AND standard Pinyin initials (`z/c/s`)
        let n = sp_seq.len() / 2;
        if (2..=8).contains(&n) {
            let mut sp_init = String::with_capacity(n);
            let mut py_init = String::with_capacity(n);
            let bytes = sp_seq.as_bytes();
            for k in 0..n {
                let c = bytes[k * 2] as char;
                sp_init.push(c);
                let pc = match c {
                    'v' => 'z',
                    'i' => 'c',
                    'u' => 's',
                    other => other,
                };
                py_init.push(pc);
            }
            for init_key in [&sp_init, &py_init] {
                let ilist = self.init_word_map.entry(init_key.clone()).or_default();
                if let Some(ex) = ilist.iter_mut().find(|(w, _)| w == word) {
                    if score > ex.1 {
                        ex.1 = score;
                    }
                } else {
                    ilist.push((word.to_string(), score));
                }
            }
            if score >= 88_000 {
                self.word_init_hint
                    .entry(word.to_string())
                    .or_insert_with(|| py_init);
            }
        }
    }

    fn char_sp2_matching_initial<'a>(&'a self, ch: char, expected_init: Option<char>) -> Option<&'a str> {
        let mut buf = [0u8; 4];
        let codes = self.reverse_map.get(ch.encode_utf8(&mut buf) as &str)?;
        if let Some(exp) = expected_init {
            for c in codes.split(',').map(str::trim) {
                if c.len() >= 2 && c.as_bytes()[0] as char == exp && c.chars().all(|x| x.is_ascii_alphabetic()) {
                    return Some(&c[..2]);
                }
            }
        }
        for c in codes.split(',').map(str::trim) {
            if c.len() >= 2 && c.chars().all(|x| x.is_ascii_alphabetic()) {
                return Some(&c[..2]);
            }
        }
        None
    }

    fn word_to_sp_seq(&self, word: &str, code_hint: Option<&str>) -> Option<String> {
        let chars: Vec<char> = word.chars().collect();
        let n = chars.len();
        if n < 2 {
            return None;
        }
        if let Some(code) = code_hint {
            if n == 2 && code.len() == 4 && code.chars().all(|c| c.is_ascii_lowercase()) {
                return Some(code.to_string());
            }
        }
        let hb = code_hint.map(|s| s.as_bytes()).unwrap_or(&[]);
        let mut out = String::with_capacity(n * 2);
        for (i, &ch) in chars.iter().enumerate() {
            if n == 3 && i == 2 && hb.len() == 4 {
                out.push(hb[2] as char);
                out.push(hb[3] as char);
                continue;
            }
            let exp_init = if n == 3 && hb.len() == 4 && i < 2 {
                Some(hb[i] as char)
            } else if n >= 4 && hb.len() == 4 {
                if i < 3 {
                    Some(hb[i] as char)
                } else if i == n - 1 {
                    Some(hb[3] as char)
                } else {
                    None
                }
            } else {
                None
            };
            let sp2 = self.char_sp2_matching_initial(ch, exp_init)?;
            out.push_str(sp2);
        }
        Some(out)
    }

    pub fn index_user_custom_into_lattice(&mut self) {
        let mut entries = Vec::new();
        for (code, list) in &self.user_map {
            for c in list {
                let expanded = Self::expand_user_template(&c.text);
                if expanded.chars().count() >= 2 && !expanded.contains('\n') {
                    entries.push((code.clone(), expanded));
                }
            }
        }
        for (code, word) in entries {
            if let Some(sp_seq) = self.word_to_sp_seq(&word, Some(&code)) {
                self.insert_sp_word(&sp_seq, &word, 220_000);
                if let Some(list) = self.sp_word_map.get_mut(&sp_seq) {
                    list.sort_by(|a, b| b.1.cmp(&a.1));
                }
            }
        }
    }

    /// Build the in-memory Full Pinyin & Shuangpin Character/Word/Initial Lattice Index.
    pub fn build_pinyin_lattice_index(&mut self) {
        self.sp_char_map.clear();
        self.sp_word_map.clear();
        self.init_word_map.clear();
        self.word_init_hint.clear();

        // 1. Curated high-frequency single characters per syllable (ensures natural #1 single-char ranking)
        const CORE_CHARS: &[(&str, &str)] = &[
            ("wo", "我握窝卧"), ("ni", "你尼泥拟"), ("ta", "他她它塔踏"), ("de", "的地得德"),
            ("ui", "是时事市实十使世师石识始式示势室试史食视"), ("bu", "不部步布补捕"),
            ("le", "了乐勒"), ("zd", "在再载灾"), ("yz", "有又由友游油右优"), ("ve", "这着者折哲浙"),
            ("ge", "个各歌哥格隔割"), ("rf", "人任认忍仁"), ("vs", "中重种众终钟忠肿"),
            ("uh", "上商伤尚赏"), ("xx", "下夏吓虾"), ("ld", "来莱赖"), ("qu", "去区取趣曲"),
            ("uo", "说硕烁"), ("kj", "看刊砍堪"), ("hc", "好号毫豪浩"), ("ma", "吗马妈麻码骂嘛"),
            ("ne", "呢呐"), ("ba", "吧把八爸罢拔巴"), ("ya", "呀压牙亚雅押"), ("aa", "啊阿呵"),
            ("oo", "哦噢"), ("ee", "额俄恶饿鹅"), ("en", "嗯恩"), ("ai", "爱哎唉埃碍"),
            ("hv", "会回汇灰挥毁绘惠辉"), ("ng", "能"), ("ke", "可课科客刻颗"),
            ("yi", "一以已意义议易医依艺亿亦衣移疑"), ("er", "二而儿耳尔"), ("sj", "三散伞"),
            ("si", "四死思司斯丝私"), ("wu", "五无物务屋武误舞午"), ("lq", "六流留刘柳"),
            ("qi", "七起其气期器企奇齐旗"), ("jq", "九就久酒究救旧"), ("yc", "要药摇腰咬邀耀"),
            ("xl", "想向像相项香响箱乡"), ("jn", "叫教交较角脚焦"), ("zo", "做作坐左座昨"),
            ("ys", "用永拥勇涌泳"), ("dv", "对队堆兑"), ("kd", "开凯慨"), ("gr", "关管馆观官惯冠"),
            ("bi", "比必笔闭币避壁"), ("xb", "新心信辛欣薪"), ("nm", "年念粘"), ("yt", "月越约阅岳悦跃"),
            ("ri", "日"), ("tm", "天田添甜填"), ("jb", "今进金近尽紧禁津劲"), ("mk", "明名命鸣铭"),
            ("hz", "后候厚猴喉"), ("qm", "前钱千签潜浅牵"), ("xm", "现先线显限县献险鲜"),
            ("dm", "点电店典垫淀"), ("ff", "分份粉奋纷愤"), ("mn", "秒苗妙庙描"),
            ("kk", "快块筷"), ("mj", "慢满漫曼"), ("gc", "高告搞稿糕"), ("di", "地第低底弟敌抵递滴"),
            ("da", "大打达答搭"), ("xn", "小笑校消效销晓"), ("do", "多夺朵躲"), ("uc", "少烧稍绍哨"),
            ("ih", "长常场唱厂昌尝偿"), ("vh", "张章长掌丈涨障帐"), ("dr", "段断短端"),
            ("vf", "真镇阵针震珍振"), ("jx", "家加价假架佳驾夹"), ("hf", "很恨狠痕"),
            ("td", "太台态抬泰"), ("zv", "最罪嘴醉"), ("gg", "更耕梗"), ("hd", "还海害孩"),
            ("ye", "也业夜叶页野液爷"), ("dz", "都斗豆抖兜"), ("vi", "只之知制直指至治职质支值致置纸止织"),
            ("dj", "但单担蛋淡胆旦弹"), ("qt", "却确缺雀"), ("cs", "从丛匆聪"), ("dc", "到道导倒刀岛盗"),
            ("ww", "为位未围维微味卫委威"), ("bw", "被北备背倍杯贝悲碑"), ("gw", "给"),
            ("rh", "让壤嚷"), ("he", "和合河何喝核荷贺鹤"), ("yu", "与于语鱼育遇玉雨羽域预余"),
            ("ho", "或活火获货伙惑"), ("ro", "若弱"), ("ru", "如入乳辱儒"), ("so", "所索锁缩"),
            ("yb", "因音引银印阴隐饮"), ("sv", "虽随岁碎"), ("rj", "然染燃"), ("ze", "则责择泽"),
            ("ji", "即机及级计记己几基济寄技既继急集极际季激迹"), ("xk", "行性形型兴星姓刑醒幸"),
            ("hh", "航行杭巷"), ("is", "冲充重虫崇宠"), ("go", "国过果锅郭裹"), ("uu", "数书术树输属熟束"),
            ("fa", "发法罚乏伐"), ("rr", "软阮"), ("jm", "见间建件剑检简渐坚肩减"),
            ("xu", "需许须续序虚徐"), ("PB", "品频贫拼聘"), ("pb", "品频贫拼聘"), ("ul", "双霜爽"),
            ("qr", "全权圈泉劝拳券"), ("uf", "什身神深甚审伸"), ("me", "么"), ("zf", "怎"),
            ("my", "没美每妹梅媒"), ("su", "速素苏诉俗宿塑"), ("vr", "转专砖传"), ("ir", "传船穿川串"),
            ("ug", "生省声胜升盛圣剩绳"), ("vg", "正政争整证征症"), ("ig", "成城称程承呈乘诚"),
            ("jk", "经京精境竟惊景静警镜"), ("qk", "情清请轻青庆倾"), ("lk", "领令另灵零龄凌"),
            ("tk", "听停庭厅挺亭"), ("dk", "定顶丁订钉盯"), ("bk", "并兵病冰饼"), ("pk", "平评凭瓶屏萍"),
            ("Gs", "公工共功宫供攻贡"), ("gs", "公工共功宫供攻贡"), ("ds", "动东洞懂冬董冻"),
            ("ts", "同通统痛铜童筒桶"), ("ks", "空控孔恐"), ("hs", "红洪鸿宏轰哄虹"),
            ("ls", "龙隆笼拢"), ("ns", "农弄浓"), ("zs", "总宗纵综踪"), ("ss", "送宋松颂诵"),
            ("jp", "解接结界节街阶届介借"), ("xp", "些写协谢械斜鞋"), ("qp", "切且窃怯"),
            ("lp", "列烈裂猎劣"), ("bp", "别憋"), ("dp", "跌叠碟蝶"), ("tp", "铁贴帖"), ("mp", "灭"),
            ("np", "捏聂"), ("pp", "撇瞥"), ("jr", "卷捐倦眷"), ("xr", "选宣旋悬玄"),
            ("jt", "觉决绝掘"), ("xt", "学血雪穴削"), ("lt", "略掠"), ("nt", "虐"),
            ("lv", "绿率律旅虑吕驴铝"), ("nv", "女"), ("ju", "据举具居局句剧巨距聚"),
            ("qu", "去区取曲趋趣屈驱渠"), ("jy", "军均君菌俊"), ("qy", "群裙"), ("xy", "训寻讯迅询巡循"),
            ("ly", "论轮伦沦"), ("cy", "村存寸"), ("zy", "尊遵"), ("sy", "孙损笋"), ("ty", "吞屯"),
            ("dy", "顿吨盾敦"), ("ky", "困昆坤捆"), ("gy", "滚棍"), ("hy", "婚混魂昏浑"),
            ("uy", "顺瞬"), ("vy", "准"), ("iy", "春纯醇唇"), ("ry", "润闰"), ("wy", "温文问闻"),
            ("wj", "万完晚湾玩碗弯"), ("wh", "王往望网忘汪旺"), ("wd", "外歪"), ("wa", "瓦挖娃哇蛙袜"),
            ("wf", "文问闻温稳纹吻蚊"), ("wg", "翁瓮嗡"),
        ];
        for &(sp2, chars_str) in CORE_CHARS {
            for (rank, ch) in chars_str.chars().enumerate() {
                let mut buf = [0u8; 4];
                let ch_s = ch.encode_utf8(&mut buf);
                let score = 110_000u32.saturating_sub((rank as u32) * 1800);
                self.insert_sp_char(sp2, ch_s, score);
            }
        }

        // 2. Index all single characters from `sp2_hints` and `flypy_map`
        let hints_snapshot: Vec<(String, Vec<String>)> = self
            .sp2_hints
            .iter()
            .map(|(sp2, list)| (sp2.clone(), list.iter().map(|h| h.ch.clone()).collect()))
            .collect();
        for (sp2, chars) in hints_snapshot {
            for (idx, ch_s) in chars.iter().enumerate() {
                let score = 55_000u32.saturating_sub((idx as u32) * 400);
                self.insert_sp_char(&sp2, ch_s, score);
            }
        }

        // 3. Curated High-Frequency Conversational Words, Phrases & Sentences (120+ CPM Core Vocabulary)
        const CORE_WORDS: &[(&str, &str, u32)] = &[
            ("womf", "我们", 145_000), ("nimf", "你们", 130_000), ("tamf", "他们", 132_000),
            ("tamf", "她们", 122_000), ("tamf", "它们", 118_000), ("ziji", "自己", 138_000),
            ("dajx", "大家", 132_000), ("zjmf", "咱们", 120_000), ("vege", "这个", 142_000),
            ("nage", "那个", 135_000), ("vexp", "这些", 130_000), ("naxp", "那些", 126_000),
            ("veli", "这里", 132_000), ("nali", "那里", 125_000), ("veyh", "这样", 138_000),
            ("nayh", "那样", 122_000), ("veme", "这么", 132_000), ("name", "那么", 130_000),
            ("ufme", "什么", 148_000), ("zfme", "怎么", 142_000), ("zfyh", "怎样", 125_000),
            ("wwufme", "为什么", 142_000), ("douc", "多少", 130_000), ("nali", "哪里", 130_000),
            ("ybww", "因为", 145_000), ("soyi", "所以", 145_000), ("djui", "但是", 142_000),
            ("rugo", "如果", 140_000), ("svrj", "虽然", 135_000), ("rjer", "然而", 128_000),
            ("erqp", "而且", 138_000), ("hove", "或者", 135_000), ("hdui", "还是", 140_000),
            ("yijk", "已经", 142_000), ("vgzd", "正在", 136_000), ("mauh", "马上", 136_000),
            ("like", "立刻", 128_000), ("ghgh", "刚刚", 130_000), ("yivi", "一直", 136_000),
            ("yidk", "一定", 136_000), ("kfdk", "肯定", 132_000), ("yexu", "也许", 126_000),
            ("keng", "可能", 140_000), ("dagd", "大概", 126_000), ("iabudo", "差不多", 135_000),
            ("jihu", "几乎", 126_000), ("wjqr", "完全", 140_000), ("fwih", "非常", 142_000),
            ("tebp", "特别", 136_000), ("yzqi", "尤其", 128_000), ("vfde", "真的", 138_000),
            ("qtui", "确实", 135_000), ("qiui", "其实", 138_000), ("uouihx", "说实话", 135_000),
            ("zsvi", "总之", 126_000), ("hrjuhxuo", "换句话说", 132_000), ("yejquiuo", "也就是说", 136_000),
            ("keyi", "可以", 148_000), ("xuyc", "需要", 145_000), ("ykgd", "应该", 140_000),
            ("bixu", "必须", 138_000), ("nggz", "能够", 136_000), ("xlyc", "想要", 134_000),
            ("xiwh", "希望", 138_000), ("jtde", "觉得", 142_000), ("rfww", "认为", 135_000),
            ("vidc", "知道", 146_000), ("mkbd", "明白", 132_000), ("lijp", "理解", 134_000),
            ("faxm", "发现", 138_000), ("kjdc", "看到", 136_000), ("tkdc", "听到", 132_000),
            ("yudc", "遇到", 132_000), ("zodc", "做到", 132_000), ("vcdc", "找到", 132_000),
            ("uzdc", "收到", 132_000), ("dadc", "达到", 134_000), ("wjig", "完成", 136_000),
            ("kdui", "开始", 140_000), ("jpuu", "结束", 134_000), ("jixu", "继续", 136_000),
            ("tkvi", "停止", 128_000), ("vybw", "准备", 134_000), ("dasr", "打算", 128_000),
            ("jtdk", "决定", 136_000), ("xrze", "选择", 140_000), ("ihui", "尝试", 132_000),
            ("bhvu", "帮助", 136_000), ("viii", "支持", 138_000), ("jpjt", "解决", 138_000),
            ("iuli", "处理", 136_000), ("yzhx", "优化", 136_000), ("ugji", "升级", 135_000),
            ("ggxb", "更新", 136_000), ("anjvl", "安装", 132_000), ("anjv", "安装", 136_000),
            ("isqi", "重启", 142_000), ("vsqi", "重启", 140_000), ("isxb", "重新", 138_000),
            ("vsxb", "重新", 136_000), ("vsyc", "重要", 140_000), ("isfu", "重复", 132_000),
            ("xxzd", "下载", 136_000), ("uhir", "上传", 130_000), ("bccy", "保存", 136_000),
            ("ujiu", "删除", 134_000), ("quxn", "取消", 134_000), ("qtrf", "确认", 136_000),
            ("fuvi", "复制", 132_000), ("vjtp", "粘贴", 132_000), ("soso", "搜索", 134_000),
            ("iakj", "查看", 136_000), ("uevi", "设置", 142_000), ("kdqi", "开启", 134_000),
            ("grbi", "关闭", 134_000), ("qphr", "切换", 136_000), ("jiho", "激活", 132_000),
            ("uibp", "识别", 136_000), ("vrlu", "转录", 136_000), ("fjyi", "翻译", 136_000),
            ("jxmi", "加密", 135_000), ("bchu", "保护", 135_000), ("jbtm", "今天", 146_000),
            ("mktm", "明天", 142_000), ("zotm", "昨天", 138_000), ("hztm", "后天", 130_000),
            ("qmtm", "前天", 128_000), ("xmzd", "现在", 148_000), ("ghcd", "刚才", 132_000),
            ("yihz", "以后", 136_000), ("yiqm", "以前", 135_000), ("viqm", "之前", 136_000),
            ("vihz", "之后", 136_000), ("uhwu", "上午", 138_000), ("xxwu", "下午", 140_000),
            ("wjuh", "晚上", 140_000), ("vswu", "中午", 134_000), ("zcuh", "早上", 135_000),
            ("uihf", "时候", 142_000), ("uijm", "时间", 144_000), ("xkqi", "星期", 135_000),
            ("vzmo", "周末", 135_000), ("libd", "礼拜", 128_000), ("jbnm", "今年", 138_000),
            ("mknm", "明年", 134_000), ("qunm", "去年", 134_000), ("mwtm", "每天", 136_000),
            ("mwci", "每次", 132_000), ("wfti", "问题", 146_000), ("bjfa", "办法", 136_000),
            ("fhfa", "方法", 138_000), ("fhan", "方案", 142_000), ("fjgj", "反感", 132_000),
            ("ihan", "长安", 134_000), ("xian", "西安", 138_000), ("qbai", "亲爱", 136_000),
            ("wjan", "晚安", 136_000), ("tmanmf", "天安门", 138_000), ("jpgo", "结果", 138_000),
            ("yryb", "原因", 136_000), ("qkkl", "情况", 140_000), ("difh", "地方", 138_000),
            ("dsxi", "东西", 140_000), ("uiqk", "事情", 140_000), ("gszo", "工作", 144_000),
            ("xtxi", "学习", 140_000), ("ugho", "生活", 140_000), ("pgyz", "朋友", 140_000),
            ("jxrf", "家人", 134_000), ("lcui", "老师", 136_000), ("tsxt", "同学", 134_000),
            ("tsui", "同事", 134_000), ("gssi", "公司", 138_000), ("uzji", "手机", 140_000),
            ("dmnc", "电脑", 142_000), ("xits", "系统", 142_000), ("rrjm", "软件", 145_000),
            ("ykjm", "硬件", 132_000), ("whlo", "网络", 138_000), ("uipb", "视频", 140_000),
            ("ybpb", "音频", 138_000), ("ugyb", "声音", 140_000), ("mdkefg", "麦克风", 140_000),
            ("jmpj", "键盘", 136_000), ("uubn", "鼠标", 134_000), ("pkmu", "屏幕", 134_000),
            ("ilz", "窗口", 132_000), ("ilkz", "窗口", 136_000), ("wfjm", "文件", 138_000),
            ("wfjmjx", "文件夹", 136_000), ("mima", "密码", 140_000), ("vhhc", "账号", 134_000),
            ("yshumk", "用户名", 138_000), ("tzxl", "头像", 136_000), ("ciku", "词库", 140_000),
            ("liui", "历史", 136_000), ("uuju", "数据", 140_000), ("ybsi", "隐私", 138_000),
            ("anqr", "安全", 142_000), ("sudu", "速度", 140_000), ("xnlu", "效率", 138_000),
            ("xnlv", "效率", 142_000), ("vyqt", "准确", 140_000), ("vyqtlv", "准确率", 138_000),
            ("uurufa", "输入法", 148_000), ("qrpb", "全拼", 144_000), ("ulpb", "双拼", 144_000),
            ("ybxk", "音形", 144_000), ("heyu", "鹤羽", 146_000), ("heyuybxk", "鹤羽音形", 148_000),
            ("rfgsvinf", "人工智能", 148_000), ("damoxk", "大模型", 140_000), ("xmka", "显卡", 136_000),
            ("bijibf", "笔记本", 138_000), ("mwyz", "没有", 148_000), ("mwwfti", "没问题", 142_000),
            ("bukeqi", "不客气", 136_000), ("mwgrxi", "没关系", 136_000), ("buhcyisi", "不好意思", 140_000),
            ("fwihgjxp", "非常感谢", 140_000), ("ytldyt", "越来越", 138_000), ("yzuihz", "有时候", 138_000),
            ("uijiuh", "实际上", 138_000), ("zsdelduo", "总的来说", 136_000), ("visoyi", "之所以", 135_000),
            ("wulyruhe", "无论如何", 135_000), ("uiuiqqui", "实事求是", 136_000),
            ("jbtmxxwu", "今天下午", 144_000), ("jbtmwjuh", "今天晚上", 142_000),
            ("jbtmkdhv", "今天开会", 144_000), ("kdhv", "开会", 140_000), ("mktmuhwu", "明天上午", 140_000),
            ("isqirrmjm", "重启软件", 145_000), ("isqirrjm", "重启软件", 146_000),
            ("vsqirrjm", "重启软件", 145_000), ("isxbrrjm", "重新软件", 135_000),
            ("wjqrmwyzwfti", "完全没有问题", 145_000), ("vsgo", "中国", 146_000), ("uijp", "世界", 142_000),
            ("wxxb", "微信", 140_000), ("wwxb", "微信", 142_000), ("nihc", "你好", 145_000),
            ("nihcma", "你好吗", 140_000), ("xpxp", "谢谢", 144_000), ("zdjm", "再见", 140_000),
            ("yhhd", "银行", 138_000), ("ybhh", "银行", 140_000), ("hhye", "行业", 140_000),
            ("xkds", "行动", 138_000), ("vhda", "长大", 136_000), ("ihdr", "长短", 134_000),
            ("lvse", "绿色", 136_000), ("luse", "绿色", 134_000), ("nvug", "女生", 136_000),
            ("nvrf", "女人", 135_000), ("njrf", "男人", 135_000), ("njug", "男生", 136_000),
            // Daily conversational A-bu-A, A-bu-AB, degree adverbs, resultatives & evaluations
            ("hcys", "好用", 146_000), ("buhcys", "不好用", 145_000), ("hcbuhcys", "好不好用", 149_000),
            ("hfhcys", "很好用", 145_000), ("tdhcys", "太好用", 144_000), ("tdhcysle", "太好用了", 146_000),
            ("vfhcys", "真好用", 143_000), ("zvhcys", "最好用", 143_000), ("gghcys", "更好用", 142_000),
            ("tkhcys", "挺好用", 142_000), ("icjihcys", "超级好用", 144_000),
            ("njys", "难用", 142_000), ("tdnjys", "太难用", 144_000), ("tdnjysle", "太难用了", 146_000),
            ("hfnjys", "很难用", 142_000), ("fjrfxk", "反人性", 144_000), ("rfxkhx", "人性化", 142_000),
            ("hcbuhc", "好不好", 148_000), ("hckj", "好看", 144_000), ("buhckj", "不好看", 142_000),
            ("hcbuhckj", "好不好看", 146_000), ("hcii", "好吃", 144_000), ("buhcii", "不好吃", 142_000),
            ("hcbuhcii", "好不好吃", 146_000), ("hcwj", "好玩", 142_000), ("buhcwj", "不好玩", 140_000),
            ("hcbuhcwj", "好不好玩", 145_000), ("hctk", "好听", 142_000), ("buhctk", "不好听", 140_000),
            ("hcbuhctk", "好不好听", 145_000), ("hcxn", "好笑", 138_000), ("buhcxn", "不好笑", 138_000),
            ("hciu", "好处", 140_000), ("hcqi", "好奇", 140_000), ("hcrg", "好人", 138_000),
            ("hcui", "好事", 138_000), ("hcyy", "好运", 138_000), ("hcjq", "好久", 140_000),
            ("hcdo", "好多", 142_000), ("hcxl", "好像", 144_000), ("hczd", "好在", 138_000),
            ("buhc", "不好", 144_000), ("uibuui", "是不是", 148_000), ("dvbudv", "对不对", 146_000),
            ("xkbuxk", "行不行", 145_000), ("ngbung", "能不能", 146_000), ("hvbuhv", "会不会", 145_000),
            ("ycbuyc", "要不要", 145_000), ("kebukeyi", "可不可以", 147_000), ("yzmwyz", "有没有", 148_000),
            ("fhbufhbm", "方不方便", 146_000), ("fhbm", "方便", 144_000), ("bufhbm", "不方便", 142_000),
            ("xibuxihr", "喜不喜欢", 145_000), ("xihr", "喜欢", 145_000), ("buxihr", "不喜欢", 142_000),
            ("mkbumkbd", "明不明白", 144_000), ("qkiu", "清楚", 142_000), ("buqkiu", "不清楚", 140_000),
            ("qkbuqkiu", "清不清楚", 144_000), ("vibuvidc", "知不知道", 146_000), ("buvidc", "不知道", 148_000),
            ("zfmezf", "怎么样", 146_000), ("zfmeyh", "怎么样", 148_000), ("zfmebj", "怎么办", 146_000),
            ("zfmehv", "怎么会", 142_000), ("zfmele", "怎么了", 144_000), ("zfmegc", "怎么搞", 138_000),
            ("zfmens", "怎么弄", 138_000), ("yzufme", "有什么", 144_000), ("mwufme", "没什么", 144_000),
            ("yzyisi", "有意思", 142_000), ("mwyisi", "没意思", 140_000), ("veyhzi", "这样子", 140_000),
            ("nayhzi", "那样子", 136_000), ("lddeji", "来得及", 140_000), ("ldbuji", "来不及", 142_000),
            ("uzbuln", "受不了", 142_000), ("gkbudk", "搞不定", 140_000), ("gcbudk", "搞不定", 142_000),
            ("gcdedk", "搞得定", 138_000), ("uobudk", "说不定", 142_000), ("gkbdde", "怪不得", 140_000),
            ("dvbuqi", "对不起", 144_000), ("kjbuqi", "看不起", 138_000), ("lnbuqi", "了不起", 140_000),
            ("kjbuds", "看不懂", 142_000), ("kjbuqk", "看不清", 142_000), ("kjbujm", "看不见", 142_000),
            ("tkbuds", "听不懂", 142_000), ("tkbuqk", "听不清", 140_000), ("tkbujm", "听不见", 140_000),
            ("vcbudc", "找不到", 142_000), ("xlbudc", "想不到", 140_000), ("kjbudc", "看不到", 142_000),
            ("zobudc", "做不到", 140_000), ("mdbudc", "买不到", 138_000), ("dabukd", "打不开", 142_000),
            ("libukd", "离不开", 138_000), ("jibuvu", "记不住", 140_000), ("rfbuvu", "忍不住", 142_000),
            ("xlbuts", "想不通", 138_000), ("uobuqk", "说不清", 138_000), ("kjyikj", "看一看", 142_000),
            ("uiyiui", "试一试", 142_000), ("xlyixl", "想一想", 140_000), ("dgyidg", "等一等", 140_000),
            ("hzxrci", "候选词", 144_000), ("uuln", "数量", 140_000), ("xigc", "习惯", 138_000),
            ("xigr", "习惯", 142_000),
        ];
        for &(sp_seq, word, score) in CORE_WORDS {
            self.insert_sp_word(sp_seq, word, score);
        }

        // 4. Index User Custom Top Words (`user_custom.tsv`)
        self.index_user_custom_into_lattice();

        // 5. Index Turbo Phrases (`zh_turbo_phrases.tsv`)
        let turbo_snap = self.turbo_phrases.clone();
        for (code4, phrase) in turbo_snap {
            if let Some(sp_seq) = self.word_to_sp_seq(&phrase, Some(&code4)) {
                self.insert_sp_word(&sp_seq, &phrase, 102_000);
            }
        }

        // 6. Index all multi-character words from `flypy_map` (`heyu_sys.tsv` / `heyu_open_sys.tsv`)
        const BOUND_LITERARY_2CHAR: &[&str] = &[
            "毫不", "微不", "殊不", "莫不", "概不", "断不", "誓不", "义不", "势不", "大不",
            "绝不", "决不", "靡不", "无不", "莫非", "何不", "岂不", "胡不",
        ];
        let mut sys_chars: Vec<(String, String, u32)> = Vec::new();
        let mut sys_words: Vec<(String, String, u32)> = Vec::new();
        for (code, list) in &self.flypy_map {
            if !code.chars().all(|c| c.is_ascii_lowercase()) {
                continue;
            }
            for (rank, cand) in list.iter().enumerate() {
                let ccount = cand.text.chars().count();
                let is_secondary = cand.layer == "secondary";
                if ccount == 1 {
                    if code.len() >= 2 {
                        let sp2 = code[..2].to_string();
                        let sc = if code.len() == 2 {
                            75_000u32.saturating_sub((rank as u32) * 2000)
                        } else if is_secondary {
                            22_000u32.saturating_sub((rank as u32) * 400)
                        } else {
                            35_000u32.saturating_sub((rank as u32) * 500)
                        };
                        sys_chars.push((sp2, cand.text.clone(), sc));
                    }
                } else if ccount >= 2 {
                    let base = if BOUND_LITERARY_2CHAR.contains(&cand.text.as_str()) {
                        42_000u32.saturating_sub((rank as u32) * 2000)
                    } else if is_secondary {
                        50_000u32.saturating_sub((rank as u32) * 3000)
                    } else {
                        84_000u32.saturating_sub((rank as u32) * 6500)
                    };
                    sys_words.push((code.clone(), cand.text.clone(), base));
                }
            }
        }
        for (sp2, ch_text, sc) in sys_chars {
            self.insert_sp_char(&sp2, &ch_text, sc);
        }
        for (code, word, score) in sys_words {
            if let Some(sp_seq) = self.word_to_sp_seq(&word, Some(&code)) {
                self.insert_sp_word(&sp_seq, &word, score);
            }
        }

        // 7. Sort all buckets descending by score so lookups are O(1) top-K
        for list in self.sp_char_map.values_mut() {
            list.sort_by(|a, b| b.1.cmp(&a.1));
        }
        for list in self.sp_word_map.values_mut() {
            list.sort_by(|a, b| b.1.cmp(&a.1));
        }
        for list in self.init_word_map.values_mut() {
            list.sort_by(|a, b| b.1.cmp(&a.1));
            list.truncate(12);
        }
    }

    /// Build the Byte-Level Syllable DAG for `self.input_buf` in `quanpin` or `shuangpin` mode.
    fn build_syllable_dag(&self, raw: &str, is_quanpin: bool) -> Vec<Vec<SylEdge>> {
        let m = raw.len();
        let bytes = raw.as_bytes();
        let mut adj: Vec<Vec<SylEdge>> = vec![Vec::new(); m + 1];

        if !is_quanpin {
            // Shuangpin mode: each pair of letters is an `sp2` syllable, AND single letters can act as initials
            for i in 0..m {
                if bytes[i] == b'\'' {
                    adj[i].push(SylEdge {
                        end: i + 1,
                        raw: String::new(),
                        sp2_cands: Vec::new(),
                        init_char: None,
                        is_full: true,
                    });
                    continue;
                }
                if i + 2 <= m && bytes[i].is_ascii_lowercase() && bytes[i + 1].is_ascii_lowercase() {
                    let sub = &raw[i..i + 2];
                    if self.sp_char_map.contains_key(sub) {
                        let mut cands = vec![(sub.to_string(), 1.0)];
                        for fz in fuzzy_sp2_variants(sub) {
                            if self.sp_char_map.contains_key(&fz) {
                                cands.push((fz, 0.80));
                            }
                        }
                        adj[i].push(SylEdge {
                            end: i + 2,
                            raw: sub.to_string(),
                            sp2_cands: cands,
                            init_char: Some(bytes[i] as char),
                            is_full: true,
                        });
                    }
                }
                // Single-letter initial edge (for initial abbreviations `wm`, `jtkh`, `srf` or odd final letter)
                let c = bytes[i] as char;
                if c.is_ascii_lowercase() {
                    adj[i].push(SylEdge {
                        end: i + 1,
                        raw: c.to_string(),
                        sp2_cands: Vec::new(),
                        init_char: Some(c),
                        is_full: false,
                    });
                }
            }
            return adj;
        }

        // Quanpin mode: Byte-Level Syllable DAG supporting:
        // 1. Exact Mandarin Pinyin syllables (1..=6 letters)
        // 2. Typo-corrected syllables (`ign->ing`, `unag->uang`, `on->ong`, `gou->guo`, etc.)
        // 3. Fuzzy Pinyin variants (`z/zh`, `c/ch`, `s/sh`, `n/l`, `in/ing`, `en/eng`, `an/ang`, `lu/lv`)
        // 4. Incomplete mid-typing prefix syllables at the end of `raw` (`zho`, `zhon`, `xio`)
        // 5. Initial abbreviations (`zh`, `ch`, `sh`, `b..z`) for pure & hybrid abbreviated input (`wm`, `womenjt`, `rgzn`, `srf`)
        for i in 0..m {
            if bytes[i] == b'\'' {
                adj[i].push(SylEdge {
                    end: i + 1,
                    raw: String::new(),
                    sp2_cands: Vec::new(),
                    init_char: None,
                    is_full: true,
                });
                continue;
            }
            let max_len = (m - i).min(6);
            let mut has_full_starting_here = false;
            for len in (1..=max_len).rev() {
                let sub = &raw[i..i + len];
                if sub.contains('\'') {
                    continue;
                }
                if let Some(sp2) = pinyin_syllable_to_sp2(sub) {
                    if self.sp_char_map.contains_key(&sp2) {
                        has_full_starting_here = true;
                        let mut cands = vec![(sp2.clone(), 1.0)];
                        for fz in fuzzy_sp2_variants(&sp2) {
                            if self.sp_char_map.contains_key(&fz) {
                                cands.push((fz, 0.82));
                            }
                        }
                        let ic = sp2.as_bytes()[0] as char;
                        adj[i].push(SylEdge {
                            end: i + len,
                            raw: sub.to_string(),
                            sp2_cands: cands,
                            init_char: Some(ic),
                            is_full: true,
                        });
                        continue;
                    }
                }
                if len >= 2 {
                    if let Some(sp2) = typo_pinyin_to_sp2(sub) {
                        if self.sp_char_map.contains_key(&sp2) {
                            let ic = sp2.as_bytes()[0] as char;
                            adj[i].push(SylEdge {
                                end: i + len,
                                raw: sub.to_string(),
                                sp2_cands: vec![(sp2, 0.90)],
                                init_char: Some(ic),
                                is_full: true,
                            });
                            continue;
                        }
                    }
                }
                // Incomplete syllable prefix at the end of the buffer (e.g. `zho`, `zhon`, `xio`, `xion`)
                if i + len == m && len >= 2 {
                    let pfx_sp2 = prefix_pinyin_to_sp2_list(sub);
                    let valid_pfx: Vec<(String, f64)> = pfx_sp2
                        .into_iter()
                        .filter(|s| self.sp_char_map.contains_key(s))
                        .map(|s| (s, 0.86))
                        .collect();
                    if !valid_pfx.is_empty() {
                        let ic = valid_pfx[0].0.as_bytes()[0] as char;
                        adj[i].push(SylEdge {
                            end: m,
                            raw: sub.to_string(),
                            sp2_cands: valid_pfx,
                            init_char: Some(ic),
                            is_full: true,
                        });
                    }
                }
            }

            // Initial-only abbreviation edges (`zh -> v`, `ch -> i`, `sh -> u`, or single consonant initial `b..z`)
            // Only emit an abbreviated initial edge when it is NOT immediately followed by a vowel (or when no full syllable starts here)
            if i + 2 <= m {
                let dig = &raw[i..i + 2];
                let dig_init = match dig {
                    "zh" => Some('v'),
                    "ch" => Some('i'),
                    "sh" => Some('u'),
                    _ => None,
                };
                let after_dig_is_vowel = i + 2 < m && matches!(bytes[i + 2], b'a' | b'e' | b'i' | b'o' | b'u' | b'v');
                if let Some(ic) = dig_init {
                    if !after_dig_is_vowel || !has_full_starting_here {
                        adj[i].push(SylEdge {
                            end: i + 2,
                            raw: dig.to_string(),
                            sp2_cands: Vec::new(),
                            init_char: Some(ic),
                            is_full: false,
                        });
                    }
                }
            }
            let c = bytes[i] as char;
            // Only consonants (`b..z` excluding `a, e, i, o, u, v`) can act as abbreviated initials in Quanpin,
            // and only when not followed by a vowel that already forms a full syllable starting here!
            let is_consonant_init = c.is_ascii_lowercase() && !matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'v');
            let next_is_vowel = i + 1 < m && matches!(bytes[i + 1], b'a' | b'e' | b'i' | b'o' | b'u' | b'v');
            if is_consonant_init && (!next_is_vowel || !has_full_starting_here) {
                adj[i].push(SylEdge {
                    end: i + 1,
                    raw: c.to_string(),
                    sp2_cands: Vec::new(),
                    init_char: Some(c),
                    is_full: false,
                });
            }
        }
        adj
    }

    fn word_bonus(&self, word: &str, at_start: bool) -> f64 {
        let mut b = 0.0;
        if let Some(&ub) = self.user_freq_boost.get(word) {
            b += (ub as f64) / 800.0;
        }
        if at_start && !self.last_committed.is_empty() {
            let prev = self.last_committed.trim();
            if let Some(u_list) = self.user_bigrams.get(prev) {
                if u_list.iter().any(|w| w == word) {
                    b += 140.0;
                }
            }
            if let Some(c_list) = self.context_ngrams.get(prev) {
                if c_list.iter().any(|w| w == word) {
                    b += 95.0;
                }
            }
        }
        b
    }

    fn top_char_for_initial(&self, init_c: char) -> Option<(String, u32)> {
        let primary_sp2 = match init_c {
            'w' => "wo",
            'n' => "ni",
            't' => "ta",
            'd' => "de",
            'u' | 's' => "ui",
            'b' => "bu",
            'l' => "le",
            'z' => "zd",
            'y' => "yi",
            'v' => "ve",
            'g' => "ge",
            'r' => "rf",
            'k' => "ke",
            'h' => "he",
            'j' => "jq",
            'q' => "qu",
            'x' => "xm",
            'm' => "ma",
            'f' => "fa",
            'p' => "pk",
            'c' | 'i' => "iu",
            'a' => "aa",
            'o' => "oo",
            'e' => "ee",
            _ => return None,
        };
        self.sp_char_map
            .get(primary_sp2)
            .and_then(|list| list.first().cloned())
    }

    /// Populate `self.candidates`, `self.candidate_consumed`, `self.preedit_segmented`, and `self.ghost_tab_text`
    /// using the Byte-Level Syllable Lattice + Viterbi Whole-Sentence Decoder.
    pub fn refresh_pinyin_lattice_candidates(&mut self) {
        let raw = self.input_buf.clone();
        let m = raw.len();
        if m == 0 {
            return;
        }
        let scheme = self.config.input_scheme.as_str();
        let is_quanpin = scheme == "quanpin";

        // 1. Check User Custom Top Words (`user_custom.tsv`) first
        let mut user_lookup_codes = vec![raw.clone()];
        if is_quanpin {
            for c in quanpin_to_shuangpin_codes(&raw) {
                if !user_lookup_codes.contains(&c) {
                    user_lookup_codes.push(c);
                }
            }
        }
        for code in &user_lookup_codes {
            if let Some(u_cands) = self.user_map.get(code) {
                for c in u_cands {
                    let expanded = Self::expand_user_template(&c.text);
                    if !self.candidates.iter().any(|ex| ex.text == expanded) {
                        let mut cand = c.clone();
                        cand.text = expanded;
                        self.candidates.push(cand);
                    }
                }
            }
        }

        // 2. Build the Byte-Level Syllable DAG over `raw[0..m]`
        let adj = self.build_syllable_dag(&raw, is_quanpin);

        // Enumerate multi-syllable paths starting from every byte position `b0`
        // `(b_start, b_end, word, span_score, seg_display)`
        #[derive(Clone, Debug)]
        struct WordSpan {
            end: usize,
            word: String,
            score: f64,
            seg: String,
        }
        let mut spans_from: Vec<Vec<WordSpan>> = vec![Vec::new(); m + 1];

        for b0 in 0..m {
            if b0 > 0 && adj.iter().take(b0).all(|edges| !edges.iter().any(|e| e.end == b0)) {
                // Still allow from b0 if reachable
            }
            // DFS up to 6 syllables starting at `b0`
            fn walk_syllables<'a>(
                eng: &'a HeyuEngine,
                adj: &'a [Vec<SylEdge>],
                b0: usize,
                cur_b: usize,
                path: &mut Vec<&'a SylEdge>,
                spans: &mut Vec<WordSpan>,
            ) {
                if !path.is_empty() {
                    let syl_edges: Vec<&&SylEdge> = path.iter().filter(|e| !e.raw.is_empty()).collect();
                    let k = syl_edges.len();
                    if k >= 1 {
                        let seg_str: String = syl_edges.iter().map(|e| e.raw.as_str()).collect::<Vec<_>>().join("'");
                        let all_full = syl_edges.iter().all(|e| e.is_full && !e.sp2_cands.is_empty());
                        let full_count = syl_edges.iter().filter(|e| e.is_full).count();
                        let bytes_covered = (cur_b - b0) as f64;
                        let at_start = b0 == 0;

                        if k == 1 {
                            let e0 = syl_edges[0];
                            if e0.is_full {
                                for (idx_sp, (sp2, wgt)) in e0.sp2_cands.iter().take(3).enumerate() {
                                    if let Some(ch_list) = eng.sp_char_map.get(sp2) {
                                        if let Some((ch_str, base_sc)) = ch_list.first() {
                                            let freq_bonus = if *base_sc >= 100_000 {
                                                (*base_sc as f64) / 720.0
                                            } else {
                                                (*base_sc as f64) / 1000.0 - 30.0
                                            };
                                            let sc = bytes_covered * 248.0 * wgt
                                                + freq_bonus
                                                - (idx_sp as f64) * 18.0
                                                + eng.word_bonus(ch_str, at_start);
                                            spans.push(WordSpan {
                                                end: cur_b,
                                                word: ch_str.clone(),
                                                score: sc,
                                                seg: seg_str.clone(),
                                            });
                                        }
                                    }
                                }
                            } else if let Some(ic) = e0.init_char {
                                if let Some((ch_str, base_sc)) = eng.top_char_for_initial(ic) {
                                    let sc = bytes_covered * 60.0 + (base_sc as f64) / 2500.0 + eng.word_bonus(&ch_str, at_start);
                                    spans.push(WordSpan {
                                        end: cur_b,
                                        word: ch_str,
                                        score: sc,
                                        seg: seg_str.clone(),
                                    });
                                }
                            }
                        } else if k <= 6 {
                            // Multi-syllable span:
                            // (a) Full/Fuzzy syllable sequence lookup in `sp_word_map`
                            if all_full {
                                // Generate top combinations of `sp2` across the `k` syllables
                                let max_alts = if k == 2 { 3 } else if k == 3 { 2 } else { 1 };
                                fn build_sp_seqs(
                                    edges: &[&&SylEdge],
                                    idx: usize,
                                    max_alts: usize,
                                    cur_seq: &mut String,
                                    cur_wgt: f64,
                                    out: &mut Vec<(String, f64)>,
                                ) {
                                    if out.len() >= 12 {
                                        return;
                                    }
                                    if idx == edges.len() {
                                        out.push((cur_seq.clone(), cur_wgt));
                                        return;
                                    }
                                    for (sp2, w) in edges[idx].sp2_cands.iter().take(max_alts) {
                                        let prev_len = cur_seq.len();
                                        cur_seq.push_str(sp2);
                                        build_sp_seqs(edges, idx + 1, max_alts, cur_seq, cur_wgt * (*w), out);
                                        cur_seq.truncate(prev_len);
                                    }
                                }
                                let mut sp_seqs = Vec::new();
                                let mut buf = String::with_capacity(k * 2);
                                build_sp_seqs(&syl_edges, 0, max_alts, &mut buf, 1.0, &mut sp_seqs);
                                for (sp_seq, wgt) in sp_seqs {
                                    if let Some(w_list) = eng.sp_word_map.get(&sp_seq) {
                                        for (rank, (w_str, base_sc)) in w_list.iter().take(4).enumerate() {
                                            let sc = bytes_covered * 260.0 * wgt
                                                - (k as f64) * 25.0
                                                + (*base_sc as f64) / 600.0
                                                - (rank as f64) * 14.0
                                                + eng.word_bonus(w_str, at_start);
                                            spans.push(WordSpan {
                                                end: cur_b,
                                                word: w_str.clone(),
                                                score: sc,
                                                seg: seg_str.clone(),
                                            });
                                        }
                                    }
                                }

                                // (a2) Dynamic Mandarin Grammar Synthesis (`A不A` / `A一A` / `不AB` / `很AB` / `太AB` / `A不AB`):
                                // Prevents bound literary 2-char fragments like `毫不` (`hcbu`) from ever hijacking
                                // daily conversational structures like `好不好用` (`haobuhaoyong`), `好不好` (`haobuhao`),
                                // `方不方便` (`fangbufangbian`), `可不可以` (`kebukeyi`), `喜不喜欢` (`xibuxihuan`), etc.!
                                if k == 3 {
                                    let sp0 = &syl_edges[0].sp2_cands[0].0;
                                    let sp1 = &syl_edges[1].sp2_cands[0].0;
                                    let sp2 = &syl_edges[2].sp2_cands[0].0;
                                    // Pattern 1: `A + bu/yi/mei + A` (`好不好`, `是不是`, `看一看`, `试一试`, `有没有`)
                                    if sp0 == sp2 && matches!(sp1.as_str(), "bu" | "yi" | "mw") {
                                        let mid_ch = match sp1.as_str() {
                                            "bu" => "不",
                                            "yi" => "一",
                                            _ => "没",
                                        };
                                        if let Some(ch_list) = eng.sp_char_map.get(sp0) {
                                            if let Some((a_ch, base_sc)) = ch_list.first() {
                                                if *base_sc >= 85_000 {
                                                    let syn = format!("{}{}{}", a_ch, mid_ch, a_ch);
                                                    let sc = bytes_covered * 262.0 + 175.0 + (*base_sc as f64) / 800.0;
                                                    spans.push(WordSpan {
                                                        end: cur_b,
                                                        word: syn,
                                                        score: sc,
                                                        seg: seg_str.clone(),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                    // Pattern 2: `bu/hen/tai/zhen/zui/geng/ting + AB` (`不好用`, `很好用`, `太好用`, `太难用`, `不方便`)
                                    let pfx_opt = match sp0.as_str() {
                                        "bu" => Some("不"),
                                        "hf" => Some("很"),
                                        "td" => Some("太"),
                                        "vf" => Some("真"),
                                        "zv" => Some("最"),
                                        "gg" => Some("更"),
                                        "tk" => Some("挺"),
                                        _ => None,
                                    };
                                    if let Some(pfx_ch) = pfx_opt {
                                        let ab_seq = format!("{}{}", sp1, sp2);
                                        if let Some(w_list) = eng.sp_word_map.get(&ab_seq) {
                                            if let Some((ab_word, base_sc)) = w_list.first() {
                                                if *base_sc >= 80_000 && ab_word.chars().count() == 2 {
                                                    let syn = format!("{}{}", pfx_ch, ab_word);
                                                    let sc = bytes_covered * 261.0 + 165.0 + (*base_sc as f64) / 700.0;
                                                    spans.push(WordSpan {
                                                        end: cur_b,
                                                        word: syn,
                                                        score: sc,
                                                        seg: seg_str.clone(),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                } else if k == 4 {
                                    // Pattern 3: `A + bu/mei + A + B` (`好不好用`, `好不好看`, `可不可以`, `喜不喜欢`, `方不方便`, `难不难用`)
                                    let sp0 = &syl_edges[0].sp2_cands[0].0;
                                    let sp1 = &syl_edges[1].sp2_cands[0].0;
                                    let sp2 = &syl_edges[2].sp2_cands[0].0;
                                    let sp3 = &syl_edges[3].sp2_cands[0].0;
                                    if sp0 == sp2 && matches!(sp1.as_str(), "bu" | "mw") {
                                        let mid_ch = if sp1 == "bu" { "不" } else { "没" };
                                        let ab_seq = format!("{}{}", sp2, sp3);
                                        if let Some(w_list) = eng.sp_word_map.get(&ab_seq) {
                                            for (rank, (ab_word, base_sc)) in w_list.iter().take(2).enumerate() {
                                                if *base_sc >= 65_000 && ab_word.chars().count() == 2 {
                                                    if let Some(first_ch) = ab_word.chars().next() {
                                                        let syn = format!("{}{}{}", first_ch, mid_ch, ab_word);
                                                        let sc = bytes_covered * 265.0
                                                            + 215.0
                                                            + (*base_sc as f64) / 650.0
                                                            - (rank as f64) * 18.0;
                                                        spans.push(WordSpan {
                                                            end: cur_b,
                                                            word: syn,
                                                            score: sc,
                                                            seg: seg_str.clone(),
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // (b) Initial abbreviation or Hybrid Full+Initial lookup in `init_word_map`
                            if syl_edges.iter().all(|e| e.init_char.is_some()) && full_count < k {
                                let init_seq: String = syl_edges.iter().filter_map(|e| e.init_char).collect();
                                if let Some(w_list) = eng.init_word_map.get(&init_seq) {
                                    for (rank, (w_str, base_sc)) in w_list.iter().take(5).enumerate() {
                                        // If any syllable in `syl_edges` was a full syllable (e.g. `wmen`), verify its `sp2` matches that character in `w_str`!
                                        if full_count > 0 {
                                            let w_chars: Vec<char> = w_str.chars().collect();
                                            if w_chars.len() != k {
                                                continue;
                                            }
                                            let mut ok = true;
                                            for (si, se) in syl_edges.iter().enumerate() {
                                                if se.is_full && !se.sp2_cands.is_empty() {
                                                    if let Some(ch_sp2) = eng.char_sp2_matching_initial(w_chars[si], se.init_char) {
                                                        if !se.sp2_cands.iter().any(|(cand_sp2, _)| cand_sp2 == ch_sp2) {
                                                            ok = false;
                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                            if !ok {
                                                continue;
                                            }
                                        }
                                        let sc = bytes_covered * 195.0
                                            + (full_count as f64) * 55.0
                                            + (*base_sc as f64) / 800.0
                                            - (rank as f64) * 15.0
                                            + eng.word_bonus(w_str, at_start);
                                        spans.push(WordSpan {
                                            end: cur_b,
                                            word: w_str.clone(),
                                            score: sc,
                                            seg: seg_str.clone(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                let k_now = path.iter().filter(|e| !e.raw.is_empty()).count();
                if k_now >= 6 || cur_b >= adj.len() - 1 {
                    return;
                }
                for edge in &adj[cur_b] {
                    path.push(edge);
                    walk_syllables(eng, adj, b0, edge.end, path, spans);
                    path.pop();
                }
            }
            let mut path = Vec::new();
            walk_syllables(self, &adj, b0, b0, &mut path, &mut spans_from[b0]);
            // Sort spans starting at `b0` descending by score
            spans_from[b0].sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        }

        // 3. Viterbi Dynamic Programming from Byte `0` to Byte `m` to decode the Best Full Sentence!
        let mut dp_score = vec![f64::NEG_INFINITY; m + 1];
        let mut dp_prev: Vec<Option<(usize, String, String)>> = vec![None; m + 1];
        dp_score[0] = 0.0;

        for b in 0..m {
            if dp_score[b].is_infinite() {
                continue;
            }
            for sp in &spans_from[b] {
                // Word-boundary transition penalty encourages fewer, longer multi-character words
                let cand_sc = dp_score[b] + sp.score - 180.0;
                if cand_sc > dp_score[sp.end] {
                    dp_score[sp.end] = cand_sc;
                    dp_prev[sp.end] = Some((b, sp.word.clone(), sp.seg.clone()));
                }
            }
        }

        // Reconstruct best full sentence reaching `m` (or furthest reachable byte if trailing unparsed char)
        let target_end = (0..=m).rev().find(|&b| dp_prev[b].is_some()).unwrap_or(0);
        if target_end > 0 {
            let mut words = Vec::new();
            let mut segs = Vec::new();
            let mut cur = target_end;
            while let Some((prev_b, ref w, ref sg)) = dp_prev[cur] {
                words.push(w.clone());
                if !sg.is_empty() {
                    segs.push(sg.clone());
                }
                cur = prev_b;
                if cur == 0 {
                    break;
                }
            }
            words.reverse();
            segs.reverse();
            let full_sentence = words.concat();
            self.preedit_segmented = segs.join("'");
            if target_end < m {
                if !self.preedit_segmented.is_empty() {
                    self.preedit_segmented.push('\'');
                }
                self.preedit_segmented.push_str(&raw[target_end..]);
            }

            if !full_sentence.is_empty() && !self.candidates.iter().any(|c| c.text == full_sentence) {
                let mut comment = String::new();
                if is_quanpin && m >= 5 {
                    if let Some(init_sc) = self.word_init_hint.get(&full_sentence) {
                        if init_sc.len() < m {
                            comment = format!("⚡简拼 {}", init_sc);
                        }
                    }
                }
                if target_end < m {
                    self.candidate_consumed.insert(full_sentence.clone(), target_end);
                }
                self.candidates.push(Candidate {
                    text: full_sentence,
                    comment,
                    layer: "viterbi".to_string(),
                });
            }
        }

        // 4. Add other full-buffer (`0..m`) exact/fuzzy/initial word matches
        for sp in &spans_from[0] {
            if sp.end == m && !self.candidates.iter().any(|c| c.text == sp.word) {
                let mut comment = String::new();
                if is_quanpin && m >= 5 {
                    if let Some(init_sc) = self.word_init_hint.get(&sp.word) {
                        if init_sc.len() < m {
                            comment = format!("⚡简拼 {}", init_sc);
                        }
                    }
                }
                self.candidates.push(Candidate {
                    text: sp.word.clone(),
                    comment,
                    layer: "full_span".to_string(),
                });
            }
        }

        // In Shuangpin mode at 4 letters, also include traditional 4-code phrase matches (`ABCc` / `ABCZ`) from `flypy_map`
        if !is_quanpin && m == 4 {
            if let Some(sys_cands) = self.flypy_map.get(&raw) {
                for c in sys_cands.iter().filter(|c| c.text.chars().count() >= 2) {
                    if !self.candidates.iter().any(|ex| ex.text == c.text) {
                        self.candidates.push(Candidate {
                            text: c.text.clone(),
                            comment: String::new(),
                            layer: c.layer.clone(),
                        });
                    }
                }
            }
        }

        // 5. Progressive Prefix Phrase Candidates (`渐进分词选词`: multi-syllable prefixes `0..sp.end` where `sp.end < m`)
        for sp in &spans_from[0] {
            if sp.end < m && sp.word.chars().count() >= 2 {
                if !self.candidates.iter().any(|c| c.text == sp.word) {
                    self.candidate_consumed.insert(sp.word.clone(), sp.end);
                    self.candidates.push(Candidate {
                        text: sp.word.clone(),
                        comment: String::new(),
                        layer: "prefix_word".to_string(),
                    });
                    if self.candidates.len() >= 12 {
                        break;
                    }
                }
            }
        }

        // 6. Single-Character Candidates for the First Syllable (or Full Single Syllable if 1 syllable)
        // Find longest full syllable edge starting at `0` (or initial edge if no full edge exists)
        let first_edges: Vec<&SylEdge> = adj[0]
            .iter()
            .filter(|e| !e.raw.is_empty())
            .collect();
        for e0 in first_edges {
            if e0.is_full {
                for (sp2, _) in &e0.sp2_cands {
                    if let Some(ch_list) = self.sp_char_map.get(sp2) {
                        for (ch_str, _) in ch_list {
                            if !self.candidates.iter().any(|c| c.text == *ch_str) {
                                if e0.end < m {
                                    self.candidate_consumed.insert(ch_str.clone(), e0.end);
                                }
                                self.candidates.push(Candidate {
                                    text: ch_str.clone(),
                                    comment: String::new(),
                                    layer: "sp_char".to_string(),
                                });
                            }
                        }
                    }
                }
            } else if e0.end == m {
                // Single initial typed (e.g. `w`, `zh`, `sh`, `ch`) -> list top characters for that initial
                if let Some(ic) = e0.init_char {
                    if let Some((top_ch, _)) = self.top_char_for_initial(ic) {
                        if !self.candidates.iter().any(|c| c.text == top_ch) {
                            self.candidates.push(Candidate {
                                text: top_ch,
                                comment: String::new(),
                                layer: "init_char".to_string(),
                            });
                        }
                    }
                    for (sp2, ch_list) in &self.sp_char_map {
                        if sp2.starts_with(ic) {
                            for (ch_str, _) in ch_list.iter().take(4) {
                                if !self.candidates.iter().any(|c| c.text == *ch_str) {
                                    self.candidates.push(Candidate {
                                        text: ch_str.clone(),
                                        comment: String::new(),
                                        layer: "init_char".to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        self.candidates.truncate(45);

        // 7. Ghost [Tab] Sentence / Super-Phrase Completion in Quanpin & Shuangpin
        if self.config.enable_ghost_tab && (2..=6).contains(&m) {
            for (code4, phrase) in &self.turbo_phrases {
                if code4.starts_with(&raw) && self.candidates.first().map(|c| &c.text) != Some(phrase) {
                    self.ghost_tab_text = Some((phrase.clone(), format!("⚡ [Tab] {}", phrase)));
                    break;
                }
            }
        }
    }
}

// ============================================================================
// QUICK-ADD HELPERS (phrase code suggestion + user lexicon writer)
// ============================================================================

/// Primary code of one character from the reverse table (`片` -> `pmpv`), lower-cased.
/// Only Han / non-ASCII characters with at least a 2-letter code count.
fn reverse_primary_code<'a>(reverse: &'a HashMap<String, String>, ch: char) -> Option<&'a str> {
    if ch.is_ascii() || ch.is_whitespace() {
        return None;
    }
    let mut buf = [0u8; 4];
    let codes = reverse.get(ch.encode_utf8(&mut buf) as &str)?;
    let first = codes.split(',').map(str::trim).find(|c| c.len() >= 2 && c.chars().all(|x| x.is_ascii_alphabetic()))?;
    Some(first)
}

/// Suggest the standard 音形 phrase code for `text` (characters without a reverse entry, e.g.
/// ASCII, punctuation or `{date}` placeholders, are ignored):
/// * 1 char  -> its full code
/// * 2 chars -> AaBb (first two letters = double pinyin of each)
/// * 3 chars -> A B Cc
/// * 4+ chars -> A B C Z (first letter of chars 1, 2, 3 and the last)
pub fn suggest_phrase_code(reverse: &HashMap<String, String>, text: &str) -> Option<String> {
    let codes: Vec<&str> = text.chars().filter_map(|c| reverse_primary_code(reverse, c)).collect();
    let first = |s: &str| s[..1].to_ascii_lowercase();
    let two = |s: &str| s[..2].to_ascii_lowercase();
    let out = match codes.len() {
        0 => return None,
        1 => codes[0].to_ascii_lowercase(),
        2 => format!("{}{}", two(codes[0]), two(codes[1])),
        3 => format!("{}{}{}", first(codes[0]), first(codes[1]), two(codes[2])),
        n => format!("{}{}{}{}", first(codes[0]), first(codes[1]), first(codes[2]), first(codes[n - 1])),
    };
    Some(out)
}

impl HeyuEngine {
    /// [`suggest_phrase_code`] against this engine's reverse table.
    pub fn suggest_phrase_code(&self, text: &str) -> Option<String> {
        suggest_phrase_code(&self.reverse_map, text)
    }
}

/// Insert `(code, text, comment)` at the top of the user lexicon file at `path` (creating it with
/// the standard header if missing). An identical `(code, text)` line is removed first so the new
/// entry becomes #1 for that code. Newlines in `text` are stored as `\n`; tabs become spaces.
pub fn write_user_phrase_file(path: &Path, code: &str, text: &str, comment: &str) -> bool {
    let code = code.trim();
    let text_escaped = text.trim().replace('\r', "").replace('\n', "\\n").replace('\t', " ");
    let comment = if comment.trim().is_empty() { "个人置顶" } else { comment.trim() };
    if code.is_empty() || text_escaped.is_empty() || code.contains(char::is_whitespace) {
        return false;
    }
    let existing = crate::vault::read_user_lexicon_decrypted(path).unwrap_or_else(|| {
        "# HEYU Personal Custom Lexicon (个人专属置顶词库)\n# Format: code<TAB>word<TAB>comment\n".to_string()
    });
    let mut header_lines = Vec::new();
    let mut body_lines = Vec::new();
    for line in existing.lines() {
        if line.starts_with('#') {
            header_lines.push(line.to_string());
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        if let (Some(c), Some(w)) = (parts.next(), parts.next()) {
            if c.trim() == code && w.trim() == text_escaped {
                continue; // Remove duplicate so new entry goes to the very top of this code
            }
        }
        body_lines.push(line.to_string());
    }
    if header_lines.is_empty() {
        header_lines.push("# HEYU Personal Custom Lexicon (个人专属置顶词库)".to_string());
        header_lines.push("# Format: code<TAB>word<TAB>comment".to_string());
    }
    let mut out = String::new();
    for h in header_lines {
        out.push_str(&h);
        out.push('\n');
    }
    out.push_str(&format!("{}\t{}\t{}\n", code, text_escaped, comment));
    for b in body_lines {
        out.push_str(&b);
        out.push('\n');
    }
    crate::vault::write_user_lexicon_file(path, &out)
}

#[cfg(test)]
mod quick_add_tests {
    use super::*;

    fn rev() -> HashMap<String, String> {
        let mut m = HashMap::new();
        for (c, codes) in [("片", "pmpv,pm,oxp"), ("语", "yuyk,yuy"), ("工", "gsaa,gsa,oxg"), ("作", "zorv,zor"), ("室", "uibt,ui"), ("鹤", "hedn,eh"), ("羽", "yuyy")] {
            m.insert(c.to_string(), codes.to_string());
        }
        m
    }

    #[test]
    fn phrase_code_rules() {
        let r = rev();
        assert_eq!(suggest_phrase_code(&r, "鹤").as_deref(), Some("hedn"));
        assert_eq!(suggest_phrase_code(&r, "鹤羽").as_deref(), Some("heyu"));
        assert_eq!(suggest_phrase_code(&r, "工作室").as_deref(), Some("gzui"));
        assert_eq!(suggest_phrase_code(&r, "片语工作室").as_deref(), Some("pygu"));
        assert_eq!(suggest_phrase_code(&r, "片语 AI 工作室!").as_deref(), Some("pygu"));
        assert_eq!(suggest_phrase_code(&r, "abc"), None);
        assert_eq!(suggest_phrase_code(&r, ""), None);
    }

    #[test]
    fn phrase_code_with_real_reverse_table() {
        let eng = HeyuEngine::new(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data"));
        if eng.reverse_map.is_empty() {
            return;
        }
        assert_eq!(eng.suggest_phrase_code("片语工作室").as_deref(), Some("pygu"));
    }

    #[test]
    fn user_phrase_file_puts_new_entry_on_top_and_dedups() {
        let dir = std::env::temp_dir().join(format!("heyu_qa_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let p = dir.join("user_custom.tsv");
        fs::write(&p, "# header\nhy\t鹤羽\t候选首选\npygu\t片语工作室\t直出\n").unwrap();
        assert!(write_user_phrase_file(&p, "pygu", "片语工作室", "候选"));
        assert!(write_user_phrase_file(&p, "gzui", "工作\t室", "直出"));
        let s = fs::read_to_string(&p).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "# header");
        assert_eq!(lines[1], "gzui\t工作 室\t直出");
        assert_eq!(lines[2], "pygu\t片语工作室\t候选");
        assert_eq!(lines[3], "hy\t鹤羽\t候选首选");
        assert_eq!(lines.len(), 4);
        assert!(!write_user_phrase_file(&p, "", "x", ""));
        assert!(!write_user_phrase_file(&p, "a b", "x", ""));
        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_data_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data")
    }

    #[test]
    fn test_flypy_100_percent_compatibility_and_4code_autocommit() {
        let mut eng = HeyuEngine::new(test_data_dir());
        if eng.uses_open_table() {
            return; // private table not present (public checkout): codes below are table specific
        }
        eng.mode = LangMode::ZhFlypy;
        eng.config.input_scheme = "yinxing".to_string();
        eng.config.auto_commit_4code = true;
        eng.config.show_shape_hints = true;
        assert!(eng.flypy_map.len() > 60000, "Flypy dictionary must load >60,000 codes");

        // Test 1-code (`w` -> `我`)
        let act = eng.process_key('w', false);
        assert_eq!(act, KeyAction::Consumed);
        assert_eq!(eng.candidates.first().unwrap().text, "我");
        let sp = eng.process_special_key(0x20, false); // Space
        assert_eq!(sp, KeyAction::Commit("我".to_string()));

        // Test Semicolon Secondary Short Code (`b` + `;` -> `吧`)
        eng.process_key('b', false);
        let semi = eng.process_key(';', false);
        assert_eq!(semi, KeyAction::Commit("吧".to_string()));

        // Test 4-code unique auto-commit (`hedn` -> `鹤` auto-commits on 4th key!)
        eng.process_key('h', false);
        eng.process_key('e', false);
        // At 2 letters `he`, verify Shuangpin Shape Memory Recovery Hints are populated!
        assert!(!eng.active_shape_hints.is_empty(), "2-letter Shuangpin must show shape hints");
        eng.process_key('d', false);
        let fourth = eng.process_key('n', false);
        assert_eq!(fourth, KeyAction::Commit("鹤".to_string()));

        // Test `oiu` macro in Chinese mode triggers microphone toggle and clears composition
        eng.process_key('o', false);
        eng.process_key('i', false);
        let act_oiu = eng.process_key('u', false);
        assert_eq!(act_oiu, KeyAction::Consumed);
        assert!(eng.pending_mic_toggle, "oiu in Chinese mode must trigger pending_mic_toggle");
        assert!(eng.input_buf.is_empty());
        eng.pending_mic_toggle = false;

        // Test that `o`-prefixed macros (`oiu`, `ocx`) do NOT trigger in foreign language modes (no collision!)
        eng.mode = LangMode::En;
        eng.process_key('o', false);
        eng.process_key('i', false);
        eng.process_key('u', false);
        assert!(!eng.pending_mic_toggle, "oiu must NOT trigger mic toggle in English mode");
        assert_eq!(eng.input_buf, "oiu");
        eng.clear_composition();
        eng.mode = LangMode::ZhFlypy;
    }

    fn write(p: &std::path::Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    #[test]
    fn test_core_table_fallback_order_private_legacy_open() {
        let tmp = std::env::temp_dir().join(format!("heyu_table_fallback_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let data = tmp.join("data");
        let private = tmp.join("private");
        write(&data.join("open").join("heyu_open_sys.tsv"), "zz\t开\t0\tprimary\n");
        write(&data.join("open").join("heyu_open_reverse.tsv"), "开\tzzzz\n");
        write(&data.join("open").join("heyu_open_shape_hints.tsv"), "zz\t开:zz\n");

        // only the open table exists -> open table is used for sys, reverse and hints
        let eng = HeyuEngine::with_private_dir(&data, Some(private.clone()));
        assert!(eng.uses_open_table());
        assert_eq!(eng.flypy_map.get("zz").unwrap()[0].text, "开");
        assert_eq!(eng.reverse_map.get("开").unwrap(), "zzzz");
        assert_eq!(eng.sp2_hints.get("zz").unwrap()[0].ch, "开");

        // legacy data/heyu_sys.tsv beats the open table
        write(&data.join("heyu_sys.tsv"), "zz\t旧\t0\tprimary\n");
        let eng = HeyuEngine::with_private_dir(&data, Some(private.clone()));
        assert!(!eng.uses_open_table());
        assert_eq!(eng.flypy_map.get("zz").unwrap()[0].text, "旧");

        // private table beats both
        write(&private.join("heyu_sys.tsv"), "zz\t私\t0\tprimary\n");
        let eng = HeyuEngine::with_private_dir(&data, Some(private.clone()));
        assert_eq!(eng.flypy_map.get("zz").unwrap()[0].text, "私");
        assert_eq!(eng.core_table_path.as_deref(), Some(private.join("heyu_sys.tsv").as_path()));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_shipped_open_table_types_common_words() {
        let open = test_data_dir().join("open");
        if !open.join("heyu_open_sys.tsv").exists() {
            return;
        }
        let tmp = std::env::temp_dir().join(format!("heyu_open_table_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let data = tmp.join("data");
        std::fs::create_dir_all(data.join("open")).unwrap();
        for f in ["heyu_open_sys.tsv", "heyu_open_reverse.tsv", "heyu_open_shape_hints.tsv"] {
            std::fs::copy(open.join(f), data.join("open").join(f)).unwrap();
        }
        let mut eng = HeyuEngine::with_private_dir(&data, None);
        assert!(eng.uses_open_table());
        assert!(eng.flypy_map.len() > 50000);
        eng.mode = LangMode::ZhFlypy;
        eng.process_key('w', false);
        assert_eq!(eng.candidates.first().unwrap().text, "我");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_italian_and_spanish_automatic_accents() {
        let mut eng = HeyuEngine::new(test_data_dir());

        // Switch to Italian mode: type `perche` + Space -> `perché `
        eng.mode = LangMode::It;
        for ch in "perche".chars() {
            eng.process_key(ch, false);
        }
        assert_eq!(eng.candidates.first().unwrap().text, "perché");
        let commit_it = eng.process_special_key(0x20, false);
        assert_eq!(commit_it, KeyAction::Commit("perché ".to_string()));

        // Switch to Spanish mode: type `espanol` + Space -> `español `
        eng.mode = LangMode::Es;
        for ch in "espanol".chars() {
            eng.process_key(ch, false);
        }
        assert_eq!(eng.candidates.first().unwrap().text, "español");
        let commit_es = eng.process_special_key(0x20, false);
        assert_eq!(commit_es, KeyAction::Commit("español ".to_string()));
    }

    #[test]
    fn test_220_cpm_contextual_ghost_tab_prediction() {
        let mut eng = HeyuEngine::new(test_data_dir());
        if eng.uses_open_table() {
            return; // `ybww` is only unique (auto-commit) in the private table
        }
        eng.mode = LangMode::ZhFlypy;
        eng.config.input_scheme = "yinxing".to_string();
        eng.config.auto_commit_4code = true;

        // Type `ybww` (`因为` in Flypy: yb + ww)
        for ch in "ybww".chars() {
            eng.process_key(ch, false);
        }
        // `因为` auto-commits on 4th key, and post_commit_ghosts should now offer `所以`!
        assert!(!eng.post_commit_ghosts.is_empty());
        assert_eq!(eng.post_commit_ghosts[0], "所以");
        let tab_act = eng.process_special_key(0x09, false); // Tab
        assert_eq!(tab_act, KeyAction::Commit("所以".to_string()));
    }

    #[test]
    fn test_inline_expression_calculator_and_financial_uppercase() {
        let mut eng = HeyuEngine::new(test_data_dir());
        eng.mode = LangMode::ZhFlypy;
        for ch in "=125*38".chars() {
            assert_eq!(eng.process_key(ch, false), KeyAction::Consumed);
        }
        assert_eq!(eng.candidates.len(), 3);
        assert_eq!(eng.candidates[0].text, "4750");
        assert_eq!(eng.candidates[1].text, "125*38=4750");
        assert_eq!(eng.candidates[2].text, "肆仟柒佰伍拾元整");
        // Apostrophe `'` commits Candidate #3 (Chinese financial uppercase)
        assert_eq!(
            eng.process_key('\'', false),
            KeyAction::Commit("肆仟柒佰伍拾元整".to_string())
        );

        for ch in "=(100+250)/7".chars() {
            eng.process_key(ch, false);
        }
        assert_eq!(eng.candidates[0].text, "50");
        assert_eq!(
            eng.process_key(';', false),
            KeyAction::Commit("(100+250)/7=50".to_string())
        );

        for ch in "=2^10".chars() {
            eng.process_key(ch, false);
        }
        assert_eq!(eng.candidates[0].text, "1024");
        assert_eq!(
            eng.process_special_key(0x20, false),
            KeyAction::Commit("1024".to_string())
        );
    }

    #[test]
    fn test_quanpin_and_shuangpin_compatibility_modes() {
        assert!(quanpin_to_shuangpin_codes("shuang").contains(&"ul".to_string()));
        assert!(quanpin_to_shuangpin_codes("zhongguo").contains(&"vsgo".to_string()));
        assert!(quanpin_to_shuangpin_codes("shuangpin").contains(&"ulpb".to_string()));
        assert!(quanpin_to_shuangpin_codes("heyu").contains(&"heyu".to_string()));

        let mut eng = HeyuEngine::new(test_data_dir());
        eng.mode = LangMode::ZhFlypy;
        eng.config.input_scheme = "quanpin".to_string();
        for ch in "zhongguo".chars() {
            eng.process_key(ch, false);
        }
        assert!(
            eng.candidates.iter().any(|c| c.text == "中国"),
            "quanpin 'zhongguo' should produce candidate '中国'"
        );
        eng.clear_composition();

        eng.config.input_scheme = "shuangpin".to_string();
        for ch in "he".chars() {
            eng.process_key(ch, false);
        }
        assert!(
            eng.candidates.iter().any(|c| c.text == "鹤" || c.text == "和"),
            "shuangpin 'he' should list single characters without requiring shape code"
        );
    }

    #[test]
    fn test_encrypted_history_macro_and_zero_wipe() {
        let tmp = std::env::temp_dir().join(format!("heyu_eng_hist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let mut eng = HeyuEngine::with_private_dir(&tmp, None);
        eng.mode = LangMode::ZhFlypy;
        eng.config.history_retention_mins = 30;
        eng.record_commit("鹤羽音形测试句子", 4);
        assert!(tmp.join("input_history.enc").is_file());

        // Type `;h` to open encrypted history recovery in HUD
        eng.process_key(';', false);
        eng.process_key('h', false);
        assert_eq!(eng.candidates.first().unwrap().text, "鹤羽音形测试句子");
        eng.clear_composition();

        // Setting retention to 0 and pruning permanently wipes the encrypted history file
        eng.config.history_retention_mins = 0;
        eng.prune_and_save_history();
        assert!(!tmp.join("input_history.enc").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_traditional_chinese_conversion() {
        assert_eq!(
            HeyuEngine::to_traditional_chinese("鹤羽音形输入法"),
            "鶴羽音形輸入法"
        );
        assert_eq!(
            HeyuEngine::to_traditional_chinese("中国语言文字测试"),
            "中國語言文字測試"
        );
        let mut eng = HeyuEngine::new(test_data_dir());
        eng.mode = LangMode::ZhFlypy;
        eng.config.traditional_chinese = true;
        for ch in "zhongguo".chars() {
            eng.process_key(ch, false);
        }
        eng.config.input_scheme = "quanpin".to_string();
        eng.clear_composition();
        for ch in "zhongguo".chars() {
            eng.process_key(ch, false);
        }
        assert!(
            eng.candidates.iter().any(|c| c.text == "中國"),
            "with traditional_chinese=true, quanpin 'zhongguo' should produce '中國'"
        );
    }

    #[test]
    fn test_shift_toggle_commits_preedit_english_directly() {
        let mut eng = HeyuEngine::new(test_data_dir());
        eng.mode = LangMode::ZhFlypy;
        let mut sess = crate::session::ImeSession::new();
        for ch in "win".chars() {
            let vk = ch.to_ascii_uppercase() as u16;
            sess.handle(&mut eng, &crate::ipc::Request::key(vk, ch, 0, false));
        }
        assert_eq!(eng.input_buf, "win");
        // Tap Left Shift (key down + key up)
        sess.handle(&mut eng, &crate::ipc::Request::key(crate::session::VK_LSHIFT, '\0', crate::ipc::MOD_SHIFT | crate::ipc::MOD_LSHIFT, false));
        let out = sess.handle(&mut eng, &crate::ipc::Request::key(crate::session::VK_LSHIFT, '\0', 0, true));
        assert_eq!(out.response.commit_text, "win");
        assert!(out.response.ascii_mode);
        assert_eq!(eng.mode, LangMode::Direct);
        assert!(eng.input_buf.is_empty());
    }

    #[test]
    fn test_quanpin_whole_sentence_viterbi_and_120cpm_superpowers() {
        let mut eng = HeyuEngine::new(test_data_dir());
        eng.mode = LangMode::ZhFlypy;
        eng.config.input_scheme = "quanpin".to_string();

        // 1. Whole-sentence Quanpin Viterbi decoding (> 20 letters)
        for ch in "womenjintianxiawukaihui".chars() {
            eng.process_key(ch, false);
        }
        assert_eq!(eng.candidates.first().unwrap().text, "我们今天下午开会");
        assert_eq!(eng.preedit_display(), "wo'men'jin'tian'xia'wu'kai'hui");
        assert_eq!(
            eng.process_special_key(0x20, false),
            KeyAction::Commit("我们今天下午开会".to_string())
        );

        for ch in "xianzaixuyaowozhongqiruanjianma".chars() {
            eng.process_key(ch, false);
        }
        assert_eq!(eng.candidates.first().unwrap().text, "现在需要我重启软件吗");
        eng.clear_composition();

        // 2. Syllable boundary disambiguation (`fangan` -> `方案` & `反感`, `qinai` -> `亲爱`, `wanan` -> `晚安`, `tiananmen` -> `天安门`, `xian` -> `现` & `西安`)
        for ch in "fangan".chars() {
            eng.process_key(ch, false);
        }
        assert!(eng.candidates.iter().any(|c| c.text == "方案"));
        assert!(eng.candidates.iter().any(|c| c.text == "反感"));
        eng.clear_composition();

        for ch in "qinai".chars() {
            eng.process_key(ch, false);
        }
        assert_eq!(eng.candidates.first().unwrap().text, "亲爱");
        eng.clear_composition();

        for ch in "tiananmen".chars() {
            eng.process_key(ch, false);
        }
        assert_eq!(eng.candidates.first().unwrap().text, "天安门");
        eng.clear_composition();

        for ch in "xian".chars() {
            eng.process_key(ch, false);
        }
        assert!(eng.candidates.iter().any(|c| c.text == "现"));
        assert!(eng.candidates.iter().any(|c| c.text == "西安"));
        eng.clear_composition();

        // 3. Super Initial Abbreviations (`wm`, `sm`, `jtkh`, `rgzn`, `srf`) & Hybrid (`womenjt`, `wmen`)
        for (input, expected) in [
            ("wm", "我们"),
            ("sm", "什么"),
            ("jtkh", "今天开会"),
            ("rgzn", "人工智能"),
            ("srf", "输入法"),
            ("womenjt", "我们今天"),
            ("wmen", "我们"),
        ] {
            for ch in input.chars() {
                eng.process_key(ch, false);
            }
            assert!(
                eng.candidates.iter().take(3).any(|c| c.text == expected),
                "input '{}' should produce '{}' in top 3, got {:?}",
                input,
                expected,
                eng.candidates.iter().take(5).map(|c| &c.text).collect::<Vec<_>>()
            );
            eng.clear_composition();
        }

        // 4. Incomplete mid-typing prefix (`zho` / `zhon` -> `中`), Fuzzy Pinyin (`zongguo` -> `中国`), and Typo Auto-Correction (`zhonggou` -> `中国`, `xign` -> `行`)
        for ch in "zhon".chars() {
            eng.process_key(ch, false);
        }
        assert!(eng.candidates.iter().any(|c| c.text == "中"));
        eng.clear_composition();

        for ch in "zongguo".chars() {
            eng.process_key(ch, false);
        }
        assert!(eng.candidates.iter().any(|c| c.text == "中国"));
        eng.clear_composition();

        for ch in "zhonggou".chars() {
            eng.process_key(ch, false);
        }
        assert!(eng.candidates.iter().any(|c| c.text == "中国"));
        eng.clear_composition();

        for ch in "xign".chars() {
            eng.process_key(ch, false);
        }
        assert!(eng.candidates.iter().any(|c| c.text == "行"));
        eng.clear_composition();

        // 5. Progressive Prefix Word Selection (`渐进分词选词不丢余码`)
        for ch in "womenjintian".chars() {
            eng.process_key(ch, false);
        }
        assert_eq!(eng.candidates[0].text, "我们今天");
        let wm_idx = eng.candidates.iter().position(|c| c.text == "我们").expect("'我们' should be offered as prefix candidate");
        let act = eng.commit_candidate_at_page_offset(wm_idx, false);
        assert_eq!(act, KeyAction::Commit("我们".to_string()));
        assert_eq!(eng.input_buf, "jintian", "remaining syllables must stay in input_buf");
        assert_eq!(eng.candidates.first().unwrap().text, "今天");
        eng.clear_composition();

        // 6. Conversational A-bu-AB / A-bu-A / degree-adverb phrases & 10-candidate two-handed selection (`0` selects 10th)
        for (input, expected_top) in [
            ("haobuhaoyong", "好不好用"),
            ("haobuhao", "好不好"),
            ("buhaoyong", "不好用"),
            ("tainanyongle", "太难用了"),
            ("fanrenxing", "反人性"),
            ("fangbufangbian", "方不方便"),
            ("kebukeyi", "可不可以"),
            ("xibuxihuan", "喜不喜欢"),
        ] {
            for ch in input.chars() {
                eng.process_key(ch, false);
            }
            assert_eq!(
                eng.candidates.first().unwrap().text,
                expected_top,
                "input '{}' should produce '{}' at #1, got {:?}",
                input,
                expected_top,
                eng.candidates.iter().take(5).map(|c| &c.text).collect::<Vec<_>>()
            );
            eng.clear_composition();
        }

        eng.config.page_size = 10;
        for ch in "shi".chars() {
            eng.process_key(ch, false);
        }
        let page_cands = eng.current_page_candidates();
        assert_eq!(page_cands.len(), 10, "page_size=10 should return 10 candidates");
        let tenth_text = page_cands[9].text.clone();
        let act10 = eng.process_key('0', false);
        assert_eq!(act10, KeyAction::Commit(tenth_text));
    }

    #[test]
    fn test_native_chinese_and_foreign_language_punctuation() {
        let mut eng = HeyuEngine::new(test_data_dir());
        eng.mode = LangMode::ZhFlypy;
        eng.config.chinese_punctuation = true;
        eng.config.input_scheme = "quanpin".to_string();

        // Chinese brackets, parentheses, curly braces, semicolons, currency, tilde
        assert_eq!(eng.process_key('[', false), KeyAction::Commit("【".to_string()));
        assert_eq!(eng.process_key(']', false), KeyAction::Commit("】".to_string()));
        assert_eq!(eng.process_key('(', true), KeyAction::Commit("（".to_string()));
        assert_eq!(eng.process_key(')', true), KeyAction::Commit("）".to_string()));
        assert_eq!(eng.process_key('{', true), KeyAction::Commit("｛".to_string()));
        assert_eq!(eng.process_key('}', true), KeyAction::Commit("｝".to_string()));
        assert_eq!(eng.process_key(';', false), KeyAction::Commit("；".to_string()));
        assert_eq!(eng.process_key('$', true), KeyAction::Commit("￥".to_string()));
        assert_eq!(eng.process_key('~', true), KeyAction::Commit("～".to_string()));

        // Smart paired Chinese double & single quotes
        assert_eq!(eng.process_key('"', true), KeyAction::Commit("“".to_string()));
        assert_eq!(eng.process_key('"', true), KeyAction::Commit("”".to_string()));
        assert_eq!(eng.process_key('\'', false), KeyAction::Commit("‘".to_string()));
        assert_eq!(eng.process_key('\'', false), KeyAction::Commit("’".to_string()));

        // Typing Chinese word followed directly by `(` or `[` commits both the word and the Chinese punctuation!
        for ch in "women".chars() {
            eng.process_key(ch, false);
        }
        assert_eq!(eng.process_key('(', true), KeyAction::Commit("我们（".to_string()));

        // Spanish smart inverted `¿ ... ?` and `¡ ... !` and angular quotes `« ... »`
        eng.mode = LangMode::Es;
        assert_eq!(eng.process_key('?', true), KeyAction::Commit("¿".to_string()));
        for ch in "hola".chars() {
            eng.process_key(ch, false);
        }
        assert_eq!(eng.process_key('?', true), KeyAction::Commit("hola?".to_string()));
        assert_eq!(eng.process_key('!', true), KeyAction::Commit("¡".to_string()));
        assert_eq!(eng.process_key('!', true), KeyAction::Commit("!".to_string()));
        assert_eq!(eng.process_key('<', true), KeyAction::Commit("«".to_string()));
        assert_eq!(eng.process_key('>', true), KeyAction::Commit("»".to_string()));
        assert_eq!(eng.process_key('$', true), KeyAction::Commit("€".to_string()));

        // Italian elision apostrophe (`l’`, `un’`) and angular quotes
        eng.mode = LangMode::It;
        eng.process_key('l', false);
        assert_eq!(eng.process_key('\'', false), KeyAction::Commit("l’".to_string()));
        assert_eq!(eng.process_key('<', true), KeyAction::Commit("«".to_string()));
        assert_eq!(eng.process_key('>', true), KeyAction::Commit("»".to_string()));
    }
}
