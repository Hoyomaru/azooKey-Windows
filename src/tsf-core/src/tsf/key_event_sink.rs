use std::{cell::Cell, rc::Rc};

use shared::windows_transport::{
    WindowsTransportInputLanguage, WindowsTransportInputStyle, WindowsTransportOperation,
    WindowsTransportRequest, WindowsTransportResponse, WindowsTransportTextContext,
    WINDOWS_TRANSPORT_PROTOCOL_VERSION,
};
use windows::{
    core::GUID,
    Win32::{
        Foundation::{BOOL, LPARAM, WPARAM},
        UI::TextServices::{ITfComposition, ITfCompositionSink, ITfContext, ITfKeyEventSink_Impl},
    },
};

use crate::{
    engine::key_event::NormalizedKeyEvent,
    globals::GUID_DISPLAY_ATTRIBUTE,
};

use super::{
    edit_session::{read_surrounding_text, request_edit_session},
    text_service::TextService_Impl,
};

impl TextService_Impl {
    fn is_actionable_key(key: &NormalizedKeyEvent) -> bool {
        key.core_key_code != 0
            || key.characters.is_some()
            || key.characters_ignoring_modifiers.is_some()
    }

    fn build_engine_request(
        &self,
        context: &ITfContext,
        key: NormalizedKeyEvent,
        text_context: WindowsTransportTextContext,
    ) -> Option<WindowsTransportRequest> {
        let contexts = self.contexts.borrow();
        let state = contexts.find(context)?;

        let activate = !state.is_engine_session_open();
        let event = key.into_transport(
            state.next_engine_event_id(),
            WindowsTransportInputStyle::DefaultRomanToKana,
            WindowsTransportInputLanguage::Japanese,
            activate,
            text_context,
        );

        Some(WindowsTransportRequest {
            protocol_version: WINDOWS_TRANSPORT_PROTOCOL_VERSION,
            operation: WindowsTransportOperation::KeyEvent,
            session_id: state.engine_session_id().to_string(),
            key_event: Some(event),
            candidate_index: None,
            context: None,
        })
    }

    fn mark_engine_session_open(&self, context: &ITfContext) {
        if let Some(state) = self.contexts.borrow().find(context) {
            state.mark_engine_session_open();
        }
    }

    fn apply_engine_response(
        &self,
        context: &ITfContext,
        tid: u32,
        response: &WindowsTransportResponse,
    ) -> anyhow::Result<()> {
        let inserted_text = response
            .effects
            .iter()
            .filter(|effect| effect.kind == "insertText")
            .filter_map(|effect| effect.text.as_deref())
            .collect::<String>();

        let marked_text = response
            .marked_text
            .elements
            .iter()
            .map(|element| element.content.as_str())
            .collect::<String>();

        let existing_composition = self
            .contexts
            .borrow()
            .find(context)
            .and_then(|state| state.take_composition());

        if marked_text.is_empty() {
            if existing_composition.is_none() && inserted_text.is_empty() {
                return Ok(());
            }

            request_edit_session(context, tid, move |editor| {
                if let Some(composition) = existing_composition {
                    editor.end_composition(&composition)?;
                }
                if !inserted_text.is_empty() {
                    editor.insert_text(&inserted_text)?;
                }
                Ok(())
            })?;
            return Ok(());
        }

        let composition_sink: ITfCompositionSink = self.this()?;

        let atom_map = self.display_attribute_atom.take();
        let attr_atom = atom_map.get(&GUID_DISPLAY_ATTRIBUTE).copied();
        self.display_attribute_atom.set(atom_map);

        let result_composition: Rc<Cell<Option<ITfComposition>>> = Rc::new(Cell::new(None));
        let result_ref = Rc::clone(&result_composition);

        request_edit_session(context, tid, move |editor| {
            if !inserted_text.is_empty() {
                editor.insert_text(&inserted_text)?;
            }

            let composition = match existing_composition {
                Some(composition) => composition,
                None => {
                    let range = editor.get_insertion_range()?;
                    editor.start_composition(&range, &composition_sink)?
                }
            };

            editor.set_composition_text(&composition, &marked_text)?;

            if let Some(atom) = attr_atom {
                let range = unsafe { composition.GetRange()? };
                editor.set_display_attribute(&range, atom)?;
            }

            result_ref.set(Some(composition));
            Ok(())
        })?;

        if let Some(composition) = result_composition.take() {
            if let Some(state) = self.contexts.borrow().find(context) {
                state.set_composition(Some(composition));
            }
        }

        Ok(())
    }
}

