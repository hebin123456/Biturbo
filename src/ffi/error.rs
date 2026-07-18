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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_returns_none_when_no_error_set() {
        // 当前线程未设置任何错误时应返回 None
        // 注意：take 会消费，先消费掉可能残留的状态
        let _ = take_last_error_bytes();
        assert!(take_last_error_bytes().is_none());
    }

    #[test]
    fn set_then_take_returns_message() {
        let _ = take_last_error_bytes();
        set_last_error_str("hello error");
        let taken = take_last_error_bytes();
        assert!(taken.is_some(), "取出的错误信息不应为空");
        assert_eq!(taken.unwrap(), b"hello error");
    }

    #[test]
    fn take_twice_second_is_none() {
        // 验证“读完即焚”语义
        let _ = take_last_error_bytes();
        set_last_error_str("once");
        let _ = take_last_error_bytes();
        assert!(take_last_error_bytes().is_none(), "第二次取出应为 None");
    }

    #[test]
    fn set_overwrites_previous() {
        let _ = take_last_error_bytes();
        set_last_error_str("first");
        set_last_error_str("second");
        let taken = take_last_error_bytes().unwrap();
        assert_eq!(taken, b"second", "后设置的应覆盖前一次");
    }

    #[test]
    fn set_empty_string_is_preserved() {
        // 空串也是合法错误信息，不应被当作 None
        let _ = take_last_error_bytes();
        set_last_error_str("");
        let taken = take_last_error_bytes();
        assert!(taken.is_some(), "空串也应被保留");
        assert!(taken.unwrap().is_empty());
    }

    #[test]
    fn set_utf8_message_preserves_bytes() {
        // 验证非 ASCII 的 UTF-8 字节被原样保留
        let _ = take_last_error_bytes();
        let msg = "解析失败：无效的 SHA-1 \u{1F6AB}";
        set_last_error_str(msg);
        let taken = take_last_error_bytes().unwrap();
        assert_eq!(taken, msg.as_bytes());
    }

    #[test]
    fn error_buffer_is_thread_local() {
        // 线程本地存储：主线程设置错误，新线程不应看到
        let _ = take_last_error_bytes();
        set_last_error_str("main-thread-only");
        let join = std::thread::spawn(|| take_last_error_bytes());
        let other = join.join().unwrap();
        // 子线程从未设置过错误，应为 None
        assert!(other.is_none(), "错误信息不应跨线程可见");
        // 主线程仍能取出
        let mine = take_last_error_bytes().unwrap();
        assert_eq!(mine, b"main-thread-only");
    }

    #[test]
    fn set_then_take_independent_threads() {
        // 两个线程各自设置/取出互不干扰
        let h1 = std::thread::spawn(|| {
            let _ = take_last_error_bytes();
            set_last_error_str("t1");
            take_last_error_bytes()
        });
        let h2 = std::thread::spawn(|| {
            let _ = take_last_error_bytes();
            set_last_error_str("t2");
            take_last_error_bytes()
        });
        assert_eq!(h1.join().unwrap().unwrap(), b"t1");
        assert_eq!(h2.join().unwrap().unwrap(), b"t2");
    }
}
