use std::{
    cell::RefCell,
    mem::ManuallyDrop,
    rc::Rc,
};

use anyhow::Result;
use windows::{
    core::{implement, Interface},
    Win32::{
        Foundation::{BOOL, E_FAIL, RECT},
        UI::TextServices::{
            ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition, ITfEditSession,
            ITfEditSession_Impl, ITfInsertAtSelection, ITfRange, GUID_PROP_ATTRIBUTE, TF_AE_END,
            TF_ANCHOR_END, TF_ANCHOR_START, TF_ES_READ, TF_ES_READWRITE, TF_ES_SYNC,
            TF_IAS_QUERYONLY, TF_SELECTION, TF_SELECTIONSTYLE, TF_TF_MOVESTART,
        },
    },
};
use windows_core::VARIANT;

// 帰ったとき用のメモ
//
// # EditSessionに必要な関数
// - get_text_before_text
// - get_text_after_text

use crate::extension::StringExt;

pub struct ContextEditor<'a> {
    context: &'a ITfContext,
    ec: u32,
}

impl<'a> ContextEditor<'a> {
    pub fn new(context: &'a ITfContext, ec: u32) -> Self {
        Self { context, ec }
    }

    /// Compositionを作成せずに文字列をinsertする
    #[macros::anyhow]
    pub fn insert_text(&self, text: &str) -> Result<()> {
        // TSFにはITfInsertAtSelectionというものがあり、それを使ってInsertすることも可能。
        // その場合、以下のようなコードを書くことになる
        //
        // ```rust
        // let insert_at: ITfInsertAtSelection = self.context.cast()?;
        // let wide = text.to_wide_16_unpadded();
        // unsafe {
        //     insert_at.InsertTextAtSelection(self.ec, INSERT_TEXT_AT_SELECTION_FLAGS(0), &wide)?;
        // }
        // ```
        //
        // ただし、InsertTextAtSelectionのフラグとしてTF_IAS_NOQUERYを利用してはならない。
        // TF_IAS_NOQUERYを利用すると、返り値のITfRangeの代わりにnull ptrが返ってくる。
        // それをRustがDropしようとしてアクセス違反でクラッシュしてしまうからである。
        unsafe {
            if let Some(selection) = self.get_selection_range()? {
                let wide = text.to_wide_16_unpadded();
                selection.SetText(self.ec, 0, &wide)?;
            }
        }
        Ok(())
    }

    #[macros::anyhow]
    pub fn delete_backward(&self, count: i32) -> Result<()> {
        unsafe {
            let selection = self.get_insertion_range()?;

            let range = selection.Clone()?;
            range.Collapse(self.ec, TF_ANCHOR_START)?;

            let mut shifted = 0i32;
            range.ShiftStart(self.ec, -count, &mut shifted, std::ptr::null())?;

            if range.IsEmpty(self.ec)?.as_bool() {
                return Ok(());
            }

            range.SetText(self.ec, 0, &[])?;
        }
        Ok(())
    }

    #[macros::anyhow(fail_with = E_FAIL)]
    pub fn get_selection_range(&self) -> Result<Option<ITfRange>> {
        unsafe {
            let mut fetched = 0u32;
            let mut selection = [windows::Win32::UI::TextServices::TF_SELECTION::default(); 1];
            self.context
                .GetSelection(self.ec, 0, &mut selection, &mut fetched)?;

            if fetched == 0 {
                return Ok(None);
            }

            let [selection_item] = selection;
            let range = std::mem::ManuallyDrop::into_inner(selection_item.range);

            Ok(range)
        }
    }

    #[macros::anyhow(fail_with = E_FAIL)]
    pub fn get_surrounding_text(&self, max_utf16_units: i32) -> Result<(String, String)> {
        let Some(selection) = self.get_selection_range()? else {
            return Ok((String::new(), String::new()));
        };

        unsafe {
            let left_range = selection.Clone()?;
            left_range.Collapse(self.ec, TF_ANCHOR_START)?;
            let mut shifted = 0_i32;
            left_range.ShiftStart(
                self.ec,
                -max_utf16_units.max(0),
                &mut shifted,
                std::ptr::null(),
            )?;
            let left = read_range_text(&left_range, self.ec, max_utf16_units)?;

            let right_range = selection.Clone()?;
            right_range.Collapse(self.ec, TF_ANCHOR_END)?;
            shifted = 0;
            right_range.ShiftEnd(
                self.ec,
                max_utf16_units.max(0),
                &mut shifted,
                std::ptr::null(),
            )?;
            let right = read_range_text(&right_range, self.ec, max_utf16_units)?;

            Ok((left, right))
        }
    }

    #[macros::anyhow(fail_with = E_FAIL)]
    pub fn get_caret_rect(&self) -> Result<(i32, i32, i32, i32)> {
        let range = match self.get_selection_range()? {
            Some(range) => range,
            None => self.get_insertion_range()?,
        };

        unsafe {
            range.Collapse(self.ec, TF_ANCHOR_END)?;
            let view = self.context.GetActiveView()?;
            let mut rect = RECT::default();
            let mut clipped = BOOL::default();
            view.GetTextExt(self.ec, &range, &mut rect, &mut clipped)?;
            Ok((rect.top, rect.left, rect.bottom, rect.right))
        }
    }

