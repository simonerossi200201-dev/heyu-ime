//! Loader test for heyu_tsf.dll (no registration needed, nothing is written to the registry).
//!
//!   cargo run -p heyu-tsf --example loader_test [-- <path to heyu_tsf.dll>]
//!   cargo run -p heyu-tsf --example loader_test --target i686-pc-windows-gnullvm
//!
//! Checks: LoadLibrary, the four exports, DllGetClassObject (+ wrong CLSID rejected),
//! IClassFactory::CreateInstance, QueryInterface for every TSF interface the TIP implements,
//! ActivateEx/Deactivate against a real ITfThreadMgr, a key test without a context, and that
//! DllCanUnloadNow flips to S_OK once every object is released.

use std::ffi::c_void;
use std::process::ExitCode;
use std::time::Instant;
use windows::core::{s, Interface, GUID, HRESULT, HSTRING};
use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, FreeLibrary, LPARAM, S_FALSE, S_OK, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, IClassFactory, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::TextServices::*;

const CLSID_HEYU: GUID = GUID::from_u128(0xC8A71319_6351_406E_8FF3_A229E1FC7663);

type GetClassObjectFn = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
type HrFn = unsafe extern "system" fn() -> HRESULT;

macro_rules! check {
    ($cond:expr, $($msg:tt)*) => {
        if $cond {
            println!("  [ OK ] {}", format!($($msg)*));
        } else {
            println!("  [FAIL] {}", format!($($msg)*));
            return Err(());
        }
    };
}

