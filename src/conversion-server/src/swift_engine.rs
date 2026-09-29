use std::{
    ffi::{c_void, OsStr},
    fs,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    slice,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use crate::engine_worker::ConversionEngine;

const ENGINE_ABI_VERSION: u32 = 2;
const DEFAULT_ENGINE_DLL: &str = "AzooKeyDesktopEngine.dll";
const ENGINE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

type AbiVersionFn = unsafe extern "C" fn() -> u32;
type CreateFn = unsafe extern "C" fn(*const u8, u32) -> *mut c_void;
type ResponseCallback = unsafe extern "C" fn(*mut c_void, i32, *const u8, u32);
type HandleAsyncFn =
    unsafe extern "C" fn(*mut c_void, *const u8, u32, ResponseCallback, *mut c_void);
type DestroyFn = unsafe extern "C" fn(*mut c_void);

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(file_name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, proc_name: *const u8) -> *mut c_void;
}

struct DynamicLibrary {
    module: *mut c_void,
}

impl DynamicLibrary {
    fn load(path: &Path) -> Result<Self, String> {
        let wide_path: Vec<u16> = OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let module = unsafe { LoadLibraryW(wide_path.as_ptr()) };
        if module.is_null() {
            return Err(format!(
                "failed to load {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            ));
        }

        Ok(Self { module })
    }

    fn symbol(&self, name: &'static [u8]) -> Result<*mut c_void, String> {
        debug_assert_eq!(name.last(), Some(&0));
        let symbol = unsafe { GetProcAddress(self.module, name.as_ptr()) };
        if symbol.is_null() {
            return Err(format!(
                "missing engine symbol {}",
                String::from_utf8_lossy(&name[..name.len().saturating_sub(1)])
            ));
        }
        Ok(symbol)
    }
}

// Swift's Windows runtime is intentionally kept loaded for the whole conversion-server
// lifetime. Calling FreeLibrary during Swift runtime teardown can deadlock.
struct CallbackState {
    result: Mutex<Option<Result<Vec<u8>, String>>>,
    ready: Condvar,
}

impl CallbackState {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            ready: Condvar::new(),
        }
    }
}

unsafe extern "C" fn engine_response_callback(
    user_data: *mut c_void,
    status: i32,
    response_ptr: *const u8,
    response_len: u32,
) {
    if user_data.is_null() {
        return;
    }

    let state = unsafe { Arc::from_raw(user_data.cast::<CallbackState>()) };
    let result = if status != 0 {
        let detail = if response_ptr.is_null() || response_len == 0 {
            None
        } else {
            Some(String::from_utf8_lossy(unsafe {
                slice::from_raw_parts(response_ptr, response_len as usize)
            }).into_owned())
        };
        Err(match detail {
            Some(detail) => format!("desktop engine returned status {status}: {detail}"),
            None => format!("desktop engine returned status {status}"),
        })
    } else if response_len == 0 {
        Ok(Vec::new())
    } else if response_ptr.is_null() {
        Err("desktop engine returned a null response buffer".into())
    } else {
        Ok(unsafe { slice::from_raw_parts(response_ptr, response_len as usize) }.to_vec())
    };

    let mut guard = match state.result.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    *guard = Some(result);
    state.ready.notify_one();
    drop(guard);
}

pub struct SwiftEngine {
    _library: DynamicLibrary,
    context: *mut c_void,
    handle_async: HandleAsyncFn,
    destroy: DestroyFn,
}

// EngineWorker moves this object once onto one dedicated thread. Every request is
// serialized there; callbacks only copy the returned bytes and wake that thread.
unsafe impl Send for SwiftEngine {}