    #[macros::anyhow(fail_with = E_FAIL)]
    pub fn get_insertion_range(&self) -> Result<ITfRange> {
        unsafe {
            let insert_at: ITfInsertAtSelection = self.context.cast()?;

            // TF_IAS_QUERYONLY（文字は挿入せず、位置だけを問い合わせる）
            let flags = TF_IAS_QUERYONLY;

            let range = insert_at.InsertTextAtSelection(self.ec, flags, &[])?;
            Ok(range)
        }
    }

    #[macros::anyhow]
    pub fn set_selection(&self, range: &ITfRange) -> Result<()> {
        unsafe {
            let selection = [TF_SELECTION {
                range: ManuallyDrop::new(Some(range.clone())),
                style: TF_SELECTIONSTYLE {
                    ase: TF_AE_END,
                    fInterimChar: false.into(),
                },
            }];
            self.context.SetSelection(self.ec, &selection)?;
        }
        Ok(())
    }

    #[macros::anyhow(fail_with = E_FAIL)]
    pub fn start_composition(
        &self,
        range: &ITfRange,
        sink: &ITfCompositionSink,
    ) -> Result<ITfComposition> {
        unsafe {
            let context_composition: ITfContextComposition = self.context.cast()?;

            let composition = context_composition.StartComposition(self.ec, range, sink)?;

            Ok(composition)
        }
    }

    #[macros::anyhow]
    pub fn end_composition(&self, composition: &ITfComposition) -> Result<()> {
        unsafe {
            composition.EndComposition(self.ec)?;
            Ok(())
        }
    }

    #[macros::anyhow]
    pub fn set_composition_text(&self, composition: &ITfComposition, text: &str) -> Result<()> {
        unsafe {
            let range = composition.GetRange()?;
            let wide = text.to_wide_16_unpadded();
            range.SetText(self.ec, 0, &wide)?;

            range.Collapse(self.ec, TF_ANCHOR_END)?;
            self.set_selection(&range)?;
        }
        Ok(())
    }

    #[macros::anyhow]
    pub fn set_display_attribute(&self, range: &ITfRange, attr_atom: u32) -> Result<()> {
        unsafe {
            let property = self.context.GetProperty(&GUID_PROP_ATTRIBUTE)?;
            let atom: i32 = attr_atom.try_into()?;
            let variant = VARIANT::from(atom);

            property.SetValue(self.ec, range, &variant)?;
        }
        Ok(())
    }
}

#[implement(ITfEditSession)]
pub struct EditSession {
    context: ITfContext,
    callback: RefCell<Option<Box<dyn FnOnce(&ContextEditor) -> Result<()>>>>,
}

impl EditSession {
    pub fn new<F>(context: &ITfContext, callback: F) -> Self
    where
        F: FnOnce(&ContextEditor) -> Result<()> + 'static,
    {
        Self {
            context: context.clone(),
            callback: RefCell::new(Some(Box::new(callback))),
        }
    }
}

impl ITfEditSession_Impl for EditSession_Impl {
    #[macros::anyhow]
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        if let Some(callback) = self.callback.borrow_mut().take() {
            let editor = ContextEditor::new(&self.context, ec);
            callback(&editor)?;
        }
        Ok(())
    }
}

fn read_range_text(range: &ITfRange, ec: u32, max_utf16_units: i32) -> Result<String> {
    if max_utf16_units <= 0 {
        return Ok(String::new());
    }

    let mut buffer = vec![0_u16; max_utf16_units as usize];
    let mut fetched = 0_u32;
    unsafe {
        range.GetText(ec, TF_TF_MOVESTART, &mut buffer, &mut fetched)?;
    }
    buffer.truncate(fetched as usize);
    Ok(String::from_utf16_lossy(&buffer))
}

pub type CaretRect = (i32, i32, i32, i32);

pub fn read_surrounding_text(
    context: &ITfContext,
    tid: u32,
    max_utf16_units: i32,
) -> Result<(String, String, Option<CaretRect>)> {
    let result: Rc<RefCell<Option<(String, String, Option<CaretRect>)>>> =
        Rc::new(RefCell::new(None));
    let result_ref = Rc::clone(&result);

    request_read_edit_session(context, tid, move |editor| {
        let (left, right) = editor.get_surrounding_text(max_utf16_units)?;
        let caret_rect = editor.get_caret_rect().ok();
        *result_ref.borrow_mut() = Some((left, right, caret_rect));
        Ok(())
    })?;

    let surrounding_text = result.borrow_mut().take();
    surrounding_text
        .ok_or_else(|| anyhow::anyhow!("read EditSession did not run synchronously"))
}

pub fn request_read_edit_session<F>(context: &ITfContext, tid: u32, callback: F) -> Result<()>
where
    F: FnOnce(&ContextEditor) -> Result<()> + 'static,
{
    let session = EditSession::new(context, callback);
    let session_interface: ITfEditSession = session.into();
    let flags = TF_ES_SYNC | TF_ES_READ;
    unsafe {
        let hr = context.RequestEditSession(tid, &session_interface, flags)?;
        if hr.is_err() {
            return Err(anyhow::anyhow!(hr));
        }
    }
    Ok(())
}

pub fn request_edit_session<F>(context: &ITfContext, tid: u32, callback: F) -> Result<()>
where
    F: FnOnce(&ContextEditor) -> Result<()> + 'static,
{
    let session = EditSession::new(context, callback);
    let session_interface: ITfEditSession = session.into();
    let flags = TF_ES_READWRITE;
    unsafe {
        let hr = context.RequestEditSession(tid, &session_interface, flags)?;
        if hr.is_err() {
            return Err(anyhow::anyhow!(hr));
        }
    }
    Ok(())
}
