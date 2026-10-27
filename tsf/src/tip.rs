//! The TSF text input processor object (one per UI thread that activates HEYU).
//!
//! Key flow (mirrors Weasel): `OnTestKeyDown` sends the key to heyu-ime.exe and caches the answer;
//! `OnKeyDown` applies the cached answer through an edit session (start/update/end composition,
//! insert committed text). A repeated `OnTestKeyDown` for a key that is still pending is answered
//! from the cache so a key is never sent twice. If the server cannot answer within
//! [`crate::IPC_TIMEOUT`] the key is passed through to the application.
//!
//! RefCell rule: `Shared::state` is only borrowed for short field reads/writes and never while
//! calling into TSF (RequestEditSession / SetValue can call straight back into this object).

use crate::edit::{EditOp, EditSession};
use crate::{guard, guard_hr, DllRef, IPC_TIMEOUT};
use heyu_core::ipc::*;
use heyu_core::pipe::PipeClient;
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::TextServices::*;
use windows_core::{implement, IUnknownImpl, Interface, Ref, Result, BOOL, GUID};

pub(crate) const TF_INVALID_COOKIE: u32 = 0xFFFF_FFFF;
const VK_SHIFT: u16 = 0x10;

pub(crate) struct State {
    pub thread_mgr: Option<ITfThreadMgr>,
    pub client_id: u32,
    pub secure_mode: bool,
    pub threadmgr_cookie: u32,
    pub conv_cookie: u32,
    pub conv_compartment: Option<ITfCompartment>,
    pub key_sink_advised: bool,
    pub composition: Option<ITfComposition>,
    pub composition_context: Option<ITfContext>,
    /// Text currently shown in our composition.
    pub preedit: String,
    /// (raw wParam, response) cached by OnTestKeyDown for OnKeyDown.
    pub pending: Option<(usize, Response)>,
    pub last_caret: Option<CaretRect>,
    pub last_sent_caret: Option<CaretRect>,
    pub ascii_mode: Option<bool>,
    pub setting_compartment: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            thread_mgr: None,
            client_id: 0,
            secure_mode: false,
            threadmgr_cookie: TF_INVALID_COOKIE,
            conv_cookie: TF_INVALID_COOKIE,
            conv_compartment: None,
            key_sink_advised: false,
            composition: None,
            composition_context: None,
            preedit: String::new(),
            pending: None,
            last_caret: None,
            last_sent_caret: None,
            ascii_mode: None,
            setting_compartment: false,
        }
    }
}

pub(crate) struct Shared {
    pub state: RefCell<State>,
    pub pipe: RefCell<PipeClient>,
}

impl Shared {
    /// One bounded round trip to the server. `None` = unavailable -> pass the key through.
    pub fn send(&self, req: &Request) -> Option<Response> {
        let (secure, caret) = match self.state.try_borrow() {
            Ok(st) => (st.secure_mode, st.last_caret),
            Err(_) => return None,
        };
        if secure {
            return None; // secure desktop / secure mode: never talk to the user-session server
        }
        let mut req = req.clone();
        if req.caret.is_none() {
            req.caret = caret;
        }
        if req.pid == 0 {
            req.pid = unsafe { heyu_core::win32::GetCurrentProcessId() };
        }
        let mut pipe = self.pipe.try_borrow_mut().ok()?;
        pipe.call(&req, IPC_TIMEOUT).filter(|r| !r.unavailable)
    }
}

#[implement(ITfTextInputProcessorEx, ITfThreadMgrEventSink, ITfKeyEventSink, ITfCompositionSink, ITfCompartmentEventSink)]
pub struct TextService {
    shared: Rc<Shared>,
    _ref: DllRef,
}

impl TextService {
    pub fn new() -> Self {
        Self {
            shared: Rc::new(Shared { state: RefCell::new(State::default()), pipe: RefCell::new(PipeClient::for_current_session()) }),
            _ref: DllRef::new(),
        }
    }
}

fn key_is_down(vk: i32) -> bool {
    unsafe { GetKeyState(vk) < 0 }
}

