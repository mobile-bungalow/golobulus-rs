use std::path::PathBuf;

pub fn set_up_env() -> Result<(), after_effects::Error> {
    if cfg!(debug_assertions) {
        log::info!("debug mode, using system python path");
        return Ok(());
    }

    let mut lib_path = unsafe {
        PathBuf::from(os::currently_executing().ok_or_else(|| {
            log::error!("couldn't get library path!");
            after_effects::Error::Generic
        })?)
    };

    log::info!("dylib location {:?}", &lib_path);
    lib_path.pop();
    if cfg!(target_os = "macos") {
        // point towards the relative standalone python install
        // this is pretty brutal but I can't find a sanitary way to
        // dynamically set PYTHONHOME.
        lib_path.pop();
        lib_path = lib_path.join("Resources/Python");
        std::env::set_var("PYTHONHOME", lib_path);
    } else {
        println!("dylib location {:?}", &lib_path);
        golob_lib::update_dylib_search_path(lib_path.to_str().unwrap())
            .map_err(|_| {
                log::error!("Couldn't update dylib path!");
                after_effects::Error::Generic
            })?;
        // We have to set this up on the main thread on windows
        golob_lib::event_loop::get_event_loop();
    }

    Ok(())
}

// Shamelessles ripped from zluda/hiprt-sys/src/lib.rs
// MIT licensed.
#[cfg(target_os = "macos")]
mod os {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStringExt;
    use std::{
        ffi::{c_char, c_int, c_void, OsString},
        mem,
    };

    extern "C" {
        fn dladdr(addr: *mut c_void, info: *mut DlInfo) -> c_int;
    }

    pub(crate) unsafe fn currently_executing() -> Option<OsString> {
        let mut dlinfo = mem::zeroed();
        if 0 == dladdr(currently_executing as _, &mut dlinfo) {
            return None;
        }
        Some(OsString::from_vec(
            CStr::from_ptr(dlinfo.dli_fname.cast_mut())
                .to_bytes()
                .to_vec(),
        ))
    }

    #[repr(C)]
    struct DlInfo {
        dli_fname: *const c_char,
        dli_fbase: *mut c_void,
        dli_sname: *const c_char,
        dli_saddr: *mut c_void,
    }
}

#[cfg(target_os = "windows")]
mod os {
    use std::ffi::OsString;
    use std::mem;
    use std::os::windows::ffi::OsStringExt;
    use winapi::um::libloaderapi::*;

    pub(crate) unsafe fn currently_executing() -> Option<OsString> {
        let mut module = mem::zeroed();
        if 0 == GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            currently_executing as _,
            &mut module,
        ) {
            return None;
        }

        let mut path_buffer: [u16; winapi::shared::minwindef::MAX_PATH] = std::mem::zeroed();
        let length = GetModuleFileNameW(module, path_buffer.as_mut_ptr(), path_buffer.len() as u32);

        if length > 0 {
            let path = OsString::from_wide(&path_buffer[..length as usize]);
            Some(path.to_string_lossy().into_owned().into())
        } else {
            None
        }
    }
}