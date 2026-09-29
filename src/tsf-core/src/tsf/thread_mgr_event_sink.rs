use windows::Win32::UI::TextServices::{ITfContext, ITfDocumentMgr, ITfThreadMgrEventSink_Impl};

use super::text_service::TextService_Impl;

// テキストボックスのフォーカスの変更などを取り扱う
impl ITfThreadMgrEventSink_Impl for TextService_Impl {
    #[macros::anyhow]
    fn OnInitDocumentMgr(&self, _pdim: Option<&ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    #[macros::anyhow]
    fn OnUninitDocumentMgr(&self, _pdim: Option<&ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    #[macros::anyhow]
    fn OnSetFocus(
        &self,
        focus: Option<&ITfDocumentMgr>,
        _prevfocus: Option<&ITfDocumentMgr>,
    ) -> Result<()> {
        self.update_lang_bar()?;

        // if focus is changed, the text layout sink should be updated
        if let Some(focus) = focus {
            self.advise_text_layout_sink(focus.clone())?;
        }
        Ok(())
    }

    #[macros::anyhow]
    fn OnPushContext(&self, pic: Option<&ITfContext>) -> Result<()> {
        if let Some(ctx) = pic {
            self.contexts.borrow_mut().register(ctx);
        }
        Ok(())
    }

    #[macros::anyhow]
    fn OnPopContext(&self, pic: Option<&ITfContext>) -> Result<()> {
        if let Some(ctx) = pic {
            let session_id = {
                let contexts = self.contexts.borrow();
                contexts
                    .find(ctx)
                    .filter(|state| state.is_engine_session_open())
                    .map(|state| state.engine_session_id().to_string())
            };

            if let Some(session_id) = session_id {
                crate::client::close_session_best_effort(session_id);
            }

            self.contexts.borrow_mut().unregister(ctx)?;
            crate::ui_client::hide_best_effort();
        }
        Ok(())
    }
}
