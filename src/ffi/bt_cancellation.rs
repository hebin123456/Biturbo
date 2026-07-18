//! # 取消令牌（轻量）
//!
//! 提供 [`bt_new_cancellation_token`] / [`bt_cancel_cancellation_token`] /
//! [`bt_release_cancellation_token`]：一种单字节取消标志，用于让长任务
//! （如 [`crate::ffi::bt_commits`] 中的提交遍历）在外部触发时尽早退出。
//!
//! 实现复刻原版 `biturbo.dll`：分配 1 字节并初始化为 `0`，取消时写为 `1`。
//! 所有“活动”令牌指针都登记在全局集合中，跨 FFI 边界可校验有效性。

use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::{heap_alloc, heap_free_u8};
use std::sync::OnceLock;
use std::sync::Mutex;
use std::collections::HashSet;

static ACTIVE_TOKENS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

fn get_active_tokens() -> &'static Mutex<HashSet<usize>> {
    ACTIVE_TOKENS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 把一个令牌指针登记到全局活动集合中。
///
/// 用于 [`bt_new_cancellation_token`] 内部建立“已发放令牌”索引，
/// 后续 `bt_cancel_*` / `bt_release_*` 据此校验指针有效性。
pub fn register_token(ptr: *mut u8) {
    if !ptr.is_null() {
        let mut lock = get_active_tokens().lock().unwrap();
        lock.insert(ptr as usize);
    }
}

/// 从全局集合中移除令牌并把 `*token` 置 `null`。
///
/// 返回被取出的内部指针，便于调用方继续 `heap_free`；
/// 若 `token` 为 `null`、`*token` 为 `null` 或不在集合中，则返回 `null`。
pub fn unregister_and_null_token(token: *mut *mut u8) -> *mut u8 {
    if token.is_null() {
        return core::ptr::null_mut();
    }
    let mut lock = get_active_tokens().lock().unwrap();
    let p = unsafe { *token };
    if p.is_null() {
        return core::ptr::null_mut();
    }
    if lock.remove(&(p as usize)) {
        unsafe { *token = core::ptr::null_mut() };
        p
    } else {
        core::ptr::null_mut()
    }
}

/// 判断给定的令牌当前是否“有效且已被取消”。
///
/// 用于长任务在关键路径上做协作式取消检查：
/// - `token_ptr_ptr` 为 `null` 或 `*token_ptr_ptr` 为 `null` → 返回 `false`；
/// - 指针不在活动集合中 → 返回 `false`；
/// - 否则返回 `*inner != 0`（即“已被取消”）。
pub fn is_token_active_and_canceled(token_ptr_ptr: *mut *mut u8) -> bool {
    if token_ptr_ptr.is_null() {
        return false;
    }
    let lock = get_active_tokens().lock().unwrap();
    let inner = unsafe { *token_ptr_ptr };
    if inner.is_null() {
        return false;
    }
    if lock.contains(&(inner as usize)) {
        unsafe { *inner != 0 }
    } else {
        false
    }
}

/// 创建一个取消令牌。
///
/// 行为复刻原版 DLL：在进程堆上分配 1 字节并初始化为 `0`，
/// 同时把指针登记到全局活动集合中。
///
/// # 返回值
/// 指向 1 字节缓冲区的指针；分配失败时返回 `null` 并写入错误信息。
///
/// # 内存所有权
/// 返回的指针必须由 [`bt_release_cancellation_token`] 释放。
#[no_mangle]
pub unsafe extern "C" fn bt_new_cancellation_token() -> *mut u8 {
    let p = unsafe { heap_alloc(1) };
    if !p.is_null() {
        unsafe { *p = 0 };
        register_token(p);
    } else {
        set_last_error_str("insufficient memory");
    }
    p
}

/// 取消一个取消令牌。
///
/// 行为复刻原版反汇编 `mov rax, [rcx]; mov byte ptr [rax], 1`：
/// 把 `*token` 指向的字节置为 `1`。
///
/// # 参数
/// - `token`：指向“令牌指针”的指针（`u8**`）。传入 `null` 或 `*token` 为 `null`
///   时静默返回；指针不在活动集合中时也不会写入。
///
/// # 内存所有权
/// 本函数不释放任何内存，仅置标志位。
#[no_mangle]
pub unsafe extern "C" fn bt_cancel_cancellation_token(token: *mut *mut u8) {
    if token.is_null() {
        return;
    }
    let lock = get_active_tokens().lock().unwrap();
    let p = unsafe { *token };
    if p.is_null() {
        return;
    }
    if lock.contains(&(p as usize)) {
        unsafe { *p = 1 };
    }
}

/// 释放由 [`bt_new_cancellation_token`] 创建的取消令牌。
///
/// 签名与原版一致：接受 `u8**`，从活动集合中移除后用进程堆 `HeapFree` 释放。
/// 调用后 `*token` 会被置 `null`，重复释放安全。
///
/// # 内存所有权
/// 仅可释放由 [`bt_new_cancellation_token`] 返回的指针。
#[no_mangle]
pub unsafe extern "C" fn bt_release_cancellation_token(token: *mut *mut u8) {
    let p = unregister_and_null_token(token);
    if !p.is_null() {
        unsafe { heap_free_u8(p) };
    }
}
