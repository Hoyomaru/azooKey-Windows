use std::{
    ffi::{c_void, OsStr},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr, slice,
};

use crate::engine_worker::ConversionEngine;

const ENGINE_ABI_VERSION: u32 = 1;
const DEFAULT_ENGINE_DLL: &str = "AzooKeyDesktopEngine.dll";

type AbiVersionFn = unsafe extern "C" fn() -> u32;
type CreateFn = unsafe extern "C" fn(*const u8, u32) -> *mut c_void;
type HandleFn = unsafe extern "C" fn(*mut c_void, *const u8, u32, *mut *mut u8, *mut u32) -> i32;
type FreeFn = unsafe extern "C" fn(*mut u8, u32);
type DestroyFn = unsafe extern "C" fn(*mut c_void);

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(file_name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, proc_name: *const u8) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
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

impl Drop for DynamicLibrary {
    fn drop(&mut self) {
        if !self.module.is_null() {
            unsafe {
                let _ = FreeLibrary(self.module);
            }
        }
    }
}

pub struct SwiftEngine {
    _library: DynamicLibrary,
    context: *mut c_void,
    handle: HandleFn,
    free: FreeFn,
    destroy: DestroyFn,
}

// The engine instance is moved once onto EngineWorker's dedicated thread and all
// calls are serialized there. The raw context never crosses between worker threads.
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
        let handle: HandleFn =
            unsafe { std::mem::transmute(library.symbol(b"azookey_engine_handle\0")?) };
        let free: FreeFn =
            unsafe { std::mem::transmute(library.symbol(b"azookey_engine_free\0")?) };
        let destroy: DestroyFn =
            unsafe { std::mem::transmute(library.symbol(b"azookey_engine_destroy\0")?) };

        let actual_version = unsafe { abi_version() };
        if actual_version != ENGINE_ABI_VERSION {
            return Err(format!(
                "unsupported desktop engine ABI version {actual_version}; expected {ENGINE_ABI_VERSION}"
            ));
        }

        let configuration = br#"{"protocolVersion":1}"#;
        let context = unsafe { create(configuration.as_ptr(), configuration.len() as u32) };
        if context.is_null() {
            return Err("desktop engine initialization failed".into());
        }

        Ok(Self {
            _library: library,
            context,
            handle,
            free,
            destroy,
        })
    }
}

impl ConversionEngine for SwiftEngine {
    fn handle(&mut self, request: String) -> Result<String, String> {
        if request.len() > u32::MAX as usize {
            return Err("engine request exceeds 4 GiB ABI limit".into());
        }

        let mut response_ptr: *mut u8 = ptr::null_mut();
        let mut response_len = 0_u32;
        let status = unsafe {
            (self.handle)(
                self.context,
                request.as_ptr(),
                request.len() as u32,
                &mut response_ptr,
                &mut response_len,
            )
        };

        if status != 0 {
            return Err(format!("desktop engine returned status {status}"));
        }

        if response_len == 0 {
            return Ok(String::new());
        }
        if response_ptr.is_null() {
            return Err("desktop engine returned a null response buffer".into());
        }

        let response =
            unsafe { slice::from_raw_parts(response_ptr, response_len as usize) }.to_vec();
        unsafe {
            (self.free)(response_ptr, response_len);
        }

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
            self.context = ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_worker::ConversionEngine;

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
        let request = r#"{"type":"bridge-smoke","text":"かな漢字"}"#.to_string();
        let response = engine
            .handle(request.clone())
            .expect("Swift engine bridge request failed");

        assert_eq!(response, request);

        let conversion_request =
            r#"{"type":"conversion-smoke","text":"へんかん","inputStyle":"direct"}"#.to_string();
        let conversion_response = engine
            .handle(conversion_request.clone())
            .expect("Swift dictionary conversion failed");

        assert_ne!(conversion_response, conversion_request);
        assert!(
            conversion_response.contains(r#""candidates":["#),
            "conversion response did not contain candidates: {conversion_response}"
        );
        assert!(
            !conversion_response.contains(r#""candidates":[]"#),
            "dictionary conversion returned no candidates"
        );
    }
}
