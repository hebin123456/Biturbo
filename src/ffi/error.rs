//! # 线程本地错误信息存储
//!
//! 提供 FFI 边界使用的最近一次错误信息读写原语。
//! 错误信息以字节向量形式保存在线程本地存储中，
//! 各线程互不影响——这一行为是 `bt_get_last_error_message`
//! “每线程独立”约定的实现基础。

use std::cell::RefCell;

thread_local! {
    static LAST_ERROR: RefCell<Option<Vec<u8>>> = RefCell::new(None);
}

/// 记录当前线程的最近一次错误信息（覆盖式写入）。
///
/// 多次调用会覆盖前一次的内容。`bt_*` 系列函数失败时
/// 通常会调用本函数以附加上下文，便于调用方通过
/// `bt_get_last_error_message` 取回。
pub fn set_last_error_str(msg: &str) {
    LAST_ERROR.with(|e| *e.borrow_mut() = Some(msg.as_bytes().to_vec()));
}

/// 取出并清空当前线程的最近一次错误信息。
///
/// 调用后该线程的错误缓冲区会被重置为 `None`，
/// 即“读完即焚”——再次调用将返回 `None`。
pub fn take_last_error_bytes() -> Option<Vec<u8>> {
    LAST_ERROR.with(|e| e.borrow_mut().take())
}
