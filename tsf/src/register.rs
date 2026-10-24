//! COM + TSF registration (`regsvr32 heyu_tsf.dll` / `regsvr32 /u heyu_tsf.dll`).
//! * HKLM\Software\Classes\CLSID\{clsid}\InprocServer32 = <dll path>, ThreadingModel = Apartment
//!   (a 32-bit regsvr32 writes to the WOW6432Node view automatically).
//! * TSF profile via ITfInputProcessorProfileMgr::RegisterProfile (zh-CN, enabled by default).
//! * TIP categories via ITfCategoryMgr::RegisterCategory.

use crate::guids::*;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, MAX_PATH, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::*;
use windows_core::{GUID, HSTRING, PCWSTR};

/// Categories the TIP registers for (no display-attribute provider in phase 1).
pub const TIP_CATEGORIES: [GUID; 7] = [
    GUID_TFCAT_TIP_KEYBOARD,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
    GUID_TFCAT_TIPCAP_SECUREMODE,
    GUID_TFCAT_TIPCAP_COMLESS,
    GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
];

fn clsid_key_path() -> String {
    format!("Software\\Classes\\CLSID\\{}", CLSID_STR)
}

pub fn module_path() -> String {
    let mut buf = [0u16; MAX_PATH as usize * 2];
    let n = unsafe { GetModuleFileNameW(Some(crate::dll_module()), &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

struct ComInit(bool);
impl ComInit {
    fn new() -> Self {
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        ComInit(hr.is_ok() && hr != RPC_E_CHANGED_MODE)
    }
}
impl Drop for ComInit {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn set_string_value(key: HKEY, name: Option<&str>, value: &str) -> windows_core::Result<()> {
    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
    let name_h = name.map(HSTRING::from);
    let name_p = name_h.as_ref().map(|h| PCWSTR(h.as_ptr())).unwrap_or(PCWSTR::null());
    unsafe { RegSetValueExW(key, name_p, None, REG_SZ, Some(bytes)) }.ok()
}

fn create_key(parent: HKEY, path: &str) -> windows_core::Result<HKEY> {
    let mut key = HKEY::default();
    let p = HSTRING::from(path);
    unsafe { RegCreateKeyExW(parent, &p, None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None) }.ok()?;
    Ok(key)
}

fn register_com_class(dll_path: &str) -> windows_core::Result<()> {
    let key = create_key(HKEY_LOCAL_MACHINE, &clsid_key_path())?;
    let r = (|| {
        set_string_value(key, None, DISPLAY_NAME)?;
        let inproc = create_key(key, "InprocServer32")?;
        let r = set_string_value(inproc, None, dll_path).and_then(|_| set_string_value(inproc, Some("ThreadingModel"), "Apartment"));
        unsafe {
            let _ = RegCloseKey(inproc);
        }
        r
    })();
    unsafe {
        let _ = RegCloseKey(key);
    }
    r
}

pub fn register_server() -> windows_core::Result<()> {
    let dll_path = module_path();
    register_com_class(&dll_path)?;

    let _com = ComInit::new();
    unsafe {
        let mgr: ITfInputProcessorProfileMgr = CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;
        let desc: Vec<u16> = DISPLAY_NAME.encode_utf16().collect();
        let icon: Vec<u16> = dll_path.encode_utf16().collect();
        mgr.RegisterProfile(
            &CLSID_HEYU_TEXT_SERVICE,
            LANGID_ZH_CN,
            &GUID_HEYU_PROFILE,
            &desc,
            &icon,
            0,
            HKL::default(),
            0,
            true,
            0,
        )?;

        let cat: ITfCategoryMgr = CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)?;
        for c in TIP_CATEGORIES.iter() {
            cat.RegisterCategory(&CLSID_HEYU_TEXT_SERVICE, c, &CLSID_HEYU_TEXT_SERVICE)?;
        }

        let _ = mgr.ActivateProfile(
            TF_PROFILETYPE_INPUTPROCESSOR,
            LANGID_ZH_CN,
            &CLSID_HEYU_TEXT_SERVICE,
            &GUID_HEYU_PROFILE,
            HKL::default(),
            TF_IPPMF_FORPROCESS | TF_IPPMF_FORSESSION | TF_IPPMF_ENABLEPROFILE,
        );
    }
    Ok(())
}

/// Best effort: removes everything `register_server` created; only reports registry failures.
pub fn unregister_server() -> windows_core::Result<()> {
    {
        let _com = ComInit::new();
        unsafe {
            if let Ok(cat) = CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) {
                for c in TIP_CATEGORIES.iter() {
                    let _ = cat.UnregisterCategory(&CLSID_HEYU_TEXT_SERVICE, c, &CLSID_HEYU_TEXT_SERVICE);
                }
            }
            if let Ok(mgr) = CoCreateInstance::<_, ITfInputProcessorProfileMgr>(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) {
                let _ = mgr.UnregisterProfile(&CLSID_HEYU_TEXT_SERVICE, LANGID_ZH_CN, &GUID_HEYU_PROFILE, 0);
            }
        }
    }
    let p = HSTRING::from(clsid_key_path());
    let err = unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, &p) };
    if err == ERROR_SUCCESS || err == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        err.ok()
    }
}
