//! OS-level UIAutomation (`IUIAutomation`, `IUIAutomationTextPattern`, `IUIAutomationTextPattern2`,
//! `IUIAutomationTextRange`) engine for HEYU (`heyu-voice.exe`).
//!
//! Provides exact OS-level document caret rectangle queries and persistent `IUIAutomationTextRange`
//! anchoring for Dual-Cursor ("independent") voice transcription across Windows applications
//! without pixel guessing or simulated mouse clicks.
#![allow(non_snake_case, dead_code)]

use crate::sys::{BOOL, HWND, POINT};
use std::ffi::c_void;

pub type HRESULT = i32;
type ULONG = u32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

// CLSID_CUIAutomation: {ff48dba4-60ef-4201-aa87-54103eef594e}
const CLSID_CUIAUTOMATION: GUID = GUID {
    data1: 0xff48dba4,
    data2: 0x60ef,
    data3: 0x4201,
    data4: [0xaa, 0x87, 0x54, 0x10, 0x3e, 0xef, 0x59, 0x4e],
};

// IID_IUIAutomation: {30cbe57d-d9d0-452a-ab13-7ac5ac4825ee}
const IID_IUIAUTOMATION: GUID = GUID {
    data1: 0x30cbe57d,
    data2: 0xd9d0,
    data3: 0x452a,
    data4: [0xab, 0x13, 0x7a, 0xc5, 0xac, 0x48, 0x25, 0xee],
};

// IID_IUIAutomationTextPattern: {32eba289-3583-42c9-9c59-3b6d9a1e9b6a}
const IID_IUIAUTOMATION_TEXTPATTERN: GUID = GUID {
    data1: 0x32eba289,
    data2: 0x3583,
    data3: 0x42c9,
    data4: [0x9c, 0x59, 0x3b, 0x6d, 0x9a, 0x1e, 0x9b, 0x6a],
};

// IID_IUIAutomationTextPattern2: {506a921a-fcc9-409f-b23b-37eb74106872}
const IID_IUIAUTOMATION_TEXTPATTERN2: GUID = GUID {
    data1: 0x506a921a,
    data2: 0xfcc9,
    data3: 0x409f,
    data4: [0xb2, 0x3b, 0x37, 0xeb, 0x74, 0x10, 0x68, 0x72],
};

const UIA_TEXT_PATTERN_ID: i32 = 10014;
const UIA_TEXT_PATTERN2_ID: i32 = 10024;
const CLSCTX_INPROC_SERVER: u32 = 0x1;
const COINIT_APARTMENTTHREADED: u32 = 0x2;

const TEXT_UNIT_CHARACTER: i32 = 0;
const ENDPOINT_START: i32 = 0;
const ENDPOINT_END: i32 = 1;

pub type SAFEARRAY = c_void;

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(pvReserved: *mut c_void, dwCoInit: u32) -> HRESULT;
    fn CoCreateInstance(
        rclsid: *const GUID,
        pUnkOuter: *mut c_void,
        dwClsContext: u32,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT;
}

#[link(name = "oleaut32")]
extern "system" {
    fn SafeArrayGetLBound(psa: *mut SAFEARRAY, nDim: u32, plLbound: *mut i32) -> HRESULT;
    fn SafeArrayGetUBound(psa: *mut SAFEARRAY, nDim: u32, plUbound: *mut i32) -> HRESULT;
    fn SafeArrayAccessData(psa: *mut SAFEARRAY, ppvData: *mut *mut c_void) -> HRESULT;
    fn SafeArrayUnaccessData(psa: *mut SAFEARRAY) -> HRESULT;
    fn SafeArrayDestroy(psa: *mut SAFEARRAY) -> HRESULT;
}

#[repr(C)]
pub struct IUIAutomation {
    pub lpVtbl: *const IUIAutomationVtbl,
}