fn compartment_i32(c: &ITfCompartment) -> Option<i32> {
    let v: VARIANT = unsafe { c.GetValue() }.ok()?;
    i32::try_from(&v).ok()
}

impl TextService_Impl {
    fn st(&self) -> std::cell::RefMut<'_, State> {
        self.shared.state.borrow_mut()
    }

    fn build_key_request(&self, wparam: WPARAM, lparam: LPARAM, up: bool) -> Request {
        let raw_vk = wparam.0 as u16;
        let lp = lparam.0 as usize;
        let scan = ((lp >> 16) & 0xFF) as u16;
        let extended = (lp >> 24) & 1 == 1;
        let lshift = key_is_down(0xA0);
        let rshift = key_is_down(0xA1);
        let mut m = 0u16;
        if key_is_down(0x10) || lshift || rshift {
            m |= MOD_SHIFT;
        }
        if lshift {
            m |= MOD_LSHIFT;
        }
        if rshift {
            m |= MOD_RSHIFT;
        }
        if key_is_down(0x11) {
            m |= MOD_CTRL;
        }
        if key_is_down(0x12) {
            m |= MOD_ALT;
        }
        if key_is_down(0x5B) || key_is_down(0x5C) {
            m |= MOD_WIN;
        }
        let caps = unsafe { GetKeyState(0x14) } & 1 != 0;
        if caps {
            m |= MOD_CAPSLOCK;
        }
        if extended {
            m |= MOD_EXTENDED;
        }
        let vk = if raw_vk == VK_SHIFT {
            if scan == 0x36 {
                0xA1
            } else {
                0xA0
            }
        } else {
            raw_vk
        };
        let ch = vk_to_ascii_char(vk, m & MOD_SHIFT != 0, caps).map(|c| c as u32).unwrap_or(0);
        let mut req = Request::key(vk, '\0', m, up);
        req.ch = ch;
        req.scan = scan;
        req
    }

    fn is_keyboard_disabled(&self, ctx: &ITfContext) -> bool {
        let tim = self.st().thread_mgr.clone();
        unsafe {
            if let Some(cm) = tim.and_then(|t| t.cast::<ITfCompartmentMgr>().ok()) {
                if let Ok(c) = cm.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE) {
                    if compartment_i32(&c) == Some(0) {
                        return true;
                    }
                }
            }
            if let Ok(cm) = ctx.cast::<ITfCompartmentMgr>() {
                for g in [GUID_COMPARTMENT_KEYBOARD_DISABLED, GUID_COMPARTMENT_EMPTYCONTEXT] {
                    if let Ok(c) = cm.GetCompartment(&g) {
                        if compartment_i32(&c).unwrap_or(0) != 0 {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    pub(crate) fn request_edit(&self, ctx: &ITfContext, op: EditOp) {
        let tid = self.st().client_id;
        let sink: ITfCompositionSink = self.to_interface();
        let es: ITfEditSession = EditSession::new(self.shared.clone(), ctx.clone(), sink, op).into();
        unsafe {
            let _ = ctx.RequestEditSession(tid, &es, TF_ES_ASYNCDONTCARE | TF_ES_READWRITE);
        }
    }

    /// Mirror the server's Chinese/ASCII state into the conversion-mode compartment (taskbar 中/英).
    fn sync_ascii(&self, ascii: bool) {
        let (conv, tid) = {
            let mut st = self.st();
            if st.ascii_mode == Some(ascii) {
                return;
            }
            st.ascii_mode = Some(ascii);
            (st.conv_compartment.clone(), st.client_id)
        };
        if let Some(c) = conv {
            self.st().setting_compartment = true;
            let v = VARIANT::from(if ascii { TF_CONVERSIONMODE_ALPHANUMERIC } else { TF_CONVERSIONMODE_NATIVE } as i32);
            unsafe {
                let _ = c.SetValue(tid, &v);
            }
            self.st().setting_compartment = false;
        }
    }

    /// Bring the document in line with a server response.
    fn apply(&self, ctx: &ITfContext, r: &Response) {
        self.sync_ascii(r.ascii_mode);
        let (has_comp, same_preedit) = {
            let st = self.st();
            (st.composition.is_some(), st.preedit == r.preedit_text)
        };
        let need_edit = !r.commit_text.is_empty() || !same_preedit || (has_comp && r.preedit_text.is_empty());
        if need_edit {
            self.request_edit(
                ctx,
                EditOp::Apply { commit: r.commit_text.clone(), preedit: r.preedit_text.clone(), caret: r.preedit_caret },
            );
        }
    }

    /// Server vanished mid-composition: finish the composition (keeping the typed code).
    fn on_server_lost(&self, ctx: &ITfContext) {
        if self.st().composition.is_some() {
            self.request_edit(ctx, EditOp::Terminate { keep_text: true });
        }
    }

    fn terminate_composition(&self, keep_text: bool) {
        let ctx = {
            let st = self.st();
            if st.composition.is_none() {
                return;
            }
            st.composition_context.clone()
        };
        if let Some(ctx) = ctx {
            self.request_edit(&ctx, EditOp::Terminate { keep_text });
        }
    }

    fn handle_key_down(&self, pic: &Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM, testing: bool) -> Result<BOOL> {
        // Fast-path passthrough for injected Unicode packets (`VK_PACKET = 0xE7` from `heyu-voice.exe`
        // `SendInput(KEYEVENTF_UNICODE)`): never intercept or disturb active keyboard typing/composition!
        if (wparam.0 as u16) == 0xE7 {
            return Ok(false.into());
        }
        let Some(ctx) = pic.as_ref().cloned() else { return Ok(false.into()) };
        if self.st().thread_mgr.is_none() {
            return Ok(false.into());
        }
        // Answer from the OnTestKeyDown cache.
        let pending = self.st().pending.take();
        if let Some((w, resp)) = pending {
            if w == wparam.0 {
                if testing {
                    // TSF tested the same key again: keep it pending, do not resend.
                    self.st().pending = Some((w, resp));
                } else {
                    self.apply(&ctx, &resp);
                }
                return Ok(true.into());
            }
        }
        if self.is_keyboard_disabled(&ctx) {
            return Ok(false.into());
        }
        let req = self.build_key_request(wparam, lparam, false);
        match self.shared.send(&req) {
            None => {
                self.on_server_lost(&ctx);
                Ok(false.into())
            }
            Some(r) if r.eaten => {
                if testing {
                    self.st().pending = Some((wparam.0, r));
                } else {
                    self.apply(&ctx, &r);
                }
                Ok(true.into())
            }
            Some(r) => {
                self.apply(&ctx, &r);
                Ok(false.into())
            }
        }
    }

    fn handle_key_up(&self, pic: &Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        let vk = wparam.0 as u16;
        if self.st().thread_mgr.is_none() {
            return Ok(false.into());
        }
        // When a voice chunk finishes typing (sentinel 0x88) or when the user types / presses Enter / Backspace / Arrows,
        // query the live TSF caret rectangle so dual-cursor co-editing tracks the exact caret position in every app!
        if vk == 0x88 || matches!(vk, 0x08 | 0x0D | 0x20 | 0x23..=0x28 | 0x2E) || (0x30..=0x5A).contains(&vk) {
            if let Some(ctx) = pic.as_ref() {
                self.request_edit(ctx, EditOp::ReportCaret);
            }
            return Ok(false.into());
        }
        // Shift releases matter for Shift tap toggle (toggles Chinese/ASCII and commits preedit English); never eat key-ups.
        if vk != VK_SHIFT && vk != 0xA0 && vk != 0xA1 {
            return Ok(false.into());
        }
        let req = self.build_key_request(wparam, lparam, true);
        if let Some(r) = self.shared.send(&req) {
            match pic.as_ref() {
                Some(ctx) => self.apply(ctx, &r),
                None => self.sync_ascii(r.ascii_mode),
            }
        }
        Ok(false.into())
    }

    fn notify_focus(&self, kind: RequestKind) {
        if let Some(r) = self.shared.send(&Request::new(kind)) {
            self.sync_ascii(r.ascii_mode);
        }
    }
}

// ============================================================================
// ITfTextInputProcessor(Ex)
// ============================================================================
impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<ITfThreadMgr>, tid: u32) -> Result<()> {
        ITfTextInputProcessorEx_Impl::ActivateEx(self, ptim, tid, 0)
    }

    fn Deactivate(&self) -> Result<()> {
        guard_hr(|| {
            self.terminate_composition(false);
            let (tim, tid, tm_cookie, conv_cookie, conv, key_adv) = {
                let mut st = self.st();
                let out = (
                    st.thread_mgr.take(),
                    st.client_id,
                    std::mem::replace(&mut st.threadmgr_cookie, TF_INVALID_COOKIE),
                    std::mem::replace(&mut st.conv_cookie, TF_INVALID_COOKIE),
                    st.conv_compartment.take(),
                    std::mem::replace(&mut st.key_sink_advised, false),
                );
                st.composition = None;
                st.composition_context = None;
                st.preedit.clear();
                st.pending = None;
                out
            };
            unsafe {
                if let Some(tim) = &tim {
                    if key_adv {
                        if let Ok(k) = tim.cast::<ITfKeystrokeMgr>() {
                            let _ = k.UnadviseKeyEventSink(tid);
                        }
                    }
                    if tm_cookie != TF_INVALID_COOKIE {
                        if let Ok(s) = tim.cast::<ITfSource>() {
                            let _ = s.UnadviseSink(tm_cookie);
                        }
                    }
                }
                if let (Some(c), true) = (&conv, conv_cookie != TF_INVALID_COOKIE) {
                    if let Ok(s) = c.cast::<ITfSource>() {
                        let _ = s.UnadviseSink(conv_cookie);
                    }
                }
            }
            let _ = self.shared.send(&Request::new(RequestKind::Reset));
            if let Ok(mut p) = self.shared.pipe.try_borrow_mut() {
                p.disconnect();
            }
            let mut st = self.st();
            st.ascii_mode = None;
            st.secure_mode = false;
            Ok(())
        })
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(&self, ptim: Ref<ITfThreadMgr>, tid: u32, dwflags: u32) -> Result<()> {
        guard_hr(|| {
            let tim = ptim.ok()?.clone();
            {
                let mut st = self.st();
                st.thread_mgr = Some(tim.clone());
                st.client_id = tid;
                st.secure_mode = dwflags & TF_TMAE_SECUREMODE != 0;
            }
            unsafe {
                // Thread manager events (focus changes).
                if let Ok(src) = tim.cast::<ITfSource>() {
                    let sink: ITfThreadMgrEventSink = self.to_interface();
                    if let Ok(c) = src.AdviseSink(&ITfThreadMgrEventSink::IID, &sink) {
                        self.st().threadmgr_cookie = c;
                    }
                }
                // Keyboard events: mandatory.
                let ksm: ITfKeystrokeMgr = match tim.cast() {
                    Ok(k) => k,
                    Err(e) => {
                        let _ = ITfTextInputProcessor_Impl::Deactivate(self);
                        return Err(e);
                    }
                };
                let ks: ITfKeyEventSink = self.to_interface();
                if let Err(e) = ksm.AdviseKeyEventSink(tid, &ks, true) {
                    let _ = ITfTextInputProcessor_Impl::Deactivate(self);
                    return Err(e);
                }
                self.st().key_sink_advised = true;

                // Compartments: keyboard open + conversion mode (taskbar 中/英 indicator).
                if let Ok(cm) = tim.cast::<ITfCompartmentMgr>() {
                    if let Ok(oc) = cm.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE) {
                        let _ = oc.SetValue(tid, &VARIANT::from(1i32));
                    }
                    if let Ok(conv) = cm.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION) {
                        self.st().conv_compartment = Some(conv.clone());
                        if let Ok(src) = conv.cast::<ITfSource>() {
                            let ces: ITfCompartmentEventSink = self.to_interface();
                            if let Ok(c) = src.AdviseSink(&ITfCompartmentEventSink::IID, &ces) {
                                self.st().conv_cookie = c;
                            }
                        }
                    }
                }
            }
            let ascii = self.shared.send(&Request::new(RequestKind::Ping)).map(|r| r.ascii_mode).unwrap_or(false);
            self.sync_ascii(ascii);
            Ok(())
        })
    }
}

// ============================================================================
// ITfThreadMgrEventSink
// ============================================================================
impl ITfThreadMgrEventSink_Impl for TextService_Impl {
    fn OnInitDocumentMgr(&self, _pdim: Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }
    fn OnUninitDocumentMgr(&self, _pdim: Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }
    fn OnSetFocus(&self, _pdimfocus: Ref<ITfDocumentMgr>, _pdimprevfocus: Ref<ITfDocumentMgr>) -> Result<()> {
        guard_hr(|| {
            // Document focus moved inside this thread: drop the half-typed code and reset the server.
            self.terminate_composition(false);
            {
                let mut st = self.st();
                st.pending = None;
                st.last_sent_caret = None;
            }
            self.notify_focus(RequestKind::FocusIn);
            Ok(())
        })
    }
    fn OnPushContext(&self, _pic: Ref<ITfContext>) -> Result<()> {
        Ok(())
    }
    fn OnPopContext(&self, _pic: Ref<ITfContext>) -> Result<()> {
        Ok(())
    }
}

// ============================================================================
// ITfKeyEventSink
// ============================================================================
impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, fforeground: BOOL) -> Result<()> {
        guard_hr(|| {
            if fforeground.as_bool() {
                self.notify_focus(RequestKind::FocusIn);
            } else {
                self.terminate_composition(false);
                self.st().pending = None;
                self.notify_focus(RequestKind::Reset);
            }
            Ok(())
        })
    }

    fn OnTestKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guard(Ok(false.into()), || self.handle_key_down(&pic, wparam, lparam, true))
    }

    fn OnTestKeyUp(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guard(Ok(false.into()), || self.handle_key_up(&pic, wparam, lparam))
    }

    fn OnKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guard(Ok(false.into()), || self.handle_key_down(&pic, wparam, lparam, false))
    }

    fn OnKeyUp(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guard(Ok(false.into()), || self.handle_key_up(&pic, wparam, lparam))
    }

    fn OnPreservedKey(&self, _pic: Ref<ITfContext>, _rguid: *const GUID) -> Result<BOOL> {
        Ok(false.into())
    }
}

