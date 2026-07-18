//! # Markdown 转 HTML
//!
//! 提供 [`bt_md_to_html`] / [`bt_release_md_to_html`]：
//! 使用 `markdown` crate 把 UTF-8 Markdown 文本渲染为 HTML 字符串，
//! 通过进程堆分配返回 NUL 终止的 C 字符串。

use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::{heap_alloc, heap_free_u8};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

/// 把 Markdown 文本渲染为 HTML。
///
/// # 参数
/// - `md_utf8`：NUL 终止的 UTF-8 Markdown 文本；为 `null` 返回错误。
/// - `out_html`：输出 `*mut c_char`，调用前可未初始化，调用后指向 NUL 终止的 HTML。
///
/// # 返回值
/// - `0`：成功。
/// - `1`：参数非法、Markdown 非 UTF-8 或内存不足。
///
/// # 内存所有权
/// 输出的 HTML 字符串通过进程堆分配，必须用 [`bt_release_md_to_html`] 释放。
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

/// 释放 [`bt_md_to_html`] 返回的 HTML 字符串。
///
/// 通过 `*mut *mut c_char` 入参：会把 `*out_html` 取出并置 `null`，
/// 释放前会把首字节置 `0` 作为“毒化”标志（与原版 DLL 一致）。
/// 传入 `null` 安全。
///
/// # 内存所有权
/// 仅可释放由 [`bt_md_to_html`] 返回的字符串。
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