#[repr(C)]
pub struct IUIAutomationVtbl {
    pub QueryInterface: unsafe extern "system" fn(*mut IUIAutomation, *const GUID, *mut *mut c_void) -> HRESULT,
    pub AddRef: unsafe extern "system" fn(*mut IUIAutomation) -> ULONG,
    pub Release: unsafe extern "system" fn(*mut IUIAutomation) -> ULONG,
    pub CompareElements: unsafe extern "system" fn(*mut IUIAutomation, *mut IUIAutomationElement, *mut IUIAutomationElement, *mut BOOL) -> HRESULT,
    pub CompareRuntimeIds: unsafe extern "system" fn(*mut IUIAutomation, *mut SAFEARRAY, *mut SAFEARRAY, *mut BOOL) -> HRESULT,
    pub GetRootElement: unsafe extern "system" fn(*mut IUIAutomation, *mut *mut IUIAutomationElement) -> HRESULT,
    pub ElementFromHandle: unsafe extern "system" fn(*mut IUIAutomation, HWND, *mut *mut IUIAutomationElement) -> HRESULT,
    pub ElementFromPoint: unsafe extern "system" fn(*mut IUIAutomation, POINT, *mut *mut IUIAutomationElement) -> HRESULT,
    pub GetFocusedElement: unsafe extern "system" fn(*mut IUIAutomation, *mut *mut IUIAutomationElement) -> HRESULT,
}

#[repr(C)]
pub struct IUIAutomationElement {
    pub lpVtbl: *const IUIAutomationElementVtbl,
}

