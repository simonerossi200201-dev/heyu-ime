//! Edit sessions: the only place the document is modified.
//! * `Apply`     - insert committed text (through a short-lived composition, like most IMEs),
//!                 then start/update the composition that shows the typed code (preedit).
//! * `Terminate` - end our composition (optionally clearing its text).
//! After editing, the composition (or caret) rectangle is read with ITfContextView::GetTextExt
//! and sent to the server so the candidate HUD follows the caret.

use crate::tip::Shared;
use crate::{guard_hr, DllRef};
use heyu_core::ipc::{CaretRect, Request, RequestKind};
use std::mem::ManuallyDrop;
use std::rc::Rc;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::HiDpi::LogicalToPhysicalPointForPerMonitorDPI;
use windows_core::{implement, Interface, Result, BOOL};

pub(crate) enum EditOp {
    Apply { commit: String, preedit: String, caret: u32 },
    Terminate { keep_text: bool },
    ReportCaret,
}

#[implement(ITfEditSession)]
pub(crate) struct EditSession {
    shared: Rc<Shared>,
    ctx: ITfContext,
    sink: ITfCompositionSink,
    op: EditOp,
    _ref: DllRef,
}

impl EditSession {
    pub(crate) fn new(shared: Rc<Shared>, ctx: ITfContext, sink: ITfCompositionSink, op: EditOp) -> Self {
        Self { shared, ctx, sink, op, _ref: DllRef::new() }
    }
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        guard_hr(|| self.run(ec))
    }
}

fn same_object(a: &ITfContext, b: &ITfContext) -> bool {
    match (a.cast::<windows_core::IUnknown>(), b.cast::<windows_core::IUnknown>()) {
        (Ok(x), Ok(y)) => x.as_raw() == y.as_raw(),
        _ => false,
    }
}

impl EditSession {
    /// Our composition, if it lives in *this* context (a composition from another context cannot
    /// be edited with this edit cookie; forget it and let TSF terminate it).
    fn composition(&self) -> Option<ITfComposition> {
        let mut st = self.shared.state.borrow_mut();
        match (&st.composition, &st.composition_context) {
            (Some(c), Some(cx)) if same_object(cx, &self.ctx) => Some(c.clone()),
            (Some(_), _) => {
                st.composition = None;
                st.composition_context = None;
                st.preedit.clear();
                None
            }
            _ => None,
        }
    }

    fn forget_composition(&self) {
        let mut st = self.shared.state.borrow_mut();
        st.composition = None;
        st.composition_context = None;
        st.preedit.clear();
    }

    fn run(&self, ec: u32) -> Result<()> {
        match &self.op {
            EditOp::Apply { commit, preedit, caret } => {
                if !commit.is_empty() {
                    self.commit_text(ec, commit)?;
                }
                if !preedit.is_empty() {
                    self.set_preedit(ec, preedit, *caret)?;
                } else {
                    self.end_composition(ec, false)?;
                }
                self.report_caret(ec);
                Ok(())
            }
            EditOp::Terminate { keep_text } => self.end_composition(ec, *keep_text),
            EditOp::ReportCaret => {
                self.shared.state.borrow_mut().last_sent_caret = None;
                self.report_caret(ec);
                Ok(())
            }
        }
    }

    fn set_selection(&self, ec: u32, range: &ITfRange) -> Result<()> {
        let sel = TF_SELECTION {
            range: ManuallyDrop::new(Some(range.clone())),
            style: TF_SELECTIONSTYLE { ase: TF_AE_NONE, fInterimChar: BOOL(0) },
        };
        let r = unsafe { self.ctx.SetSelection(ec, std::slice::from_ref(&sel)) };
        drop(ManuallyDrop::into_inner(sel.range));
        r
    }

    fn caret_to_end(&self, ec: u32, range: &ITfRange) -> Result<()> {
        unsafe {
            let r = range.Clone()?;
            r.Collapse(ec, TF_ANCHOR_END)?;
            self.set_selection(ec, &r)
        }
    }

