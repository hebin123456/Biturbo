use crate::ffi::types::BtBuf;
use crate::ffi::winheap::heap_free;

#[inline]
unsafe fn release_btbuf(buf: *mut BtBuf) {
    if buf.is_null() {
        return;
    }
    // Match original: only free if cap != 0.
    let cap = unsafe { (*buf).cap };
    if cap == 0 {
        return;
    }
    let ptr = std::ptr::replace(&mut (*buf).ptr, core::ptr::null_mut());
    unsafe {
        (*buf).cap = 0;
        (*buf).len = 0;
    }
    if !ptr.is_null() {
        unsafe { heap_free(ptr) };
    }
}

// These exports all share the same RVA (0x18007CA20) in the original DLL.
#[no_mangle]
pub unsafe extern "C" fn bt_release_behind_ahead_counts(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_committer_times(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_decode_image(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_highlight_syntax(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_layout_treemap(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_parse_patch(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_search_commits(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

