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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    // 以下测试覆盖 register_token / unregister_and_null_token /
    // is_token_active_and_canceled 三个纯逻辑函数。
    // 这些函数仅操作全局 HashSet 与指针读写，不依赖 winheap，
    // 因此在 Linux 沙箱上也可编译运行。
    // 每个测试使用各自栈上的字节作为“令牌内存”，并在结束时调用
    // unregister 清理，避免污染全局活动集合。

    #[test]
    fn register_token_null_is_noop() {
        // register_token 对 null 不应做任何事，也不会 panic
        register_token(ptr::null_mut());
    }

    #[test]
    fn is_canceled_null_token_ptr_returns_false() {
        // token_ptr_ptr 为 null 时直接返回 false
        assert!(!is_token_active_and_canceled(ptr::null_mut()));
    }

    #[test]
    fn is_canceled_null_inner_returns_false() {
        // *token_ptr_ptr 为 null 时返回 false
        let mut inner: *mut u8 = ptr::null_mut();
        let token_ptr = &mut inner as *mut *mut u8;
        assert!(!is_token_active_and_canceled(token_ptr));
    }

    #[test]
    fn unregister_null_token_returns_null() {
        // token 为 null 时返回 null
        assert!(unregister_and_null_token(ptr::null_mut()).is_null());
    }

    #[test]
    fn unregister_null_inner_returns_null() {
        // *token 为 null 时返回 null，且不修改 *token
        let mut inner: *mut u8 = ptr::null_mut();
        let token_ptr = &mut inner as *mut *mut u8;
        let result = unregister_and_null_token(token_ptr);
        assert!(result.is_null());
        assert!(inner.is_null(), "*token 应仍为 null");
    }

    #[test]
    fn register_then_unregister_roundtrip() {
        // 注册一个指针后应能成功取消注册并取回原指针
        let mut byte: u8 = 0;
        let ptr_byte = &mut byte as *mut u8;
        register_token(ptr_byte);
        let mut inner = ptr_byte;
        let token_ptr = &mut inner as *mut *mut u8;
        let recovered = unregister_and_null_token(token_ptr);
        assert_eq!(recovered, ptr_byte, "应取回原指针");
        assert!(inner.is_null(), "unregister 后 *token 应被置 null");
    }

    #[test]
    fn unregister_not_in_set_returns_null() {
        // 未注册的指针应返回 null，且不修改 *token
        let mut byte: u8 = 0;
        let ptr_byte = &mut byte as *mut u8;
        let mut inner = ptr_byte;
        let token_ptr = &mut inner as *mut *mut u8;
        let result = unregister_and_null_token(token_ptr);
        assert!(result.is_null(), "未注册的指针应返回 null");
        assert_eq!(inner, ptr_byte, "*token 不应被修改");
    }

    #[test]
    fn is_canceled_unregistered_pointer_returns_false() {
        // 指针不在活动集合中时返回 false，即使 *inner != 0
        let mut byte: u8 = 1; // 模拟“已取消”
        let ptr_byte = &mut byte as *mut u8;
        let mut inner = ptr_byte;
        let token_ptr = &mut inner as *mut *mut u8;
        assert!(!is_token_active_and_canceled(token_ptr), "未注册指针不应视为已取消");
    }

    #[test]
    fn is_canceled_registered_zero_byte_returns_false() {
        // 已注册但 *inner == 0 时返回 false
        let mut byte: u8 = 0;
        let ptr_byte = &mut byte as *mut u8;
        register_token(ptr_byte);
        let mut inner = ptr_byte;
        let token_ptr = &mut inner as *mut *mut u8;
        let result = is_token_active_and_canceled(token_ptr);
        // 清理：取消注册
        let _ = unregister_and_null_token(token_ptr);
        assert!(!result, "已注册但未取消应返回 false");
    }

    #[test]
    fn is_canceled_registered_nonzero_byte_returns_true() {
        // 已注册且 *inner != 0 时返回 true
        let mut byte: u8 = 0;
        let ptr_byte = &mut byte as *mut u8;
        register_token(ptr_byte);
        // 模拟取消：写 1
        unsafe { *ptr_byte = 1; }
        let mut inner = ptr_byte;
        let token_ptr = &mut inner as *mut *mut u8;
        let result = is_token_active_and_canceled(token_ptr);
        // 清理：先把字节置回 0，再取消注册
        unsafe { *ptr_byte = 0; }
        let _ = unregister_and_null_token(token_ptr);
        assert!(result, "已注册且 *inner != 0 应返回 true");
    }

    #[test]
    fn double_unregister_second_returns_null() {
        // 重复取消注册：第一次成功，第二次返回 null
        let mut byte: u8 = 0;
        let ptr_byte = &mut byte as *mut u8;
        register_token(ptr_byte);
        let mut inner = ptr_byte;
        let token_ptr = &mut inner as *mut *mut u8;
        let first = unregister_and_null_token(token_ptr);
        assert_eq!(first, ptr_byte, "第一次应成功取回");
        assert!(inner.is_null(), "*token 应被置 null");
        let second = unregister_and_null_token(token_ptr);
        assert!(second.is_null(), "第二次应返回 null");
    }

    #[test]
    fn register_same_pointer_twice_is_idempotent_in_set() {
        // HashSet 重复 insert 同一指针：集合大小不变，unregister 仍能取回
        let mut byte: u8 = 0;
        let ptr_byte = &mut byte as *mut u8;
        register_token(ptr_byte);
        register_token(ptr_byte); // 重复注册
        let mut inner = ptr_byte;
        let token_ptr = &mut inner as *mut *mut u8;
        let recovered = unregister_and_null_token(token_ptr);
        assert_eq!(recovered, ptr_byte, "重复注册后仍应能取回");
        assert!(inner.is_null());
    }
}
