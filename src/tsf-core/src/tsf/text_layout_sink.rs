use windows::{
    core::Interface as _,
    Win32::UI::TextServices::{
        ITfContextView, ITfDocumentMgr, ITfSource, ITfTextLayoutSink, ITfTextLayoutSink_Impl,
        TfLayoutCode,
    },
};

use anyhow::Result;

use super::{text_service::TextService, text_service::TextService_Impl};

impl ITfTextLayoutSink_Impl for TextService_Impl {
    // テキストの位置が変化したこっと期の動作を指定するイベントリスナー的なもの
    // ただし、Windows Storeのアプリ（メモ帳とか）では呼ばれないっぽいので注意が必要
    #[macros::anyhow]
    fn OnLayoutChange(
        &self,
        pic: Option<&windows::Win32::UI::TextServices::ITfContext>,
        _lcode: TfLayoutCode,
        _pview: Option<&ITfContextView>,
    ) -> Result<()> {
        let Some(context) = pic else {
            return Ok(());
        };
        let tid = self.tid.get();
        if tid == 0 {
            return Ok(());
        }

        if let Ok((_, _, Some(caret_rect))) =
            super::edit_session::read_surrounding_text(context, tid, 0)
        {
            crate::ui_client::publish_position_best_effort(caret_rect);
        }
        Ok(())
    }
}

impl TextService {
    pub fn advise_text_layout_sink(&self, doc_mgr: ITfDocumentMgr) -> Result<()> {
        unsafe {
            let context = doc_mgr.GetTop()?;
            let this_layout_sink = self.this::<ITfTextLayoutSink>()?;
            let cookie = context
                .cast::<ITfSource>()?
                .AdviseSink(&ITfTextLayoutSink::IID, &this_layout_sink)?;

            self.contexts
                .borrow()
                .set_text_layout_cookie(&context, cookie);

            Ok(())
        }
    }
}