    fn start_composition(&self, ec: u32) -> Result<ITfComposition> {
        unsafe {
            let ins: ITfInsertAtSelection = self.ctx.cast()?;
            let range = ins.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])?;
            let cc: ITfContextComposition = self.ctx.cast()?;
            let comp = cc.StartComposition(ec, &range, &self.sink)?;
            let mut st = self.shared.state.borrow_mut();
            st.composition = Some(comp.clone());
            st.composition_context = Some(self.ctx.clone());
            st.preedit.clear();
            Ok(comp)
        }
    }

    fn current_selection(&self, ec: u32) -> Option<ITfRange> {
        let mut sel = [TF_SELECTION::default()];
        let mut fetched = 0u32;
        unsafe {
            self.ctx.GetSelection(ec, TF_DEFAULT_SELECTION, &mut sel, &mut fetched).ok()?;
            let range = ManuallyDrop::take(&mut sel[0].range);
            if fetched == 0 {
                return None;
            }
            range
        }
    }

    fn delete_before_caret(&self, ec: u32, n: usize) -> Result<()> {
        if let Some(range) = self.current_selection(ec) {
            unsafe {
                range.Collapse(ec, TF_ANCHOR_START)?;
                let mut moved = 0i32;
                range.ShiftStart(ec, -(n as i32), &mut moved, std::ptr::null())?;
                range.SetText(ec, 0, &[])?;
            }
        }
        Ok(())
    }

    fn commit_text(&self, ec: u32, text: &str) -> Result<()> {
        let backspaces = text.chars().take_while(|c| *c == '\u{8}').count();
        let body: Vec<u16> = text.chars().skip(backspaces).collect::<String>().encode_utf16().collect();
        let existing = self.composition();
        unsafe {
            if backspaces > 0 {
                if let Some(comp) = existing.as_ref() {
                    if let Ok(r) = comp.GetRange() {
                        let _ = r.SetText(ec, 0, &[]);
                    }
                    let _ = comp.EndComposition(ec);
                    self.forget_composition();
                }
                self.delete_before_caret(ec, backspaces)?;
                if body.is_empty() {
                    return Ok(());
                }
            }
            // Commit through a composition (existing one, or a short-lived new one).
            let comp = match existing.filter(|_| backspaces == 0) {
                Some(c) => c,
                None => self.start_composition(ec)?,
            };
            let range = comp.GetRange()?;
            range.SetText(ec, 0, &body)?;
            let _ = self.caret_to_end(ec, &range);
            comp.EndComposition(ec)?;
            self.forget_composition();
        }
        Ok(())
    }

    fn set_preedit(&self, ec: u32, preedit: &str, caret: u32) -> Result<()> {
        let wide: Vec<u16> = preedit.encode_utf16().collect();
        let comp = match self.composition() {
            Some(c) => c,
            None => self.start_composition(ec)?,
        };
        unsafe {
            let range = comp.GetRange()?;
            range.SetText(ec, 0, &wide)?;
            let sel = range.Clone()?;
            if caret as usize >= wide.len() {
                sel.Collapse(ec, TF_ANCHOR_END)?;
            } else {
                sel.Collapse(ec, TF_ANCHOR_START)?;
                let mut moved = 0i32;
                sel.ShiftStart(ec, caret as i32, &mut moved, std::ptr::null())?;
                sel.ShiftEnd(ec, caret as i32, &mut moved, std::ptr::null())?;
            }
            let _ = self.set_selection(ec, &sel);
        }
        self.shared.state.borrow_mut().preedit = preedit.to_string();
        Ok(())
    }

    fn end_composition(&self, ec: u32, keep_text: bool) -> Result<()> {
        let Some(comp) = self.composition() else {
            self.forget_composition();
            return Ok(());
        };
        unsafe {
            if let Ok(range) = comp.GetRange() {
                if keep_text {
                    let _ = self.caret_to_end(ec, &range);
                } else {
                    let _ = range.SetText(ec, 0, &[]);
                }
            }
            let _ = comp.EndComposition(ec);
        }
        self.forget_composition();
        Ok(())
    }

    /// Read the composition (or caret) rectangle and tell the server where to draw the HUD.
    fn report_caret(&self, ec: u32) {
        let range = match self.composition() {
            Some(c) => unsafe { c.GetRange().ok() },
            None => self.current_selection(ec),
        };
        let Some(range) = range else { return };
        let mut rc = RECT::default();
        let mut clipped = BOOL(0);
        unsafe {
            let Ok(view) = self.ctx.GetActiveView() else { return };
            if view.GetTextExt(ec, &range, &mut rc, &mut clipped).is_err() {
                return; // e.g. TS_E_NOLAYOUT: keep the previous position
            }
            if rc.left == 0 && rc.top == 0 && rc.right == 0 && rc.bottom == 0 {
                return;
            }
            // DPI-unaware apps report logical coordinates; the server works in physical pixels.
            if let Ok(hwnd) = view.GetWnd() {
                if !hwnd.is_invalid() {
                    let mut p1 = POINT { x: rc.left, y: rc.top };
                    let mut p2 = POINT { x: rc.right, y: rc.bottom };
                    if LogicalToPhysicalPointForPerMonitorDPI(Some(hwnd), &mut p1).as_bool()
                        && LogicalToPhysicalPointForPerMonitorDPI(Some(hwnd), &mut p2).as_bool()
                    {
                        rc = RECT { left: p1.x, top: p1.y, right: p2.x, bottom: p2.y };
                    }
                }
            }
        }
        let caret = CaretRect { left: rc.left, top: rc.top, right: rc.right.max(rc.left), bottom: rc.bottom.max(rc.top) };
        let changed = {
            let mut st = self.shared.state.borrow_mut();
            st.last_caret = Some(caret);
            if st.last_sent_caret != Some(caret) {
                st.last_sent_caret = Some(caret);
                true
            } else {
                false
            }
        };
        if changed {
            let mut req = Request::new(RequestKind::Caret);
            req.caret = Some(caret);
            let _ = self.shared.send(&req);
        }
    }
}
