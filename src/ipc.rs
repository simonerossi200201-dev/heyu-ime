//! HEYU (鹤羽输入法) - Shared, versioned IPC protocol (TSF client DLL <-> heyu-ime.exe server)
//! ===========================================================================================
//! Transport: a local message-mode named pipe `\\.\pipe\heyu-ime-<session id>` (see [`crate::pipe`]).
//! Every request gets exactly one response. All integers are little-endian.
//!
//! Request (fixed 48 bytes, protocol v1):
//! ```text
//!  off size field
//!   0   4   magic  = b"HYQ1"
//!   4   2   version = 1
//!   6   1   kind (RequestKind)
//!   7   1   flags  (bit0 key-up, bit1 caret rect valid)
//!   8   2   vk (virtual-key code; VK_SHIFT is normalised to VK_LSHIFT/VK_RSHIFT by the client)
//!  10   2   scan code
//!  12   4   unicode scalar of the key (0 = unknown; server falls back to its own vk mapping)
//!  16   2   modifiers (MOD_* bits)
//!  18   2   reserved
//!  20  16   caret rect left, top, right, bottom (i32, screen coordinates)
//!  36   4   client process id
//!  40   4   arg (kind-specific, e.g. SetAsciiMode value)
//!  44   4   reserved
//! ```
//! Response (variable, protocol v1):
//! ```text
//!   0   4   magic = b"HYR1"
//!   4   2   version = 1
//!   6   1   flags (RESP_* bits)
//!   7   1   reserved
//!   8   4   preedit caret (UTF-16 code units from start of preedit)
//!  12   2   commit_text length in bytes (UTF-8)
//!  14   2   preedit_text length in bytes (UTF-8)
//!  16   n   commit_text bytes, followed by preedit_text bytes
//! ```
//! `commit_text` may start with one or more U+0008 (backspace) characters, meaning
//! "delete that many characters before the caret first" (used by e.g. `??` -> `¿`).

pub const PROTOCOL_VERSION: u16 = 1;
pub const REQUEST_MAGIC: [u8; 4] = *b"HYQ1";
pub const RESPONSE_MAGIC: [u8; 4] = *b"HYR1";
pub const REQUEST_SIZE: usize = 48;
pub const RESPONSE_HEADER_SIZE: usize = 16;
/// Hard cap for a single pipe message (both directions).
pub const MAX_MESSAGE_SIZE: usize = 8192;
/// Max bytes of commit / preedit text carried in one response.
pub const MAX_TEXT_BYTES: usize = 3000;

pub const MOD_SHIFT: u16 = 1 << 0;
pub const MOD_CTRL: u16 = 1 << 1;
pub const MOD_ALT: u16 = 1 << 2;
pub const MOD_WIN: u16 = 1 << 3;
pub const MOD_CAPSLOCK: u16 = 1 << 4;
pub const MOD_LSHIFT: u16 = 1 << 5;
pub const MOD_RSHIFT: u16 = 1 << 6;
pub const MOD_EXTENDED: u16 = 1 << 7;

pub const REQ_FLAG_KEY_UP: u8 = 1 << 0;
pub const REQ_FLAG_CARET_VALID: u8 = 1 << 1;

/// Key was consumed by the IME (do not pass it to the application).
pub const RESP_EATEN: u8 = 1 << 0;
/// Engine is in direct/ASCII pass-through mode ("英").
pub const RESP_ASCII_MODE: u8 = 1 << 1;
/// Candidate HUD is visible.
pub const RESP_HUD_VISIBLE: u8 = 1 << 2;
/// Server could not process the request (timeout/busy) - client must pass the key through.
pub const RESP_UNAVAILABLE: u8 = 1 << 3;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    /// Key down / key up (see `REQ_FLAG_KEY_UP`).
    Key = 1,
    /// Caret / composition rectangle changed (no key).
    Caret = 2,
    /// A document gained focus in the client.
    FocusIn = 3,
    /// Focus lost / composition terminated by the app: clear composition + hide HUD.
    Reset = 4,
    /// Liveness + status query.
    Ping = 5,
    /// Set ASCII mode: arg 0 = Chinese, 1 = direct/ASCII, 2 = toggle.
    SetAsciiMode = 6,
}

