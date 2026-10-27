//! HEYU (鹤羽输入法) - Core Library & C-ABI / TSF Bridge
//! ======================================================
//! * `engine`  - HeyuEngine state machine (dictionaries, candidates)
//! * `hud`     - layered-window candidate HUD / status pill renderer
//! * `win32`   - hand-written Win32 FFI
//! * `ipc`     - versioned binary protocol shared with the TSF DLL (`tsf/`)
//! * `pipe`    - local named-pipe client (DLL side) and server (heyu-ime.exe side)
//! * `session` - key-event state machine shared by the pipe server, the hook fallback and tests
//! * `paths`   - repo vs. installed (`%APPDATA%\HEYU`) data directory resolution

pub mod engine;
pub mod hud;
pub mod ipc;
pub mod paths;
pub mod pipe;
pub mod session;
pub mod vault;
pub mod win32;

pub use engine::{Candidate, EngineConfig, HeyuEngine, KeyAction, LangMode};
