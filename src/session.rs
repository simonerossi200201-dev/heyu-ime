//! HEYU (鹤羽输入法) - Key-event session logic shared by every front-end
//! ======================================================================
//! Pure state machine (no windows, no I/O): turns an IPC [`Request`] into engine calls and a
//! [`Response`]. Used by the pipe server (TSF mode), by the `--hook` fallback daemon and by the
//! integration tests, so all three behave identically:
//! * Left-Shift tap (down + up with no other key in between) toggles Chinese <-> direct mode.
//! * Ctrl+Shift+L / Ctrl+Shift+Space cycle languages, Ctrl+Shift+T cycles the HUD theme,
//!   Ctrl+Shift+H toggles the shuangpin shape hints.
//! * Ctrl / Win shortcuts are never eaten (an active composition is cancelled first).
//! * Space/Tab/Backspace/Enter/Esc/PageUp/PageDown and Alt+1..6 go to `process_special_key`,
//!   printable keys go to `process_key`.

use crate::engine::{HeyuEngine, KeyAction, LangMode};
use crate::hud::THEMES;
use crate::ipc::*;

pub const VK_SHIFT: u16 = 0x10;
pub const VK_LSHIFT: u16 = 0xA0;
pub const VK_RSHIFT: u16 = 0xA1;

/// Result of handling one request.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub response: Response,
    /// HUD / status pill must be redrawn.
    pub refresh_ui: bool,
}

#[derive(Debug, Default)]
pub struct ImeSession {
    lshift_tap_candidate: bool,
    /// Last caret/composition rectangle reported by a TSF client (screen coordinates).
    pub last_caret: Option<CaretRect>,
    /// Process id of the client that sent the last focus/key request.
    pub last_pid: u32,
    /// Queued voice text from `heyu-voice.exe` to be committed atomically via TSF on `vk == 0x88`.
    pub pending_voice_commit: Option<String>,
}

fn is_modifier_vk(vk: u16) -> bool {
    matches!(vk, 0x10 | 0x11 | 0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5)
}

