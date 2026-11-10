//! HEYU (鹤羽输入法) TSF identity constants - the ONE place these GUIDs are defined.
//! (install.ps1 / uninstall.ps1 / dev_register_check.ps1 parse the `*_STR` lines of this file.)
//! Generated once on 2026-10-19; never change them, or installed profiles become orphaned.

use windows_core::GUID;

/// COM class id of the text service (InprocServer32 = heyu_tsf.dll).
pub const CLSID_HEYU_TEXT_SERVICE: GUID = GUID::from_u128(0xC8A71319_6351_406E_8FF3_A229E1FC7663);
pub const CLSID_STR: &str = "{C8A71319-6351-406E-8FF3-A229E1FC7663}";

/// TSF language profile id (one profile, zh-CN).
pub const GUID_HEYU_PROFILE: GUID = GUID::from_u128(0x92AF8AF0_4D38_489E_8C63_E263D15DFEE4);
pub const PROFILE_STR: &str = "{92AF8AF0-4D38-489E-8C63-E263D15DFEE4}";

/// zh-CN
pub const LANGID_ZH_CN: u16 = 0x0804;

pub const DISPLAY_NAME: &str = "鹤羽音形";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_forms_match_guids() {
        assert_eq!(format!("{{{:?}}}", CLSID_HEYU_TEXT_SERVICE), CLSID_STR);
        assert_eq!(format!("{{{:?}}}", GUID_HEYU_PROFILE), PROFILE_STR);
        assert_ne!(CLSID_HEYU_TEXT_SERVICE, GUID_HEYU_PROFILE);
    }
}
