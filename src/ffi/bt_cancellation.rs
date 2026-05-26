use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::{heap_alloc, heap_free_u8};
use std::sync::OnceLock;
use std::sync::Mutex;
use std::collections::HashSet;

static ACTIVE_TOKENS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

fn get_active_tokens() -> &'static Mutex<HashSet<usize>> {
    ACTIVE_TOKENS.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn register_token(ptr: *mut u8) {
    if !ptr.is_null() {
        let mut lock = get_active_tokens().lock().unwrap();
        lock.insert(ptr as usize);
    }
}

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

/// Create a new cancellation token.
///
/// Original behavior: allocate 1 byte and set it to 0.
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

/// Cancel a cancellation token.
///
/// Original disassembly: `mov rax, [rcx]; mov byte ptr [rax], 1`.
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

/// Release a cancellation token created by `bt_new_cancellation_token`.
///
/// Signature matches the original: takes `u8**` and frees `*token` with HeapFree.
#[no_mangle]
pub unsafe extern "C" fn bt_release_cancellation_token(token: *mut *mut u8) {
    let p = unregister_and_null_token(token);
    if !p.is_null() {
        unsafe { heap_free_u8(p) };
    }
}
