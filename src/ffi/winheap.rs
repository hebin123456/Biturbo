//! # FFI 内存分配器（跨平台）
//!
//! 提供 Biturbo 所有 FFI out 参数缓冲区的统一分配/释放接口。
//!
//! # 平台实现
//! - **Windows**：使用 `kernel32` 的 `GetProcessHeap` / `HeapAlloc` / `HeapFree`，
//!   保持与原版 `biturbo.dll` 完全一致的内存模型。
//! - **Linux / macOS**：使用 libc 的 `malloc` / `free`。
//!
//! # 内存所有权约定
//! - 所有 `bt_*` 函数返回给调用方的缓冲区都必须经本模块分配；
//! - 调用方释放时必须使用对应的 `bt_release_*` 函数（最终调用 [`heap_free`]），
//!   **绝对不能**与 C 运行时的 `free` / `delete` / `CoTaskMemFree` 等混用，
//!   否则会因堆不一致导致损坏。

use core::ffi::c_void;
use std::os::raw::c_char;

// ---------------------------------------------------------------------------
// Windows：kernel32 进程堆（保留原版 ABI）
// ---------------------------------------------------------------------------

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetProcessHeap() -> *mut c_void;
    fn HeapAlloc(hHeap: *mut c_void, dwFlags: u32, dwBytes: usize) -> *mut c_void;
    fn HeapFree(hHeap: *mut c_void, dwFlags: u32, lpMem: *mut c_void) -> i32;
}

#[cfg(windows)]
unsafe fn sys_alloc(bytes: usize) -> *mut u8 {
    if bytes == 0 {
        return core::ptr::null_mut();
    }
    let heap = unsafe { GetProcessHeap() };
    if heap.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { HeapAlloc(heap, 0, bytes) as *mut u8 }
}

#[cfg(windows)]
unsafe fn sys_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let heap = unsafe { GetProcessHeap() };
    if heap.is_null() {
        return;
    }
    unsafe {
        let _ = HeapFree(heap, 0, ptr);
    }
}

// ---------------------------------------------------------------------------
// Linux / macOS：libc malloc/free
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

#[cfg(not(windows))]
unsafe fn sys_alloc(bytes: usize) -> *mut u8 {
    if bytes == 0 {
        return core::ptr::null_mut();
    }
    unsafe { malloc(bytes) as *mut u8 }
}

#[cfg(not(windows))]
unsafe fn sys_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    unsafe { free(ptr) }
}

// ---------------------------------------------------------------------------
// 跨平台公共 API
// ---------------------------------------------------------------------------

/// 分配 `bytes` 字节内存。
///
/// - `bytes == 0` 时返回 `null`（与原版 DLL 一致）；
/// - 获取堆失败时也返回 `null`。
///
/// # 内存所有权
/// 返回的指针必须由 [`heap_free`] 释放，不能使用 C 的 `free`。
pub unsafe fn heap_alloc(bytes: usize) -> *mut u8 {
    unsafe { sys_alloc(bytes) }
}

/// 释放由 [`heap_alloc`] 分配的内存。
///
/// 传入 `null` 是安全的（直接返回）。
///
/// # 内存所有权
/// 仅可释放由本模块函数（`heap_alloc` / `heap_alloc_c_string` 等）返回的指针，
/// 释放 C `malloc` / Rust 全局分配器返回的指针会损坏堆。
pub unsafe fn heap_free(ptr: *mut c_void) {
    unsafe { sys_free(ptr) }
}

/// 等价于 [`heap_free`]，但接受 `*mut u8` 以便在 FFI 边界避免类型转换。
///
/// 语义与 [`heap_free`] 完全一致：仅可释放由本模块分配的字节缓冲区。
pub unsafe fn heap_free_u8(ptr: *mut u8) {
    unsafe { heap_free(ptr as *mut c_void) }
}

/// 分配并以 `\0` 终止的 C 字符串副本。
///
/// # 参数
/// - `s`: 任意 UTF-8 字符串切片；其字节会被原样复制并追加一个 `\0`。
///
/// # 返回
/// - 成功：指向 NUL 终止字符串的 `*mut c_char`，需用 [`heap_free`] 释放。
/// - 分配失败时返回 `null`。
///
/// # 内存所有权
/// 返回的指针归调用方所有，必须使用 [`heap_free`] 释放。
pub unsafe fn heap_alloc_c_string(s: &str) -> *mut c_char {
    let bytes = s.as_bytes();
    let n = bytes.len() + 1;
    let p = unsafe { heap_alloc(n) };
    if p.is_null() {
        return core::ptr::null_mut();
    }
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        *p.add(bytes.len()) = 0;
    }
    p as *mut c_char
}