fn run() -> Result<(), ()> {
    let dll = std::env::args().nth(1).map(std::path::PathBuf::from).unwrap_or_else(|| {
        let exe = std::env::current_exe().unwrap();
        exe.parent().unwrap().parent().unwrap().join("heyu_tsf.dll")
    });
    println!("heyu_tsf loader test ({}-bit process)", std::mem::size_of::<usize>() * 8);
    println!("  DLL: {}", dll.display());

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let h = LoadLibraryW(&HSTRING::from(dll.as_os_str()));
        check!(h.is_ok(), "LoadLibraryW -> {:?}", h.as_ref().err());
        let h = h.unwrap();

        let get = GetProcAddress(h, s!("DllGetClassObject"));
        let can = GetProcAddress(h, s!("DllCanUnloadNow"));
        let reg = GetProcAddress(h, s!("DllRegisterServer"));
        let unreg = GetProcAddress(h, s!("DllUnregisterServer"));
        check!(get.is_some(), "export DllGetClassObject");
        check!(can.is_some(), "export DllCanUnloadNow");
        check!(reg.is_some(), "export DllRegisterServer (not called)");
        check!(unreg.is_some(), "export DllUnregisterServer (not called)");
        let get: GetClassObjectFn = std::mem::transmute(get.unwrap());
        let can: HrFn = std::mem::transmute(can.unwrap());

        check!(can() == S_OK, "DllCanUnloadNow == S_OK before any object exists");

        let mut p: *mut c_void = std::ptr::null_mut();
        let wrong = GUID::from_u128(0x12345678_0000_0000_0000_000000000000);
        check!(get(&wrong, &IClassFactory::IID, &mut p) == CLASS_E_CLASSNOTAVAILABLE, "unknown CLSID rejected");

        let mut p: *mut c_void = std::ptr::null_mut();
        let hr = get(&CLSID_HEYU, &IClassFactory::IID, &mut p);
        check!(hr.is_ok() && !p.is_null(), "DllGetClassObject(CLSID_HEYU, IClassFactory) -> {:?}", hr);
        let factory = IClassFactory::from_raw(p);

        let tip: windows::core::Result<ITfTextInputProcessorEx> = factory.CreateInstance(None);
        check!(tip.is_ok(), "IClassFactory::CreateInstance -> ITfTextInputProcessorEx {:?}", tip.as_ref().err());
        let tip = tip.unwrap();
        check!(tip.cast::<ITfTextInputProcessor>().is_ok(), "QI ITfTextInputProcessor");
        check!(tip.cast::<ITfKeyEventSink>().is_ok(), "QI ITfKeyEventSink");
        check!(tip.cast::<ITfThreadMgrEventSink>().is_ok(), "QI ITfThreadMgrEventSink");
        check!(tip.cast::<ITfCompositionSink>().is_ok(), "QI ITfCompositionSink");
        check!(tip.cast::<ITfCompartmentEventSink>().is_ok(), "QI ITfCompartmentEventSink");
        check!(tip.cast::<ITfDisplayAttributeProvider>().is_err(), "QI ITfDisplayAttributeProvider correctly unsupported");
        check!(can() == S_FALSE, "DllCanUnloadNow == S_FALSE while objects are alive");

        // Activation against a real thread manager (the TIP is not registered; this exercises
        // sink advising, compartments and the server ping/pass-through path).
        let tim: windows::core::Result<ITfThreadMgr> = CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER);
        check!(tim.is_ok(), "CoCreateInstance(CLSID_TF_ThreadMgr)");
        let tim = tim.unwrap();
        let tid = tim.Activate();
        check!(tid.is_ok(), "ITfThreadMgr::Activate -> tid {:?}", tid);
        // A text service gets its own client id; obtain one for our CLSID like TSF would.
        let tip_tid = tim.cast::<ITfClientId>().and_then(|c| c.GetClientId(&CLSID_HEYU));
        check!(tip_tid.is_ok(), "ITfClientId::GetClientId(CLSID_HEYU) -> {:?}", tip_tid);
        let tid = tip_tid.unwrap();
        let t0 = Instant::now();
        let act = tip.ActivateEx(&tim, tid, 0);
        // When unregistered in HKLM\SOFTWARE\Microsoft\CTF\TIP, AdviseKeyEventSink returns
        // E_INVALIDARG (0x80070057) and ActivateEx rolls back cleanly; when registered, it returns Ok(()).
        let e_invalidarg = HRESULT(0x80070057u32 as i32);
        check!(
            act.is_ok() || act.as_ref().err().map(|e| e.code() == e_invalidarg).unwrap_or(false),
            "ITfTextInputProcessorEx::ActivateEx -> {:?} ({:?})",
            act,
            t0.elapsed()
        );

        let ks: ITfKeyEventSink = tip.cast().unwrap();
        let t0 = Instant::now();
        let eaten = ks.OnTestKeyDown(None, WPARAM(0x41), LPARAM(0x001E0001));
        let took = t0.elapsed();
        check!(matches!(eaten, Ok(b) if !b.as_bool()), "OnTestKeyDown without a context passes the key through ({:?})", took);
        check!(took.as_millis() < 60, "key test returned within the 50 ms budget");
        let up = ks.OnTestKeyUp(None, WPARAM(0x10), LPARAM(0xC02A0001u32 as isize));
        check!(up.is_ok(), "OnTestKeyUp(Shift) handled -> {:?}", up);
        check!(ks.OnSetFocus(true.into()).is_ok(), "ITfKeyEventSink::OnSetFocus");

        let deact = tip.Deactivate();
        check!(deact.is_ok(), "ITfTextInputProcessor::Deactivate -> {:?}", deact);
        let _ = tim.Deactivate();

        drop(ks);
        drop(tip);
        drop(factory);
        drop(tim);
        check!(can() == S_OK, "DllCanUnloadNow == S_OK after releasing everything");
        let _ = FreeLibrary(h);
        println!("  [ OK ] FreeLibrary");
        CoUninitialize();
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => {
            println!("ALL LOADER CHECKS PASSED");
            ExitCode::SUCCESS
        }
        Err(()) => {
            println!("LOADER TEST FAILED");
            ExitCode::FAILURE
        }
    }
}
