use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::{heap_alloc, heap_free_u8};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

/// Convert markdown (UTF-8) to HTML.
///
/// Signature confirmed by disassembly:
/// - `rcx`: markdown C string
/// - `rdx`: out pointer to `char*`
/// - returns `i32` status (0 on success)
#[no_mangle]
pub unsafe extern "C" fn bt_md_to_html(md_utf8: *const c_char, out_html: *mut *mut c_char) -> c_int {
    if out_html.is_null() {
        set_last_error_str("invalid output pointer");
        return 1;
    }
    unsafe { *out_html = core::ptr::null_mut() };

    if md_utf8.is_null() {
        set_last_error_str("invalid markdown pointer");
        return 1;
    }

    let md_bytes = unsafe { CStr::from_ptr(md_utf8) }.to_bytes();
    let md_str = match std::str::from_utf8(md_bytes) {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8");
            return 1;
        }
    };

    let html = markdown::to_html(md_str);
    let mut bytes = html.into_bytes();
    bytes.push(0);

    let dst = unsafe { heap_alloc(bytes.len()) };
    if dst.is_null() {
        set_last_error_str("insufficient memory");
        return 1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
        *out_html = dst as *mut c_char;
    }
    0
}

/// Release HTML returned by `bt_md_to_html`.
///
/// Signature matches the original: `char**`.
#[no_mangle]
pub unsafe extern "C" fn bt_release_md_to_html(out_html: *mut *mut c_char) {
    if out_html.is_null() {
        return;
    }
    let p = std::ptr::replace(out_html, core::ptr::null_mut()) as *mut u8;
    if p.is_null() {
        return;
    }
    // Best-effort poison like the original.
    unsafe { *p = 0 };
    unsafe { heap_free_u8(p) };
}