// ============================================================================
// ITfCompositionSink
// ============================================================================
impl ITfCompositionSink_Impl for TextService_Impl {
    fn OnCompositionTerminated(&self, _ecwrite: u32, _pcomposition: Ref<ITfComposition>) -> Result<()> {
        guard_hr(|| {
            {
                let mut st = self.st();
                st.composition = None;
                st.composition_context = None;
                st.preedit.clear();
                st.pending = None;
            }
            // The app ended our composition (click elsewhere, etc.): clear the engine buffer + HUD.
            let _ = self.shared.send(&Request::new(RequestKind::Reset));
            Ok(())
        })
    }
}

// ============================================================================
// ITfCompartmentEventSink (user clicked the taskbar 中/英 indicator)
// ============================================================================
impl ITfCompartmentEventSink_Impl for TextService_Impl {
    fn OnChange(&self, rguid: *const GUID) -> Result<()> {
        guard_hr(|| {
            if rguid.is_null() || unsafe { *rguid } != GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION {
                return Ok(());
            }
            let (setting, conv, known) = {
                let st = self.st();
                (st.setting_compartment, st.conv_compartment.clone(), st.ascii_mode)
            };
            if setting {
                return Ok(());
            }
            let Some(conv) = conv else { return Ok(()) };
            let val = compartment_i32(&conv).unwrap_or(TF_CONVERSIONMODE_NATIVE as i32) as u32;
            let want_ascii = val & TF_CONVERSIONMODE_NATIVE == 0;
            if known == Some(want_ascii) {
                return Ok(());
            }
            let mut req = Request::new(RequestKind::SetAsciiMode);
            req.arg = want_ascii as u32;
            if let Some(r) = self.shared.send(&req) {
                self.st().ascii_mode = Some(r.ascii_mode);
            }
            self.terminate_composition(false);
            Ok(())
        })
    }
}
