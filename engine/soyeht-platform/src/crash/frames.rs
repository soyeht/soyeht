//! The frames of the calling thread's stack as `module+0xoffset`, innermost first.
//!
//! A shipped program has no symbols (`strip = "symbols"`), so a frame is the module's file name
//! and the frame's offset from the module's load address, which does not change between runs
//! (ASLR moves the base, not the offset). Whoever has the unstripped build of that version turns
//! it into a name (`atos -l`, `addr2line`). The folder of the module is never recorded.

#[cfg(all(unix, not(target_env = "musl")))]
mod system {
    use std::{ffi::CStr, mem::MaybeUninit, os::raw::c_void};

    pub(super) fn capture(limit: usize) -> Vec<String> {
        let mut addresses: Vec<*mut c_void> = vec![std::ptr::null_mut(); limit];
        let count = i32::try_from(limit).unwrap_or(i32::MAX);
        // SAFETY: the buffer holds `count` pointers and outlives the call.
        let found = unsafe { libc::backtrace(addresses.as_mut_ptr(), count) };
        addresses.truncate(usize::try_from(found).unwrap_or(0));
        addresses.into_iter().filter_map(describe).collect()
    }

    fn describe(address: *mut c_void) -> Option<String> {
        let mut info = MaybeUninit::<libc::Dl_info>::zeroed();
        // SAFETY: `dladdr` fills the structure it is given; it reads no memory of `address`.
        if unsafe { libc::dladdr(address, info.as_mut_ptr()) } == 0 {
            return None;
        }
        // SAFETY: `dladdr` returned non-zero, so the structure is filled.
        let info = unsafe { info.assume_init() };
        if info.dli_fname.is_null() {
            return None;
        }
        // SAFETY: `dli_fname` is a NUL-terminated path owned by the loader.
        let path = unsafe { CStr::from_ptr(info.dli_fname) }.to_string_lossy();
        let module = path.rsplit('/').next().unwrap_or_default();
        let offset = (address as usize).wrapping_sub(info.dli_fbase as usize);
        Some(format!("{module}+{offset:#x}"))
    }
}

#[cfg(windows)]
mod system {
    use std::os::raw::c_void;
    use windows_sys::Win32::{
        Foundation::HMODULE,
        System::{
            Diagnostics::Debug::RtlCaptureStackBackTrace,
            LibraryLoader::{
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW,
                GetModuleHandleExW,
            },
        },
    };

    /// Room for a module path.
    const PATH_UNITS: usize = 1_024;

    pub(super) fn capture(limit: usize) -> Vec<String> {
        let mut addresses: Vec<*mut c_void> = vec![std::ptr::null_mut(); limit];
        let count = u32::try_from(limit).unwrap_or(u32::MAX);
        // SAFETY: the buffer holds `count` pointers and outlives the call.
        let found = unsafe {
            RtlCaptureStackBackTrace(0, count, addresses.as_mut_ptr(), std::ptr::null_mut())
        };
        addresses.truncate(usize::from(found));
        addresses.into_iter().filter_map(describe).collect()
    }

    fn describe(address: *mut c_void) -> Option<String> {
        let mut module: HMODULE = std::ptr::null_mut();
        // SAFETY: the flags make the address the thing looked up, and `module` is written.
        let found = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                address.cast(),
                &mut module,
            )
        };
        if found == 0 {
            return None;
        }
        let mut path = [0_u16; PATH_UNITS];
        // SAFETY: the buffer holds `PATH_UNITS` units and outlives the call.
        let length = unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), PATH_UNITS as u32) };
        if length == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(path.get(..length as usize)?);
        let name = path.rsplit(['\\', '/']).next().unwrap_or_default();
        let offset = (address as usize).wrapping_sub(module as usize);
        Some(format!("{name}+{offset:#x}"))
    }
}

#[cfg(not(any(windows, all(unix, not(target_env = "musl")))))]
mod system {
    pub(super) fn capture(_limit: usize) -> Vec<String> {
        Vec::new()
    }
}

/// At most `limit` frames of the calling thread, without the first, which is this function.
pub fn capture(limit: usize) -> Vec<String> {
    system::capture(limit + 1).into_iter().skip(1).collect()
}