#[repr(C)]
pub struct IUIAutomationElementVtbl {
    pub QueryInterface: unsafe extern "system" fn(*mut IUIAutomationElement, *const GUID, *mut *mut c_void) -> HRESULT,
    pub AddRef: unsafe extern "system" fn(*mut IUIAutomationElement) -> ULONG,
    pub Release: unsafe extern "system" fn(*mut IUIAutomationElement) -> ULONG,
    pub SetFocus: unsafe extern "system" fn(*mut IUIAutomationElement) -> HRESULT,
    pub GetRuntimeId: unsafe extern "system" fn(*mut IUIAutomationElement, *mut *mut SAFEARRAY) -> HRESULT,
    pub FindFirst: unsafe extern "system" fn(*mut IUIAutomationElement, i32, *mut c_void, *mut *mut IUIAutomationElement) -> HRESULT,
    pub FindAll: unsafe extern "system" fn(*mut IUIAutomationElement, i32, *mut c_void, *mut *mut c_void) -> HRESULT,
    pub FindFirstBuildCache: unsafe extern "system" fn(*mut IUIAutomationElement, i32, *mut c_void, *mut c_void, *mut *mut IUIAutomationElement) -> HRESULT,
    pub FindAllBuildCache: unsafe extern "system" fn(*mut IUIAutomationElement, i32, *mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT,
    pub BuildUpdatedCache: unsafe extern "system" fn(*mut IUIAutomationElement, *mut c_void, *mut *mut IUIAutomationElement) -> HRESULT,
    pub GetCurrentPropertyValue: unsafe extern "system" fn(*mut IUIAutomationElement, i32, *mut c_void) -> HRESULT,
    pub GetCurrentPropertyValueEx: unsafe extern "system" fn(*mut IUIAutomationElement, i32, BOOL, *mut c_void) -> HRESULT,
    pub GetCachedPropertyValue: unsafe extern "system" fn(*mut IUIAutomationElement, i32, *mut c_void) -> HRESULT,
    pub GetCachedPropertyValueEx: unsafe extern "system" fn(*mut IUIAutomationElement, i32, BOOL, *mut c_void) -> HRESULT,
    pub GetCurrentPatternAs: unsafe extern "system" fn(*mut IUIAutomationElement, i32, *const GUID, *mut *mut c_void) -> HRESULT,
    pub GetCachedPatternAs: unsafe extern "system" fn(*mut IUIAutomationElement, i32, *const GUID, *mut *mut c_void) -> HRESULT,
    pub GetCurrentPattern: unsafe extern "system" fn(*mut IUIAutomationElement, i32, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
pub struct IUIAutomationTextPattern {
    pub lpVtbl: *const IUIAutomationTextPatternVtbl,
}

#[repr(C)]
pub struct IUIAutomationTextPatternVtbl {
    pub QueryInterface: unsafe extern "system" fn(*mut IUIAutomationTextPattern, *const GUID, *mut *mut c_void) -> HRESULT,
    pub AddRef: unsafe extern "system" fn(*mut IUIAutomationTextPattern) -> ULONG,
    pub Release: unsafe extern "system" fn(*mut IUIAutomationTextPattern) -> ULONG,
    pub RangeFromPoint: unsafe extern "system" fn(*mut IUIAutomationTextPattern, POINT, *mut *mut IUIAutomationTextRange) -> HRESULT,
    pub RangeFromChild: unsafe extern "system" fn(*mut IUIAutomationTextPattern, *mut IUIAutomationElement, *mut *mut IUIAutomationTextRange) -> HRESULT,
    pub GetSelection: unsafe extern "system" fn(*mut IUIAutomationTextPattern, *mut *mut IUIAutomationTextRangeArray) -> HRESULT,
    pub GetVisibleRanges: unsafe extern "system" fn(*mut IUIAutomationTextPattern, *mut *mut IUIAutomationTextRangeArray) -> HRESULT,
    pub get_DocumentRange: unsafe extern "system" fn(*mut IUIAutomationTextPattern, *mut *mut IUIAutomationTextRange) -> HRESULT,
    pub get_SupportedTextSelection: unsafe extern "system" fn(*mut IUIAutomationTextPattern, *mut i32) -> HRESULT,
}

#[repr(C)]
pub struct IUIAutomationTextPattern2 {
    pub lpVtbl: *const IUIAutomationTextPattern2Vtbl,
}

#[repr(C)]
pub struct IUIAutomationTextPattern2Vtbl {
    pub QueryInterface: unsafe extern "system" fn(*mut IUIAutomationTextPattern2, *const GUID, *mut *mut c_void) -> HRESULT,
    pub AddRef: unsafe extern "system" fn(*mut IUIAutomationTextPattern2) -> ULONG,
    pub Release: unsafe extern "system" fn(*mut IUIAutomationTextPattern2) -> ULONG,
    pub RangeFromPoint: unsafe extern "system" fn(*mut IUIAutomationTextPattern2, POINT, *mut *mut IUIAutomationTextRange) -> HRESULT,
    pub RangeFromChild: unsafe extern "system" fn(*mut IUIAutomationTextPattern2, *mut IUIAutomationElement, *mut *mut IUIAutomationTextRange) -> HRESULT,
    pub GetSelection: unsafe extern "system" fn(*mut IUIAutomationTextPattern2, *mut *mut IUIAutomationTextRangeArray) -> HRESULT,
    pub GetVisibleRanges: unsafe extern "system" fn(*mut IUIAutomationTextPattern2, *mut *mut IUIAutomationTextRangeArray) -> HRESULT,
    pub get_DocumentRange: unsafe extern "system" fn(*mut IUIAutomationTextPattern2, *mut *mut IUIAutomationTextRange) -> HRESULT,
    pub get_SupportedTextSelection: unsafe extern "system" fn(*mut IUIAutomationTextPattern2, *mut i32) -> HRESULT,
    pub RangeFromAnnotation: unsafe extern "system" fn(*mut IUIAutomationTextPattern2, *mut IUIAutomationElement, *mut *mut IUIAutomationTextRange) -> HRESULT,
    pub GetCaretRange: unsafe extern "system" fn(*mut IUIAutomationTextPattern2, *mut BOOL, *mut *mut IUIAutomationTextRange) -> HRESULT,
}

#[repr(C)]
pub struct IUIAutomationTextRange {
    pub lpVtbl: *const IUIAutomationTextRangeVtbl,
}

#[repr(C)]
pub struct IUIAutomationTextRangeVtbl {
    pub QueryInterface: unsafe extern "system" fn(*mut IUIAutomationTextRange, *const GUID, *mut *mut c_void) -> HRESULT,
    pub AddRef: unsafe extern "system" fn(*mut IUIAutomationTextRange) -> ULONG,
    pub Release: unsafe extern "system" fn(*mut IUIAutomationTextRange) -> ULONG,
    pub Clone: unsafe extern "system" fn(*mut IUIAutomationTextRange, *mut *mut IUIAutomationTextRange) -> HRESULT,
    pub Compare: unsafe extern "system" fn(*mut IUIAutomationTextRange, *mut IUIAutomationTextRange, *mut BOOL) -> HRESULT,
    pub CompareEndpoints: unsafe extern "system" fn(*mut IUIAutomationTextRange, i32, *mut IUIAutomationTextRange, i32, *mut i32) -> HRESULT,
    pub ExpandToEnclosingUnit: unsafe extern "system" fn(*mut IUIAutomationTextRange, i32) -> HRESULT,
    pub FindAttribute: unsafe extern "system" fn(*mut IUIAutomationTextRange, i32, [u64; 3], BOOL, *mut *mut IUIAutomationTextRange) -> HRESULT,
    pub FindText: unsafe extern "system" fn(*mut IUIAutomationTextRange, *const u16, BOOL, BOOL, *mut *mut IUIAutomationTextRange) -> HRESULT,
    pub GetAttributeValue: unsafe extern "system" fn(*mut IUIAutomationTextRange, i32, *mut c_void) -> HRESULT,
    pub GetBoundingRectangles: unsafe extern "system" fn(*mut IUIAutomationTextRange, *mut *mut SAFEARRAY) -> HRESULT,
    pub GetEnclosingElement: unsafe extern "system" fn(*mut IUIAutomationTextRange, *mut *mut IUIAutomationElement) -> HRESULT,
    pub GetText: unsafe extern "system" fn(*mut IUIAutomationTextRange, i32, *mut *mut u16) -> HRESULT,
    pub Move: unsafe extern "system" fn(*mut IUIAutomationTextRange, i32, i32, *mut i32) -> HRESULT,
    pub MoveEndpointByUnit: unsafe extern "system" fn(*mut IUIAutomationTextRange, i32, i32, i32, *mut i32) -> HRESULT,
    pub MoveEndpointByRange: unsafe extern "system" fn(*mut IUIAutomationTextRange, i32, *mut IUIAutomationTextRange, i32) -> HRESULT,
    pub Select: unsafe extern "system" fn(*mut IUIAutomationTextRange) -> HRESULT,
    pub AddToSelection: unsafe extern "system" fn(*mut IUIAutomationTextRange) -> HRESULT,
    pub RemoveFromSelection: unsafe extern "system" fn(*mut IUIAutomationTextRange) -> HRESULT,
    pub ScrollIntoView: unsafe extern "system" fn(*mut IUIAutomationTextRange, BOOL) -> HRESULT,
    pub GetChildren: unsafe extern "system" fn(*mut IUIAutomationTextRange, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
pub struct IUIAutomationTextRangeArray {
    pub lpVtbl: *const IUIAutomationTextRangeArrayVtbl,
}

#[repr(C)]
pub struct IUIAutomationTextRangeArrayVtbl {
    pub QueryInterface: unsafe extern "system" fn(*mut IUIAutomationTextRangeArray, *const GUID, *mut *mut c_void) -> HRESULT,
    pub AddRef: unsafe extern "system" fn(*mut IUIAutomationTextRangeArray) -> ULONG,
    pub Release: unsafe extern "system" fn(*mut IUIAutomationTextRangeArray) -> ULONG,
    pub get_Length: unsafe extern "system" fn(*mut IUIAutomationTextRangeArray, *mut i32) -> HRESULT,
    pub GetElement: unsafe extern "system" fn(*mut IUIAutomationTextRangeArray, i32, *mut *mut IUIAutomationTextRange) -> HRESULT,
}

unsafe fn release_range(r: &mut *mut IUIAutomationTextRange) {
    if !r.is_null() {
        let p = *r;
        *r = std::ptr::null_mut();
        ((*(*p).lpVtbl).Release)(p);
    }
}

unsafe fn extract_rect_from_safearray(psa: *mut SAFEARRAY) -> Option<(i32, i32, i32)> {
    if psa.is_null() {
        return None;
    }
    let mut lb: i32 = 0;
    let mut ub: i32 = -1;
    let mut out = None;
    if SafeArrayGetLBound(psa, 1, &mut lb) >= 0 && SafeArrayGetUBound(psa, 1, &mut ub) >= 0 {
        let count = ub - lb + 1;
        if count >= 4 {
            let mut data_ptr: *mut c_void = std::ptr::null_mut();
            if SafeArrayAccessData(psa, &mut data_ptr) >= 0 && !data_ptr.is_null() {
                let slice = std::slice::from_raw_parts(data_ptr as *const f64, count as usize);
                // Use the last bounding rectangle in the range (represents the tail of the range)
                let base = ((count as usize / 4) - 1) * 4;
                let left = slice[base];
                let top = slice[base + 1];
                let width = slice[base + 2];
                let height = slice[base + 3];
                if left.is_finite() && top.is_finite() && height >= 4.0 && (left != 0.0 || top != 0.0) {
                    let cx = (left + width.max(0.0)).round() as i32;
                    let cy = (top + height * 0.5).round() as i32;
                    let lh = (height.round() as i32).clamp(16, 56);
                    out = Some((cx, cy, lh));
                }
                let _ = SafeArrayUnaccessData(psa);
            }
        }
    }
    let _ = SafeArrayDestroy(psa);
    out
}

unsafe fn range_screen_point(range: *mut IUIAutomationTextRange) -> Option<(i32, i32, i32)> {
    if range.is_null() {
        return None;
    }
    // 1. Try direct GetBoundingRectangles on the range
    let mut psa: *mut SAFEARRAY = std::ptr::null_mut();
    if ((*(*range).lpVtbl).GetBoundingRectangles)(range, &mut psa) >= 0 && !psa.is_null() {
        if let Some(pt) = extract_rect_from_safearray(psa) {
            return Some(pt);
        }
    }
    // 2. Many UIA providers return 0 rects for a degenerate (collapsed 0-length) caret range.
    // Clone the range and expand by +1 or -1 character to obtain the exact line & character bounding box!
    let mut cloned: *mut IUIAutomationTextRange = std::ptr::null_mut();
    if ((*(*range).lpVtbl).Clone)(range, &mut cloned) >= 0 && !cloned.is_null() {
        let mut moved: i32 = 0;
        let _ = ((*(*cloned).lpVtbl).MoveEndpointByUnit)(
            cloned,
            ENDPOINT_END,
            TEXT_UNIT_CHARACTER,
            1,
            &mut moved,
        );
        let mut psa2: *mut SAFEARRAY = std::ptr::null_mut();
        if ((*(*cloned).lpVtbl).GetBoundingRectangles)(cloned, &mut psa2) >= 0 && !psa2.is_null() {
            if !psa2.is_null() {
                // For a forward-expanded character, we want the LEFT edge (`left`), not `left + width`
                let mut lb: i32 = 0;
                let mut ub: i32 = -1;
                let mut pt_left = None;
                if SafeArrayGetLBound(psa2, 1, &mut lb) >= 0 && SafeArrayGetUBound(psa2, 1, &mut ub) >= 0 {
                    let count = ub - lb + 1;
                    if count >= 4 {
                        let mut data_ptr: *mut c_void = std::ptr::null_mut();
                        if SafeArrayAccessData(psa2, &mut data_ptr) >= 0 && !data_ptr.is_null() {
                            let slice = std::slice::from_raw_parts(data_ptr as *const f64, count as usize);
                            let left = slice[0];
                            let top = slice[1];
                            let height = slice[3];
                            if left.is_finite() && top.is_finite() && height >= 4.0 && (left != 0.0 || top != 0.0) {
                                pt_left = Some((
                                    left.round() as i32,
                                    (top + height * 0.5).round() as i32,
                                    (height.round() as i32).clamp(16, 56),
                                ));
                            }
                            let _ = SafeArrayUnaccessData(psa2);
                        }
                    }
                }
                let _ = SafeArrayDestroy(psa2);
                if let Some(res) = pt_left {
                    ((*(*cloned).lpVtbl).Release)(cloned);
                    return Some(res);
                }
            }
        }
        // Try moving start backward by 1 character (when caret is at the very end of a line/paragraph)
        let _ = ((*(*cloned).lpVtbl).MoveEndpointByRange)(cloned, ENDPOINT_END, range, ENDPOINT_END);
        let _ = ((*(*cloned).lpVtbl).MoveEndpointByRange)(cloned, ENDPOINT_START, range, ENDPOINT_START);
        let _ = ((*(*cloned).lpVtbl).MoveEndpointByUnit)(
            cloned,
            ENDPOINT_START,
            TEXT_UNIT_CHARACTER,
            -1,
            &mut moved,
        );
        let mut psa3: *mut SAFEARRAY = std::ptr::null_mut();
        if ((*(*cloned).lpVtbl).GetBoundingRectangles)(cloned, &mut psa3) >= 0 && !psa3.is_null() {
            if let Some(res) = extract_rect_from_safearray(psa3) {
                ((*(*cloned).lpVtbl).Release)(cloned);
                return Some(res);
            }
        }
        ((*(*cloned).lpVtbl).Release)(cloned);
    }
    None
}

unsafe fn active_caret_range_from_element(elem: *mut IUIAutomationElement) -> *mut IUIAutomationTextRange {
    if elem.is_null() {
        return std::ptr::null_mut();
    }
    // 1. Try IUIAutomationTextPattern2::GetCaretRange
    let mut pat2: *mut IUIAutomationTextPattern2 = std::ptr::null_mut();
    if ((*(*elem).lpVtbl).GetCurrentPatternAs)(
        elem,
        UIA_TEXT_PATTERN2_ID,
        &IID_IUIAUTOMATION_TEXTPATTERN2,
        &mut pat2 as *mut _ as *mut *mut c_void,
    ) >= 0
        && !pat2.is_null()
    {
        let mut is_active: BOOL = 0;
        let mut caret_range: *mut IUIAutomationTextRange = std::ptr::null_mut();
        let hr = ((*(*pat2).lpVtbl).GetCaretRange)(pat2, &mut is_active, &mut caret_range);
        ((*(*pat2).lpVtbl).Release)(pat2);
        if hr >= 0 && !caret_range.is_null() {
            return caret_range;
        }
    }
    // 2. Fallback to IUIAutomationTextPattern::GetSelection
    let mut pat1: *mut IUIAutomationTextPattern = std::ptr::null_mut();
    if ((*(*elem).lpVtbl).GetCurrentPatternAs)(
        elem,
        UIA_TEXT_PATTERN_ID,
        &IID_IUIAUTOMATION_TEXTPATTERN,
        &mut pat1 as *mut _ as *mut *mut c_void,
    ) >= 0
        && !pat1.is_null()
    {
        let mut arr: *mut IUIAutomationTextRangeArray = std::ptr::null_mut();
        let hr = ((*(*pat1).lpVtbl).GetSelection)(pat1, &mut arr);
        ((*(*pat1).lpVtbl).Release)(pat1);
        if hr >= 0 && !arr.is_null() {
            let mut len: i32 = 0;
            let _ = ((*(*arr).lpVtbl).get_Length)(arr, &mut len);
            let mut first_range: *mut IUIAutomationTextRange = std::ptr::null_mut();
            if len > 0 {
                let _ = ((*(*arr).lpVtbl).GetElement)(arr, 0, &mut first_range);
            }
            ((*(*arr).lpVtbl).Release)(arr);
            if !first_range.is_null() {
                return first_range;
            }
        }
    }
    std::ptr::null_mut()
}

pub struct UiaVoiceAnchor {
    uia: *mut IUIAutomation,
    voice_range: *mut IUIAutomationTextRange,
}

impl UiaVoiceAnchor {
    pub fn new() -> Self {
        unsafe {
            let _ = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
            let mut uia: *mut IUIAutomation = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_CUIAUTOMATION,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_IUIAUTOMATION,
                &mut uia as *mut _ as *mut *mut c_void,
            );
            if hr < 0 {
                uia = std::ptr::null_mut();
            }
            Self {
                uia,
                voice_range: std::ptr::null_mut(),
            }
        }
    }

    pub fn clear_voice_range(&mut self) {
        unsafe {
            release_range(&mut self.voice_range);
        }
    }

    pub fn has_voice_range(&self) -> bool {
        !self.voice_range.is_null()
    }

    /// Query the focused UIA element's live caret screen coordinates `(cx, cy, line_h)`.
    pub fn query_caret_screen_point(&self) -> Option<(i32, i32, i32)> {
        if self.uia.is_null() {
            return None;
        }
        unsafe {
            let mut elem: *mut IUIAutomationElement = std::ptr::null_mut();
            if ((*(*self.uia).lpVtbl).GetFocusedElement)(self.uia, &mut elem) < 0 || elem.is_null() {
                return None;
            }
            let r = active_caret_range_from_element(elem);
            ((*(*elem).lpVtbl).Release)(elem);
            if r.is_null() {
                return None;
            }
            let pt = range_screen_point(r);
            ((*(*r).lpVtbl).Release)(r);
            pt
        }
    }

    /// Query the screen coordinates of the currently anchored `voice_range` (e.g. after window move/scroll).
    pub fn voice_range_screen_point(&self) -> Option<(i32, i32, i32)> {
        if self.voice_range.is_null() {
            return None;
        }
        unsafe { range_screen_point(self.voice_range) }
    }

    /// Anchor `voice_range` at the currently focused element's active caret and return its `(cx, cy, lh)`.
    pub fn anchor_at_current_caret(&mut self) -> Option<(i32, i32, i32)> {
        if self.uia.is_null() {
            return None;
        }
        unsafe {
            let mut elem: *mut IUIAutomationElement = std::ptr::null_mut();
            if ((*(*self.uia).lpVtbl).GetFocusedElement)(self.uia, &mut elem) < 0 || elem.is_null() {
                return None;
            }
            let r = active_caret_range_from_element(elem);
            ((*(*elem).lpVtbl).Release)(elem);
            if r.is_null() {
                return None;
            }
            // Collapse range to its end endpoint so it's a pure insertion point
            let _ = ((*(*r).lpVtbl).MoveEndpointByRange)(r, ENDPOINT_START, r, ENDPOINT_END);
            let pt = range_screen_point(r);
            release_range(&mut self.voice_range);
            self.voice_range = r;
            pt
        }
    }

    /// Anchor `voice_range` at screen point `(x, y)` via `IUIAutomationTextPattern::RangeFromPoint`
    /// without needing to simulate a mouse click!
    pub fn anchor_at_screen_point(&mut self, x: i32, y: i32) -> Option<(i32, i32, i32)> {
        if self.uia.is_null() {
            return None;
        }
        unsafe {
            let pt = POINT { x, y };
            let mut elem: *mut IUIAutomationElement = std::ptr::null_mut();
            if ((*(*self.uia).lpVtbl).ElementFromPoint)(self.uia, pt, &mut elem) < 0 || elem.is_null() {
                if ((*(*self.uia).lpVtbl).GetFocusedElement)(self.uia, &mut elem) < 0 || elem.is_null() {
                    return None;
                }
            }
            let mut pat1: *mut IUIAutomationTextPattern = std::ptr::null_mut();
            let hr = ((*(*elem).lpVtbl).GetCurrentPatternAs)(
                elem,
                UIA_TEXT_PATTERN_ID,
                &IID_IUIAUTOMATION_TEXTPATTERN,
                &mut pat1 as *mut _ as *mut *mut c_void,
            );
            ((*(*elem).lpVtbl).Release)(elem);
            if hr < 0 || pat1.is_null() {
                return None;
            }
            let mut r: *mut IUIAutomationTextRange = std::ptr::null_mut();
            let r_hr = ((*(*pat1).lpVtbl).RangeFromPoint)(pat1, pt, &mut r);
            ((*(*pat1).lpVtbl).Release)(pat1);
            if r_hr < 0 || r.is_null() {
                return None;
            }
            let _ = ((*(*r).lpVtbl).MoveEndpointByRange)(r, ENDPOINT_START, r, ENDPOINT_END);
            let screen_pt = range_screen_point(r).or(Some((x, y, 22)));
            release_range(&mut self.voice_range);
            self.voice_range = r;
            screen_pt
        }
    }

    /// Select the independent `voice_range` at the OS document level, invoke `commit_fn` to type/commit text,
    /// advance `voice_range` to the end of the newly inserted text, and restore the user's original selection!
    pub fn inject_at_voice_range<F>(&mut self, commit_fn: F) -> Option<(Result<usize, String>, Option<(i32, i32, i32)>)>
    where
        F: FnOnce() -> Result<usize, String>,
    {
        if self.uia.is_null() || self.voice_range.is_null() {
            return None;
        }
        unsafe {
            // 1. Save the user's current selection range so we can restore it after inserting at `voice_range`
            let mut elem: *mut IUIAutomationElement = std::ptr::null_mut();
            let mut saved_user_range: *mut IUIAutomationTextRange = std::ptr::null_mut();
            if ((*(*self.uia).lpVtbl).GetFocusedElement)(self.uia, &mut elem) >= 0 && !elem.is_null() {
                saved_user_range = active_caret_range_from_element(elem);
                ((*(*elem).lpVtbl).Release)(elem);
            }

            // 2. Select `self.voice_range` via UIAutomation (zero mouse click!)
            if ((*(*self.voice_range).lpVtbl).Select)(self.voice_range) < 0 {
                release_range(&mut saved_user_range);
                return None;
            }

            // 3. Commit the voice segment at the newly selected OS caret
            let res = commit_fn();

            // 4. Capture the updated caret position right after the inserted text and update `self.voice_range`!
            let mut new_pt = None;
            let mut elem_after: *mut IUIAutomationElement = std::ptr::null_mut();
            if ((*(*self.uia).lpVtbl).GetFocusedElement)(self.uia, &mut elem_after) >= 0 && !elem_after.is_null() {
                let updated_caret = active_caret_range_from_element(elem_after);
                ((*(*elem_after).lpVtbl).Release)(elem_after);
                if !updated_caret.is_null() {
                    let _ = ((*(*updated_caret).lpVtbl).MoveEndpointByRange)(
                        updated_caret,
                        ENDPOINT_START,
                        updated_caret,
                        ENDPOINT_END,
                    );
                    new_pt = range_screen_point(updated_caret);
                    release_range(&mut self.voice_range);
                    self.voice_range = updated_caret;
                }
            }

            // 5. Restore the user's primary selection / cursor if they were editing elsewhere!
            if !saved_user_range.is_null() {
                let _ = ((*(*saved_user_range).lpVtbl).Select)(saved_user_range);
                release_range(&mut saved_user_range);
            }

            Some((res, new_pt))
        }
    }
}

impl Drop for UiaVoiceAnchor {
    fn drop(&mut self) {
        unsafe {
            release_range(&mut self.voice_range);
            if !self.uia.is_null() {
                ((*(*self.uia).lpVtbl).Release)(self.uia);
                self.uia = std::ptr::null_mut();
            }
        }
    }
}