impl RequestKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::Key,
            2 => Self::Caret,
            3 => Self::FocusIn,
            4 => Self::Reset,
            5 => Self::Ping,
            6 => Self::SetAsciiMode,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaretRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub kind: RequestKind,
    pub key_up: bool,
    pub vk: u16,
    pub scan: u16,
    pub ch: u32,
    pub modifiers: u16,
    pub caret: Option<CaretRect>,
    pub pid: u32,
    pub arg: u32,
}

impl Request {
    pub fn new(kind: RequestKind) -> Self {
        Self { kind, key_up: false, vk: 0, scan: 0, ch: 0, modifiers: 0, caret: None, pid: 0, arg: 0 }
    }

    pub fn key(vk: u16, ch: char, modifiers: u16, key_up: bool) -> Self {
        Self { vk, ch: ch as u32, modifiers, key_up, ..Self::new(RequestKind::Key) }
    }

    pub fn char_value(&self) -> Option<char> {
        if self.ch == 0 {
            None
        } else {
            char::from_u32(self.ch)
        }
    }

    pub fn encode(&self) -> [u8; REQUEST_SIZE] {
        let mut b = [0u8; REQUEST_SIZE];
        b[0..4].copy_from_slice(&REQUEST_MAGIC);
        b[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        b[6] = self.kind as u8;
        let mut flags = 0u8;
        if self.key_up {
            flags |= REQ_FLAG_KEY_UP;
        }
        if self.caret.is_some() {
            flags |= REQ_FLAG_CARET_VALID;
        }
        b[7] = flags;
        b[8..10].copy_from_slice(&self.vk.to_le_bytes());
        b[10..12].copy_from_slice(&self.scan.to_le_bytes());
        b[12..16].copy_from_slice(&self.ch.to_le_bytes());
        b[16..18].copy_from_slice(&self.modifiers.to_le_bytes());
        let r = self.caret.unwrap_or_default();
        b[20..24].copy_from_slice(&r.left.to_le_bytes());
        b[24..28].copy_from_slice(&r.top.to_le_bytes());
        b[28..32].copy_from_slice(&r.right.to_le_bytes());
        b[32..36].copy_from_slice(&r.bottom.to_le_bytes());
        b[36..40].copy_from_slice(&self.pid.to_le_bytes());
        b[40..44].copy_from_slice(&self.arg.to_le_bytes());
        b
    }

    pub fn decode(b: &[u8]) -> Result<Self, ProtocolError> {
        if b.len() < REQUEST_SIZE {
            return Err(ProtocolError::Truncated);
        }
        if b[0..4] != REQUEST_MAGIC {
            return Err(ProtocolError::BadMagic);
        }
        let ver = u16le(b, 4);
        if ver != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion(ver));
        }
        let kind = RequestKind::from_u8(b[6]).ok_or(ProtocolError::BadKind(b[6]))?;
        let flags = b[7];
        let caret = if flags & REQ_FLAG_CARET_VALID != 0 {
            Some(CaretRect { left: i32le(b, 20), top: i32le(b, 24), right: i32le(b, 28), bottom: i32le(b, 32) })
        } else {
            None
        };
        Ok(Self {
            kind,
            key_up: flags & REQ_FLAG_KEY_UP != 0,
            vk: u16le(b, 8),
            scan: u16le(b, 10),
            ch: u32le(b, 12),
            modifiers: u16le(b, 16),
            caret,
            pid: u32le(b, 36),
            arg: u32le(b, 40),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Response {
    pub eaten: bool,
    pub ascii_mode: bool,
    pub hud_visible: bool,
    pub unavailable: bool,
    pub commit_text: String,
    pub preedit_text: String,
    /// Caret position inside the preedit, in UTF-16 code units.
    pub preedit_caret: u32,
}

impl Response {
    /// Response used when the server cannot answer: never eat the key.
    pub fn unavailable() -> Self {
        Self { unavailable: true, ..Default::default() }
    }

    pub fn encode(&self) -> Vec<u8> {
        let commit = truncate_utf8(&self.commit_text, MAX_TEXT_BYTES);
        let preedit = truncate_utf8(&self.preedit_text, MAX_TEXT_BYTES);
        let mut b = Vec::with_capacity(RESPONSE_HEADER_SIZE + commit.len() + preedit.len());
        b.extend_from_slice(&RESPONSE_MAGIC);
        b.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        let mut flags = 0u8;
        if self.eaten {
            flags |= RESP_EATEN;
        }
        if self.ascii_mode {
            flags |= RESP_ASCII_MODE;
        }
        if self.hud_visible {
            flags |= RESP_HUD_VISIBLE;
        }
        if self.unavailable {
            flags |= RESP_UNAVAILABLE;
        }
        b.push(flags);
        b.push(0);
        b.extend_from_slice(&self.preedit_caret.to_le_bytes());
        b.extend_from_slice(&(commit.len() as u16).to_le_bytes());
        b.extend_from_slice(&(preedit.len() as u16).to_le_bytes());
        b.extend_from_slice(commit.as_bytes());
        b.extend_from_slice(preedit.as_bytes());
        b
    }

    pub fn decode(b: &[u8]) -> Result<Self, ProtocolError> {
        if b.len() < RESPONSE_HEADER_SIZE {
            return Err(ProtocolError::Truncated);
        }
        if b[0..4] != RESPONSE_MAGIC {
            return Err(ProtocolError::BadMagic);
        }
        let ver = u16le(b, 4);
        if ver != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion(ver));
        }
        let flags = b[6];
        let preedit_caret = u32le(b, 8);
        let clen = u16le(b, 12) as usize;
        let plen = u16le(b, 14) as usize;
        if b.len() < RESPONSE_HEADER_SIZE + clen + plen {
            return Err(ProtocolError::Truncated);
        }
        let c0 = RESPONSE_HEADER_SIZE;
        let commit_text = std::str::from_utf8(&b[c0..c0 + clen]).map_err(|_| ProtocolError::BadUtf8)?.to_string();
        let preedit_text = std::str::from_utf8(&b[c0 + clen..c0 + clen + plen])
            .map_err(|_| ProtocolError::BadUtf8)?
            .to_string();
        Ok(Self {
            eaten: flags & RESP_EATEN != 0,
            ascii_mode: flags & RESP_ASCII_MODE != 0,
            hud_visible: flags & RESP_HUD_VISIBLE != 0,
            unavailable: flags & RESP_UNAVAILABLE != 0,
            commit_text,
            preedit_text,
            preedit_caret,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    Truncated,
    BadMagic,
    UnsupportedVersion(u16),
    BadKind(u8),
    BadUtf8,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for ProtocolError {}

fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn i32le(b: &[u8], o: usize) -> i32 {
    u32le(b, o) as i32
}

/// Map a virtual key + Shift/CapsLock to the ASCII character the engine expects (US layout).
/// Shared by the hook daemon, the pipe server and the TSF client.
pub fn vk_to_ascii_char(vk: u16, shift: bool, caps: bool) -> Option<char> {
    if (0x41..=0x5A).contains(&vk) {
        let is_upper = shift ^ caps;
        let base = if is_upper { b'A' } else { b'a' };
        return Some((base + (vk as u8 - 0x41)) as char);
    }
    if (0x30..=0x39).contains(&vk) {
        if !shift {
            return Some((b'0' + (vk as u8 - 0x30)) as char);
        } else {
            return match vk {
                0x30 => Some(')'),
                0x31 => Some('!'),
                0x32 => Some('@'),
                0x33 => Some('#'),
                0x34 => Some('$'),
                0x35 => Some('%'),
                0x36 => Some('^'),
                0x37 => Some('&'),
                0x38 => Some('*'),
                0x39 => Some('('),
                _ => None,
            };
        }
    }
    if (0x60..=0x69).contains(&vk) && !shift {
        return Some((b'0' + (vk as u8 - 0x60)) as char);
    }
    match (vk, shift) {
        (0xBA, false) => Some(';'),
        (0xBA, true) => Some(':'),
        (0xBB, false) => Some('='),
        (0xBB, true) => Some('+'),
        (0xBC, false) => Some(','),
        (0xBC, true) => Some('<'),
        (0xBD, false) => Some('-'),
        (0xBD, true) => Some('_'),
        (0xBE, false) => Some('.'),
        (0xBE, true) => Some('>'),
        (0xBF, false) => Some('/'),
        (0xBF, true) => Some('?'),
        (0xC0, false) => Some('`'),
        (0xC0, true) => Some('~'),
        (0xDB, false) => Some('['),
        (0xDB, true) => Some('{'),
        (0xDD, false) => Some(']'),
        (0xDD, true) => Some('}'),
        (0xDC, false) => Some('\\'),
        (0xDC, true) => Some('|'),
        (0xDE, false) => Some('\''),
        (0xDE, true) => Some('"'),
        _ => None,
    }
}

/// Pipe name for a given Windows session id.
pub fn pipe_name_for_session(session_id: u32) -> String {
    format!(r"\\.\pipe\heyu-ime-{}", session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip_all_fields() {
        let req = Request {
            kind: RequestKind::Key,
            key_up: true,
            vk: 0x48,
            scan: 0x23,
            ch: 'h' as u32,
            modifiers: MOD_SHIFT | MOD_LSHIFT | MOD_CAPSLOCK,
            caret: Some(CaretRect { left: -1920, top: 10, right: -1900, bottom: 34 }),
            pid: 4242,
            arg: 7,
        };
        let bytes = req.encode();
        assert_eq!(bytes.len(), REQUEST_SIZE);
        assert_eq!(&bytes[0..4], b"HYQ1");
        assert_eq!(Request::decode(&bytes).unwrap(), req);
    }

    #[test]
    fn request_without_caret_and_each_kind() {
        for k in 1u8..=6 {
            let kind = RequestKind::from_u8(k).unwrap();
            let req = Request::new(kind);
            let back = Request::decode(&req.encode()).unwrap();
            assert_eq!(back.kind, kind);
            assert_eq!(back.caret, None);
            assert!(!back.key_up);
        }
    }

    #[test]
    fn request_rejects_garbage() {
        let req = Request::key(0x41, 'a', 0, false);
        let mut b = req.encode();
        assert_eq!(Request::decode(&b[..20]), Err(ProtocolError::Truncated));
        b[6] = 99;
        assert_eq!(Request::decode(&b), Err(ProtocolError::BadKind(99)));
        let mut b = req.encode();
        b[4] = 2;
        assert_eq!(Request::decode(&b), Err(ProtocolError::UnsupportedVersion(2)));
        let mut b = req.encode();
        b[0] = b'X';
        assert_eq!(Request::decode(&b), Err(ProtocolError::BadMagic));
    }

    #[test]
    fn response_roundtrip_unicode() {
        let resp = Response {
            eaten: true,
            ascii_mode: false,
            hud_visible: true,
            unavailable: false,
            commit_text: "\u{8}¿鹤羽 perché 😀".to_string(),
            preedit_text: "hedn".to_string(),
            preedit_caret: 4,
        };
        let b = resp.encode();
        assert_eq!(&b[0..4], b"HYR1");
        assert_eq!(Response::decode(&b).unwrap(), resp);
    }

    #[test]
    fn response_flags_and_empty_strings() {
        let resp = Response { ascii_mode: true, ..Default::default() };
        let back = Response::decode(&resp.encode()).unwrap();
        assert!(back.ascii_mode && !back.eaten && !back.hud_visible && !back.unavailable);
        assert!(back.commit_text.is_empty() && back.preedit_text.is_empty());
        assert!(Response::decode(&Response::unavailable().encode()).unwrap().unavailable);
    }

    #[test]
    fn response_truncation_is_char_safe_and_detected() {
        let long = "鹤".repeat(2000); // 6000 bytes > MAX_TEXT_BYTES
        let resp = Response { commit_text: long, ..Default::default() };
        let b = resp.encode();
        assert!(b.len() <= MAX_MESSAGE_SIZE);
        let back = Response::decode(&b).unwrap();
        assert_eq!(back.commit_text.chars().count(), MAX_TEXT_BYTES / 3);
        assert_eq!(Response::decode(&b[..b.len() - 1]), Err(ProtocolError::Truncated));
        let mut bad = Response { commit_text: "ab".into(), ..Default::default() }.encode();
        bad[RESPONSE_HEADER_SIZE] = 0xFF;
        assert_eq!(Response::decode(&bad), Err(ProtocolError::BadUtf8));
    }

    #[test]
    fn vk_mapping_matches_us_layout() {
        assert_eq!(vk_to_ascii_char(0x41, false, false), Some('a'));
        assert_eq!(vk_to_ascii_char(0x41, true, false), Some('A'));
        assert_eq!(vk_to_ascii_char(0x41, true, true), Some('a'));
        assert_eq!(vk_to_ascii_char(0xBA, false, false), Some(';'));
        assert_eq!(vk_to_ascii_char(0xBF, true, false), Some('?'));
        assert_eq!(vk_to_ascii_char(0x70, false, false), None);
        assert_eq!(pipe_name_for_session(3), r"\\.\pipe\heyu-ime-3");
    }
}