fn is_navigation_vk(vk: u16) -> bool {
    // End, Home, Left, Up, Right, Down, Delete
    matches!(vk, 0x23 | 0x24 | 0x25 | 0x26 | 0x27 | 0x28 | 0x2E)
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

impl ImeSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Full engine snapshot as a response.
    pub fn snapshot(engine: &HeyuEngine, eaten: bool, commit_text: String) -> Response {
        let preedit = if engine.mode == LangMode::Direct { String::new() } else { engine.input_buf.clone() };
        Response {
            eaten,
            ascii_mode: engine.mode == LangMode::Direct,
            hud_visible: engine.is_hud_visible(),
            unavailable: false,
            commit_text,
            preedit_caret: utf16_len(&preedit),
            preedit_text: preedit,
        }
    }

    fn from_action(engine: &HeyuEngine, act: KeyAction) -> Outcome {
        match act {
            KeyAction::PassThrough => Outcome { response: Self::snapshot(engine, false, String::new()), refresh_ui: false },
            KeyAction::Consumed => Outcome { response: Self::snapshot(engine, true, String::new()), refresh_ui: true },
            KeyAction::Commit(t) => Outcome { response: Self::snapshot(engine, true, t), refresh_ui: true },
        }
    }

    fn pass(engine: &HeyuEngine, refresh_ui: bool) -> Outcome {
        Outcome { response: Self::snapshot(engine, false, String::new()), refresh_ui }
    }

    fn eat(engine: &HeyuEngine) -> Outcome {
        Outcome { response: Self::snapshot(engine, true, String::new()), refresh_ui: true }
    }

    fn clear_all(&mut self, engine: &mut HeyuEngine) -> bool {
        self.lshift_tap_candidate = false;
        let was_visible = engine.is_hud_visible();
        engine.clear_composition();
        engine.post_commit_ghosts.clear();
        was_visible
    }

    pub fn handle(&mut self, engine: &mut HeyuEngine, req: &Request) -> Outcome {
        if let Some(c) = req.caret {
            if c.right >= c.left && c.bottom >= c.top && !(c.left == 0 && c.top == 0 && c.right == 0 && c.bottom == 0) {
                self.last_caret = Some(c);
            }
        }
        if req.pid != 0 {
            self.last_pid = req.pid;
        }
        match req.kind {
            RequestKind::Key => self.handle_key(engine, req),
            RequestKind::Caret => Self::pass(engine, engine.is_hud_visible()),
            RequestKind::Ping => Self::pass(engine, false),
            RequestKind::FocusIn | RequestKind::Reset => {
                let refresh = self.clear_all(engine);
                Self::pass(engine, refresh)
            }
            RequestKind::SetAsciiMode => {
                let is_direct = engine.mode == LangMode::Direct;
                let want_direct = match req.arg {
                    0 => false,
                    1 => true,
                    _ => !is_direct,
                };
                if want_direct != is_direct {
                    let committed = engine.toggle_direct_mode().unwrap_or_default();
                    return Outcome {
                        response: Self::snapshot(engine, false, committed),
                        refresh_ui: true,
                    };
                }
                Self::pass(engine, true)
            }
        }
    }

    fn handle_key(&mut self, engine: &mut HeyuEngine, req: &Request) -> Outcome {
        let vk = req.vk;
        // Fast-path passthrough for injected Unicode packets (VK_PACKET = 0xE7):
        // Never disturb `lshift_tap_candidate` or active keyboard composition!
        if vk == 0xE7 {
            return Self::pass(engine, false);
        }
        // Atomic TSF Voice Commit trigger (`vk == 0x88` key-down from `heyu-voice.exe`):
        // Commits the entire recognized voice sentence in one TSF EditSession and triggers `report_caret`.
        if vk == 0x88 {
            if !req.key_up {
                if let Some(vtext) = self.pending_voice_commit.take() {
                    engine.push_external_commit(&vtext);
                    return Outcome {
                        response: Self::snapshot(engine, true, vtext),
                        refresh_ui: false,
                    };
                }
            }
            return Self::pass(engine, false);
        }

        let m = req.modifiers;
        let shift = m & MOD_SHIFT != 0;
        let ctrl = m & MOD_CTRL != 0;
        let alt = m & MOD_ALT != 0;
        let win = m & MOD_WIN != 0;
        let caps = m & MOD_CAPSLOCK != 0;

        if req.key_up {
            if matches!(vk, VK_SHIFT | VK_LSHIFT | VK_RSHIFT) && self.lshift_tap_candidate {
                self.lshift_tap_candidate = false;
                let committed = engine.toggle_direct_mode().unwrap_or_default();
                return Outcome {
                    response: Self::snapshot(engine, false, committed),
                    refresh_ui: true,
                };
            }
            return Self::pass(engine, false);
        }

        // ---- key down ----
        if matches!(vk, VK_SHIFT | VK_LSHIFT | VK_RSHIFT) && !ctrl && !alt && !win {
            self.lshift_tap_candidate = true;
            return Self::pass(engine, false);
        }
        // Any other key cancels a pending Left-Shift tap.
        self.lshift_tap_candidate = false;
        if is_modifier_vk(vk) {
            return Self::pass(engine, false);
        }

        if ctrl && shift && !alt && !win {
            match vk {
                0x4C | 0x20 => {
                    engine.cycle_language_mode();
                    return Self::eat(engine);
                }
                0x54 => {
                    engine.config.theme_index = (engine.config.theme_index + 1) % THEMES.len();
                    engine.save_config();
                    return Self::eat(engine);
                }
                0x48 => {
                    engine.config.show_shape_hints = !engine.config.show_shape_hints;
                    engine.save_config();
                    engine.refresh_candidates();
                    return Self::eat(engine);
                }
                0x42 => {
                    // Ctrl+Shift+B: toggle hide candidate HUD (blind typing mode)
                    engine.config.hide_candidate_hud = !engine.config.hide_candidate_hud;
                    engine.save_config();
                    return Self::eat(engine);
                }
                0x49 => {
                    // Ctrl+Shift+I: cycle foreign language immersion mode
                    engine.config.immersion_mode = match engine.config.immersion_mode.as_str() {
                        "OFF" => "ALL".to_string(),
                        "ALL" => "IT".to_string(),
                        "IT" => "ES".to_string(),
                        "ES" => "EN".to_string(),
                        _ => "OFF".to_string(),
                    };
                    engine.save_config();
                    engine.refresh_candidates();
                    return Self::eat(engine);
                }
                0xBB | 0x6B => {
                    // Ctrl+Shift+= / +: increase candidate HUD font size
                    engine.config.font_scale = (engine.config.font_scale + 0.10).clamp(0.75, 2.0);
                    engine.save_config();
                    return Self::eat(engine);
                }
                0xBD | 0x6D => {
                    // Ctrl+Shift+-: decrease candidate HUD font size
                    engine.config.font_scale = (engine.config.font_scale - 0.10).clamp(0.75, 2.0);
                    engine.save_config();
                    return Self::eat(engine);
                }
                0x56 => {
                    // Ctrl+Shift+V: formal combination hotkey for `oio` (toggle Microphone voice input in all languages)
                    let act = engine.trigger_mic_voice_toggle();
                    return Self::from_action(engine, act);
                }
                0x5A => {
                    // Ctrl+Shift+Z: formal combination hotkey for `ocx` (undo last committed word/sentence in all languages)
                    let act = engine.undo_last_commit();
                    return Self::from_action(engine, act);
                }
                0x44 => {
                    // Ctrl+Shift+D: formal combination hotkey for `orq` (insert localized date in all languages)
                    let act = engine.execute_formal_macro_hotkey("date");
                    return Self::from_action(engine, act);
                }
                0x55 => {
                    // Ctrl+Shift+U: formal combination hotkey for `ouj` (insert current time in all languages)
                    let act = engine.execute_formal_macro_hotkey("time");
                    return Self::from_action(engine, act);
                }
                0x57 => {
                    // Ctrl+Shift+W: formal combination hotkey for `oxq` (insert localized weekday in all languages)
                    let act = engine.execute_formal_macro_hotkey("weekday");
                    return Self::from_action(engine, act);
                }
                0x4D => {
                    // Ctrl+Shift+M: formal combination hotkey for `ors` (insert full timestamp in all languages)
                    let act = engine.execute_formal_macro_hotkey("datetime");
                    return Self::from_action(engine, act);
                }
                _ => {}
            }
        }

        // Ctrl+1..9 and Ctrl+0 while composing: commit candidate AND permanently pin it to #1 in user_custom.tsv!
        if ctrl && !shift && !alt && !win && !engine.input_buf.is_empty() && (0x30..=0x39).contains(&vk) {
            let offset = if vk == 0x30 { 9 } else { (vk - 0x31) as usize };
            let act = engine.pin_candidate_at_page_offset(offset);
            if act != KeyAction::PassThrough {
                return Self::from_action(engine, act);
            }
        }

        if ctrl || win {
            let mut refresh = false;
            if !engine.input_buf.is_empty() {
                engine.clear_composition();
                refresh = true;
            }
            return Self::pass(engine, refresh);
        }

        if matches!(vk, 0x08 | 0x09 | 0x0D | 0x1B | 0x20 | 0x21 | 0x22) || (alt && (0x31..=0x36).contains(&vk)) {
            let act = engine.process_special_key(vk, alt);
            return Self::from_action(engine, act);
        }

        if alt {
            return Self::pass(engine, false);
        }

        let ch = req
            .char_value()
            .filter(|c| c.is_ascii() && !c.is_ascii_control())
            .or_else(|| vk_to_ascii_char(vk, shift, caps));
        if let Some(ch) = ch {
            let act = engine.process_key(ch, shift);
            return Self::from_action(engine, act);
        }

        // Caret-moving keys would corrupt an active composition: swallow them while composing.
        if !engine.input_buf.is_empty() && engine.mode != LangMode::Direct && is_navigation_vk(vk) {
            return Self::eat(engine);
        }
        Self::pass(engine, false)
    }
}