impl SwiftEngine {
    pub fn load_default() -> Result<Self, String> {
        let path = std::env::var_os("AZOOKEY_DESKTOP_ENGINE_DLL")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|path| path.parent().map(|parent| parent.join(DEFAULT_ENGINE_DLL)))
            })
            .ok_or_else(|| {
                "could not resolve conversion-server executable directory".to_string()
            })?;

        Self::load(&path)
    }

    fn load(path: &Path) -> Result<Self, String> {
        let library = DynamicLibrary::load(path)?;

        let abi_version: AbiVersionFn =
            unsafe { std::mem::transmute(library.symbol(b"azookey_engine_abi_version\0")?) };
        let create: CreateFn =
            unsafe { std::mem::transmute(library.symbol(b"azookey_engine_create\0")?) };
        let handle_async: HandleAsyncFn =
            unsafe { std::mem::transmute(library.symbol(b"azookey_engine_handle_async\0")?) };
        let destroy: DestroyFn =
            unsafe { std::mem::transmute(library.symbol(b"azookey_engine_destroy\0")?) };

        let actual_version = unsafe { abi_version() };
        if actual_version != ENGINE_ABI_VERSION {
            return Err(format!(
                "unsupported desktop engine ABI version {actual_version}; expected {ENGINE_ABI_VERSION}"
            ));
        }

        let configuration = Self::configuration(path)?;
        let context = unsafe { create(configuration.as_ptr(), configuration.len() as u32) };
        if context.is_null() {
            return Err("desktop engine initialization failed".into());
        }

        Ok(Self {
            _library: library,
            context,
            handle_async,
            destroy,
        })
    }

    fn configuration(dll_path: &Path) -> Result<Vec<u8>, String> {
        let data_root = std::env::var_os("AZOOKEY_ENGINE_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .map(|path| path.join("azooKey"))
            })
            .unwrap_or_else(|| std::env::temp_dir().join("azooKey"));

        let memory_directory = data_root.join("Memory");
        let shared_directory = data_root.join("Shared");
        fs::create_dir_all(&memory_directory)
            .map_err(|error| format!("failed to create engine memory directory: {error}"))?;
        fs::create_dir_all(&shared_directory)
            .map_err(|error| format!("failed to create engine shared directory: {error}"))?;

        let resources_directory = dll_path.parent().unwrap_or(Path::new("."));
        let json = serde_json::json!({
            "protocolVersion": ENGINE_ABI_VERSION,
            "applicationSupportDirectory": data_root.to_string_lossy(),
            "memoryDirectory": memory_directory.to_string_lossy(),
            "resourcesDirectory": resources_directory.to_string_lossy(),
            "sharedContainerDirectory": shared_directory.to_string_lossy(),
        });

        serde_json::to_vec(&json)
            .map_err(|error| format!("failed to encode desktop engine configuration: {error}"))
    }
}

impl ConversionEngine for SwiftEngine {
    fn handle(&mut self, request: String) -> Result<String, String> {
        if request.len() > u32::MAX as usize {
            return Err("engine request exceeds 4 GiB ABI limit".into());
        }

        let state = Arc::new(CallbackState::new());
        let callback_state = Arc::into_raw(Arc::clone(&state)) as *mut c_void;

        unsafe {
            (self.handle_async)(
                self.context,
                request.as_ptr(),
                request.len() as u32,
                engine_response_callback,
                callback_state,
            );
        }

        let guard = state
            .result
            .lock()
            .map_err(|_| "desktop engine callback lock poisoned".to_string())?;
        let (mut guard, wait) = state
            .ready
            .wait_timeout_while(guard, ENGINE_RESPONSE_TIMEOUT, |result| result.is_none())
            .map_err(|_| "desktop engine callback wait poisoned".to_string())?;

        if wait.timed_out() && guard.is_none() {
            return Err(format!(
                "desktop engine response timed out after {} seconds",
                ENGINE_RESPONSE_TIMEOUT.as_secs()
            ));
        }

        let response = guard
            .take()
            .ok_or_else(|| "desktop engine callback completed without a response".to_string())??;

        String::from_utf8(response)
            .map_err(|error| format!("desktop engine returned invalid UTF-8: {error}"))
    }
}