// sink (aka event listener) for key events
// 返り値はS_OKのみであることに注意
impl ITfKeyEventSink_Impl for TextService_Impl {
    #[macros::anyhow(ignore_with = false.into())]
    fn OnTestKeyDown(
        &self,
        pic: Option<&ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Result<BOOL> {
        let Some(context) = pic else {
            return Ok(false.into());
        };

        let key = match NormalizedKeyEvent::from_windows(wparam, lparam) {
            Ok(key) => key,
            Err(_) => return Ok(false.into()),
        };
        Ok(Self::is_actionable_key(&key).into())
    }

    #[macros::anyhow(ignore_with = false.into())]
    fn OnKeyDown(
        &self,
        pic: Option<&ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Result<BOOL> {
        let context = match pic {
            Some(context) => context,
            None => return Ok(false.into()),
        };

        let tid = self.tid.get();
        if tid == 0 {
            return Ok(false.into());
        }

        let key = match NormalizedKeyEvent::from_windows(wparam, lparam) {
            Ok(key) if Self::is_actionable_key(&key) => key,
            Ok(_) => return Ok(false.into()),
            Err(error) => {
                tracing::warn!("Failed to normalize key event: {error:?}");
                return Ok(false.into());
            }
        };

        // Capture surrounding text in a short synchronous READ session, then release
        // every TSF range before starting IPC or conversion work.
        let (left, right) = match read_surrounding_text(context, tid, 200) {
            Ok(context) => context,
            Err(error) => {
                tracing::debug!("Surrounding text unavailable: {error:?}");
                (String::new(), String::new())
            }
        };
        let text_context = WindowsTransportTextContext {
            left: (!left.is_empty()).then_some(left),
            right: (!right.is_empty()).then_some(right),
        };

        let request = match self.build_engine_request(context, key, text_context) {
            Some(request) => request,
            None => return Ok(false.into()),
        };

        // IPC is deliberately completed before requesting a TSF write EditSession.
        let response = match crate::client::handle(&request) {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!("Shared conversion engine unavailable: {error:?}");
                return Ok(false.into());
            }
        };

        self.mark_engine_session_open(context);
        crate::ui_client::publish_response_best_effort(&response);

        if response
            .effects
            .iter()
            .any(|effect| effect.kind == "fallthroughToApplication")
            || !response.handled
        {
            return Ok(false.into());
        }

        if let Err(error) = self.apply_engine_response(context, tid, &response) {
            tracing::error!("Failed to apply shared engine response: {error:?}");
            return Ok(false.into());
        }

        Ok(true.into())
    }

    #[macros::anyhow(ignore_with = false.into())]
    fn OnTestKeyUp(
        &self,
        _pic: Option<&ITfContext>,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        Ok(false.into())
    }

    #[macros::anyhow(ignore_with = false.into())]
    fn OnKeyUp(&self, _pic: Option<&ITfContext>, _wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        Ok(false.into())
    }

    #[macros::anyhow(ignore_with = false.into())]
    fn OnPreservedKey(&self, _pic: Option<&ITfContext>, _rguid: *const GUID) -> Result<BOOL> {
        Ok(true.into())
    }

    #[macros::anyhow]
    fn OnSetFocus(&self, _fforeground: BOOL) -> Result<()> {
        Ok(())
    }
}
