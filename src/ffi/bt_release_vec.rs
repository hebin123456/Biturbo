//! # 通用 `BtBuf` 释放函数集
//!
//! 原版 `biturbo.dll` 中多个 `bt_release_*` 函数共享同一 RVA，
//! 内部实现等价于“按 `cap != 0` 判断后用进程堆 `HeapFree` 释放 `ptr`”。
//! 本模块以 [`release_btbuf`] 为共享实现，对外暴露各具语义名称的导出函数。

use crate::ffi::types::BtBuf;
use crate::ffi::winheap::heap_free;

#[inline]
unsafe fn release_btbuf(buf: *mut BtBuf) {
    if buf.is_null() {
        return;
    }
    // Match original: only free if cap != 0, and do not mutate the caller's
    // buffer fields after release.
    let cap = unsafe { (*buf).cap };
    if cap == 0 {
        return;
    }
    let ptr = unsafe { (*buf).ptr };
    if !ptr.is_null() {
        unsafe { heap_free(ptr) };
    }
}

// These exports all share the same RVA (0x18007CA20) in the original DLL.

/// 释放 [`crate::ffi::bt_commits::bt_get_behind_ahead_counts`] 返回的缓冲区。
///
/// # 内存所有权
/// `buf` 必须是由对应 `bt_get_*` 函数填充的 `BtBuf` 指针；
/// 释放后调用方不应再访问其中的 `ptr`。传入 `null` 安全。
#[no_mangle]
pub unsafe extern "C" fn bt_release_behind_ahead_counts(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

/// 释放 [`crate::ffi::bt_committer_times::bt_get_committer_times`] 返回的缓冲区。
///
/// # 内存所有权
/// `buf` 必须由对应的 `bt_get_committer_times` 填充；传入 `null` 安全。
#[no_mangle]
pub unsafe extern "C" fn bt_release_committer_times(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

/// 释放 [`crate::ffi::bt_decode_image::bt_decode_image`] 返回的缓冲区。
///
/// # 内存所有权
/// `buf` 必须由对应的 `bt_decode_image` 填充；传入 `null` 安全。
#[no_mangle]
pub unsafe extern "C" fn bt_release_decode_image(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

/// 释放 [`crate::ffi::bt_highlight_syntax::bt_highlight_syntax`] 返回的缓冲区。
///
/// # 内存所有权
/// `buf` 必须由对应的 `bt_highlight_syntax` 填充；传入 `null` 安全。
#[no_mangle]
pub unsafe extern "C" fn bt_release_highlight_syntax(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

/// 释放 [`crate::ffi::bt_layout_treemap::bt_layout_treemap`] 返回的缓冲区。
///
/// # 内存所有权
/// `buf` 必须由对应的 `bt_layout_treemap` 填充；传入 `null` 安全。
#[no_mangle]
pub unsafe extern "C" fn bt_release_layout_treemap(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

/// 释放 [`crate::ffi::bt_parse_patch::bt_parse_patch`] 返回的缓冲区。
///
/// # 内存所有权
/// `buf` 必须由对应的 `bt_parse_patch` 填充；传入 `null` 安全。
#[no_mangle]
pub unsafe extern "C" fn bt_release_parse_patch(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

/// 释放 [`crate::ffi::bt_commits::bt_search_commits`] 返回的缓冲区。
///
/// # 内存所有权
/// `buf` 必须由对应的 `bt_search_commits` 填充；传入 `null` 安全。
#[no_mangle]
pub unsafe extern "C" fn bt_release_search_commits(buf: *mut BtBuf) {
    unsafe { release_btbuf(buf) }
}

#[cfg(all(test, windows))]
mod tests {
    // 这些测试仅在 Windows 平台下编译运行（依赖 kernel32 的 HeapFree）。
    // Linux 沙箱无法链接 kernel32，故此模块在 Linux 上为空。
    use super::*;

    #[test]
    fn release_btbuf_null_is_safe() {
        // 传入 null 指针应直接返回，不触发任何解引用
        unsafe { release_btbuf(core::ptr::null_mut()) };
    }

    #[test]
    fn release_btbuf_zero_cap_is_safe() {
        // cap == 0 时不应尝试 HeapFree（即使 ptr 非 null）
        let mut buf = BtBuf { ptr: core::ptr::null_mut(), len: 0, cap: 0 };
        unsafe { release_btbuf(&mut buf) };
    }

    #[test]
    fn all_public_release_functions_accept_null() {
        // 每个公开的 release 函数都应安全接受 null
        unsafe {
            bt_release_behind_ahead_counts(core::ptr::null_mut());
            bt_release_committer_times(core::ptr::null_mut());
            bt_release_decode_image(core::ptr::null_mut());
            bt_release_highlight_syntax(core::ptr::null_mut());
            bt_release_layout_treemap(core::ptr::null_mut());
            bt_release_parse_patch(core::ptr::null_mut());
            bt_release_search_commits(core::ptr::null_mut());
        }
    }
}