impl Drop for SwiftEngine {
    fn drop(&mut self) {
        if !self.context.is_null() {
            unsafe {
                (self.destroy)(self.context);
            }
            self.context = std::ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_worker::ConversionEngine;
    use shared::windows_transport::{
        WindowsTransportInputLanguage, WindowsTransportInputStyle, WindowsTransportKeyEvent,
        WindowsTransportOperation, WindowsTransportRequest, WindowsTransportResponse,
        WindowsTransportTextContext, WINDOWS_TRANSPORT_PROTOCOL_VERSION,
    };

    #[test]
    #[ignore = "requires AzooKeyDesktopEngine.dll built by the Desktop fork"]
    fn swift_engine_dll_roundtrip() {
        let configured_path = std::env::var_os("AZOOKEY_DESKTOP_ENGINE_DLL")
            .expect("AZOOKEY_DESKTOP_ENGINE_DLL must point to the built Swift DLL");
        assert!(
            Path::new(&configured_path).is_file(),
            "configured Swift engine DLL does not exist"
        );

        let mut engine = SwiftEngine::load_default().expect("failed to load Swift desktop engine");
        let session_id = "rust-windows-transport-smoke".to_string();
        let mut last_response = None;

        for (offset, character) in ["へ", "ん", "か", "ん"].into_iter().enumerate() {
            let request = WindowsTransportRequest {
                protocol_version: WINDOWS_TRANSPORT_PROTOCOL_VERSION,
                operation: WindowsTransportOperation::KeyEvent,
                session_id: session_id.clone(),
                key_event: Some(WindowsTransportKeyEvent {
                    event_id: (offset + 1) as u64,
                    core_key_code: 0,
                    characters: Some(character.to_string()),
                    characters_ignoring_modifiers: Some(character.to_string()),
                    modifier_flags: 0,
                    input_style: WindowsTransportInputStyle::Direct,
                    input_language: WindowsTransportInputLanguage::Japanese,
                    activate: offset == 0,
                    live_conversion_enabled: false,
                    enable_debug_window: false,
                    enable_suggestion: false,
                    enable_predictive_typing: false,
                    enable_typo_correction: false,
                    enable_option_direct_full_width_input: false,
                    type_back_slash: false,
                    option_direct_input_text: None,
                    visible_candidate_start_index: 0,
                    context: WindowsTransportTextContext::default(),
                }),
                candidate_index: None,
                context: None,
            };

            let response_json = engine
                .handle(serde_json::to_string(&request).unwrap())
                .expect("shared ConverterEngine key event failed");
            last_response = Some(
                serde_json::from_str::<WindowsTransportResponse>(&response_json)
                    .expect("Windows transport response was invalid"),
            );
        }

        let response = last_response.expect("no Windows transport response");
        assert!(response.handled);
        assert_eq!(response.convert_target, "へんかん");
        assert!(
            !response.candidate_window.candidates.is_empty(),
            "shared ConverterEngine returned no candidates"
        );
        assert!(
            response
                .candidate_window
                .candidates
                .iter()
                .any(|candidate| candidate.text == "変換"),
            "expected 変換 candidate, got {:?}",
            response
                .candidate_window
                .candidates
                .iter()
                .map(|candidate| candidate.text.as_str())
                .collect::<Vec<_>>()
        );

        let close_request = WindowsTransportRequest {
            protocol_version: WINDOWS_TRANSPORT_PROTOCOL_VERSION,
            operation: WindowsTransportOperation::CloseSession,
            session_id,
            key_event: None,
            candidate_index: None,
            context: None,
        };
        let close_json = engine
            .handle(serde_json::to_string(&close_request).unwrap())
            .expect("shared ConverterEngine close-session failed");
        let close_response: WindowsTransportResponse =
            serde_json::from_str(&close_json).expect("close-session response was invalid");
        assert!(close_response.handled);
        assert!(close_response.is_empty);
    }
}
