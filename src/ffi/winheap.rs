use core::ffi::c_void;
use std::os::raw::c_char;

#[link(name = "kernel32")]
extern "system" {
    fn GetProcessHeap() -> *mut c_void;
    fn HeapAlloc(hHeap: *mut c_void, dwFlags: u32, dwBytes: usize) -> *mut c_void;
    fn HeapFree(hHeap: *mut c_void, dwFlags: u32, lpMem: *mut c_void) -> i32;
}

pub unsafe fn heap_alloc(bytes: usize) -> *mut u8 {
    if bytes == 0 {
        return core::ptr::null_mut();
    }
    let heap = unsafe { GetProcessHeap() };
    if heap.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { HeapAlloc(heap, 0, bytes) as *mut u8 }
}

pub unsafe fn heap_free(ptr: *mut c_void) {
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

pub unsafe fn heap_free_u8(ptr: *mut u8) {
    unsafe { heap_free(ptr as *mut c_void) }
}

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

