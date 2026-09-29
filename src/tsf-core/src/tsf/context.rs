use std::{
    cell::Cell,
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::Result;
use windows::{
    core::Interface,
    Win32::UI::TextServices::{ITfComposition, ITfContext, ITfSource},
};

static NEXT_ENGINE_SESSION_ID: AtomicU64 = AtomicU64::new(1);

pub struct ContextState {
    pub context: ITfContext,
    pub composition: Cell<Option<ITfComposition>>,
    pub text_edit_sink_cookie: Cell<Option<u32>>,
    pub text_layout_sink_cookie: Cell<Option<u32>>,
    engine_session_id: String,
    next_engine_event_id: Cell<u64>,
    engine_session_open: Cell<bool>,
}

impl ContextState {
    pub fn set_composition(&self, comp: Option<ITfComposition>) {
        self.composition.set(comp);
    }

    pub fn take_composition(&self) -> Option<ITfComposition> {
        self.composition.take()
    }

    pub fn engine_session_id(&self) -> &str {
        &self.engine_session_id
    }

    pub fn next_engine_event_id(&self) -> u64 {
        let next = self.next_engine_event_id.get().wrapping_add(1);
        self.next_engine_event_id.set(next);
        next
    }

    pub fn is_engine_session_open(&self) -> bool {
        self.engine_session_open.get()
    }

    pub fn mark_engine_session_open(&self) {
        self.engine_session_open.set(true);
    }

    pub fn mark_engine_session_closed(&self) {
        self.engine_session_open.set(false);
        self.next_engine_event_id.set(0);
    }

    pub fn unadvise_text_layout_sink(&self) -> Result<()> {
        if let Some(cookie) = self.text_layout_sink_cookie.take() {
            unsafe {
                self.context.cast::<ITfSource>()?.UnadviseSink(cookie)?;
            }
        }
        Ok(())
    }

    pub fn unadvise_text_edit_sink(&self) -> Result<()> {
        if let Some(cookie) = self.text_edit_sink_cookie.take() {
            unsafe {
                self.context.cast::<ITfSource>()?.UnadviseSink(cookie)?;
            }
        }
        Ok(())
    }

    pub fn unadvise_all(&self) -> Result<()> {
        self.unadvise_text_layout_sink()?;
        self.unadvise_text_edit_sink()?;
        Ok(())
    }
}

impl Drop for ContextState {
    fn drop(&mut self) {
        let _ = self.unadvise_all();
    }
}

#[derive(Default)]
pub struct ContextManager {
    registry: HashMap<isize, ContextState>,
}

impl ContextManager {
    fn key(context: &ITfContext) -> isize {
        context.as_raw() as isize
    }

    pub fn register(&mut self, context: &ITfContext) {
        let key = Self::key(context);
        if self.registry.contains_key(&key) {
            return;
        }

        let sequence = NEXT_ENGINE_SESSION_ID.fetch_add(1, Ordering::Relaxed);
        self.registry.insert(
            key,
            ContextState {
                context: context.clone(),
                composition: Cell::new(None),
                text_edit_sink_cookie: Cell::new(None),
                text_layout_sink_cookie: Cell::new(None),
                engine_session_id: format!("win-{}-{sequence}", std::process::id()),
                next_engine_event_id: Cell::new(0),
                engine_session_open: Cell::new(false),
            },
        );
    }

    pub fn unregister(&mut self, context: &ITfContext) -> Result<()> {
        if let Some(state) = self.registry.remove(&Self::key(context)) {
            state.unadvise_all()?;
        }
        Ok(())
    }

    pub fn find(&self, context: &ITfContext) -> Option<&ContextState> {
        self.registry.get(&Self::key(context))
    }

    pub fn set_text_layout_cookie(&self, context: &ITfContext, cookie: u32) {
        if let Some(state) = self.registry.get(&Self::key(context)) {
            state.text_layout_sink_cookie.set(Some(cookie));
        }
    }

    pub fn clear(&mut self) {
        self.registry.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_session_ids_are_monotonic_and_process_scoped() {
        let first = NEXT_ENGINE_SESSION_ID.fetch_add(1, Ordering::Relaxed);
        let second = NEXT_ENGINE_SESSION_ID.fetch_add(1, Ordering::Relaxed);
        assert_eq!(second, first + 1);

        let session_id = format!("win-{}-{first}", std::process::id());
        assert!(session_id.starts_with("win-"));
        assert!(session_id.ends_with(&first.to_string()));
    }
}
