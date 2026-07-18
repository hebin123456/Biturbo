//! # 最近一次错误信息读取
//!
//! 暴露 [`bt_get_last_error_message`] 给 C 侧调用方，
//! 用于在 `bt_*` 函数返回失败后取回线程本地的错误描述。
//! 错误信息由 [`crate::ffi::error`] 模块维护，每线程独立。

use crate::ffi::error::take_last_error_bytes;

/// 取出并复制当前线程的最近一次错误信息到调用方提供的缓冲区。
///
/// 行为与原版 `biturbo.dll` 保持一致：
/// - 当线程没有错误信息时返回 `0`；
/// - 成功写入时返回消息字节长度（正值）；
/// - 当缓冲区过小（不足以容纳消息 + 终止符）时返回 `-(len + 2)`，
///   调用方需以至少 `len + 1` 字节的缓冲区重试。
///
/// # 参数
/// - `out`: 调用方提供的字节缓冲区指针；为 `null` 时仅探测所需长度。
/// - `out_len`: `out` 缓冲区容量（字节）。
///
/// # 内存所有权
/// `out` 由调用方拥有并负责释放；本函数仅做复制并以 `\0` 终止。
/// 错误信息本身在调用后会被消费清空。
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

