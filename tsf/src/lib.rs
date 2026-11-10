//! HEYU (鹤羽输入法) - thin TSF text-service DLL (`heyu_tsf.dll`)
//! ================================================================
//! Loaded by Windows into *every* process that has a text input context, so it is deliberately
//! tiny: it never loads dictionaries, never spawns threads, never panics across FFI (every exported
//! function and every COM method runs inside `catch_unwind`), and never blocks a key for more than
//! [`IPC_TIMEOUT`] - if `heyu-ime.exe` is not reachable, keys are simply passed through.
//!
//! Exports: `DllGetClassObject`, `DllCanUnloadNow`, `DllRegisterServer`, `DllUnregisterServer`.

#![allow(non_snake_case)]

mod edit;
pub mod guids;
mod register;
mod tip;

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;
use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, E_UNEXPECTED, HMODULE, S_FALSE, S_OK};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::{GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};
use windows_core::{implement, IUnknown, Interface, BOOL, GUID, HRESULT, PCWSTR};

/// Per-request budget for the pipe round trip (key events must never stall the app).
pub(crate) const IPC_TIMEOUT: Duration = Duration::from_millis(40);

static OBJECT_COUNT: AtomicIsize = AtomicIsize::new(0);
static LOCK_COUNT: AtomicIsize = AtomicIsize::new(0);

/// Held by every COM object this DLL hands out; `DllCanUnloadNow` only says yes when none are alive.
pub(crate) struct DllRef;
impl DllRef {
    pub(crate) fn new() -> Self {
        OBJECT_COUNT.fetch_add(1, Ordering::SeqCst);
        DllRef
    }
}
impl Drop for DllRef {
    fn drop(&mut self) {
        OBJECT_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Run `f`, converting a panic into `fallback` (a panic must never unwind into the host process).
pub(crate) fn guard<R>(fallback: R, f: impl FnOnce() -> R) -> R {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

pub(crate) fn guard_hr(f: impl FnOnce() -> windows_core::Result<()>) -> windows_core::Result<()> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err(E_UNEXPECTED.into()))
}

static MODULE_ANCHOR: u8 = 0;

/// HMODULE of this DLL (not of the host exe).
pub(crate) fn dll_module() -> HMODULE {
    let mut h = HMODULE::default();
    unsafe {
        let _ = GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(&MODULE_ANCHOR as *const u8 as *const u16),
            &mut h,
        );
    }
    h
}

// ============================================================================
// CLASS FACTORY
// ============================================================================
#[implement(IClassFactory)]
struct ClassFactory {
    _ref: DllRef,
}

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(&self, punkouter: windows_core::Ref<IUnknown>, riid: *const GUID, ppvobject: *mut *mut c_void) -> windows_core::Result<()> {
        guard_hr(|| unsafe {
            if ppvobject.is_null() || riid.is_null() {
                return Err(E_POINTER.into());
            }
            *ppvobject = std::ptr::null_mut();
            if !punkouter.is_null() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }
            let unk: IUnknown = tip::TextService::new().into();
            unk.query(riid, ppvobject).ok()
        })
    }

    fn LockServer(&self, flock: BOOL) -> windows_core::Result<()> {
        guard_hr(|| {
            if flock.as_bool() {
                LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
            } else {
                LOCK_COUNT.fetch_sub(1, Ordering::SeqCst);
            }
            Ok(())
        })
    }
}

// ============================================================================
// EXPORTS
// ============================================================================
#[no_mangle]
pub extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    guard(E_UNEXPECTED, || unsafe {
        if ppv.is_null() || rclsid.is_null() || riid.is_null() {
            return E_POINTER;
        }
        *ppv = std::ptr::null_mut();
        if *rclsid != guids::CLSID_HEYU_TEXT_SERVICE {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = ClassFactory { _ref: DllRef::new() }.into();
        factory.query(riid, ppv)
    })
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    guard(S_FALSE, || {
        if OBJECT_COUNT.load(Ordering::SeqCst) <= 0 && LOCK_COUNT.load(Ordering::SeqCst) <= 0 {
            S_OK
        } else {
            S_FALSE
        }
    })
}

#[no_mangle]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    guard(E_UNEXPECTED, || match register::register_server() {
        Ok(()) => S_OK,
        Err(e) => {
            let _ = register::unregister_server();
            let hr = e.code();
            if hr.is_ok() {
                E_FAIL
            } else {
                hr
            }
        }
    })
}

#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    guard(E_UNEXPECTED, || match register::unregister_server() {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    })
}
