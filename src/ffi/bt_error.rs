use crate::ffi::error::take_last_error_bytes;

/// Copy and consume the last error message for the current thread.
///
/// Observed behavior in the original DLL:
/// - Returns 0 if no message is available
/// - Returns message length (positive) on success
/// - Returns `-(len + 2)` if the provided buffer is too small (needs `len + 1`)
#[no_mangle]
pub unsafe extern "C" fn bt_get_last_error_message(out: *mut u8, out_len: usize) -> isize {
    let Some(msg) = take_last_error_bytes() else {
        if !out.is_null() && out_len > 0 {
            unsafe { *out = 0 };
        }
        return 0;
    };

    let msg_len = msg.len();
    if out.is_null() || out_len == 0 {
        return -(msg_len as isize) - 2;
    }

    if out_len <= msg_len {
        return -(msg_len as isize) - 2;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(msg.as_ptr(), out, msg_len);
        *out.add(msg_len) = 0;
        if out_len > msg_len + 1 {
            core::ptr::write_bytes(out.add(msg_len + 1), 0, out_len - (msg_len + 1));
        }
    }

    msg_len as isize
}

