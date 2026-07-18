//! 端到端测试：`bt_get_last_error_message` 的线程本地错误信息读取。
//!
//! 通过触发 FFI 失败设置错误信息，然后验证 `bt_get_last_error_message`
//! 的返回值约定：0=无错误、正值=消息长度、负值=缓冲区不足。

use biturbo::ffi::bt_error::bt_get_last_error_message;
use biturbo::ffi::bt_get_tree::bt_get_tree;

/// 调用一个必定失败的 FFI 函数来设置线程本地错误信息。
fn trigger_error() {
    let _ = unsafe { bt_get_tree(std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) };
}

#[test]
fn no_error_returns_zero() {
    // 先消费可能残留的错误
    let mut buf = [0u8; 256];
    let _ = unsafe { bt_get_last_error_message(buf.as_mut_ptr(), buf.len()) };

    // 此刻没有新错误，应返回 0
    let rc = unsafe { bt_get_last_error_message(buf.as_mut_ptr(), buf.len()) };
    assert_eq!(rc, 0, "无错误时应返回 0");
}

#[test]
fn error_message_retrieved_with_sufficient_buffer() {
    trigger_error();
    let mut buf = [0u8; 256];
    let rc = unsafe { bt_get_last_error_message(buf.as_mut_ptr(), buf.len()) };
    assert!(rc > 0, "有错误时应返回正长度，实际={rc}");

    // 验证缓冲区中确实有消息文本
    let msg = std::str::from_utf8(&buf[..rc as usize]).expect("错误消息应为 UTF-8");
    assert!(!msg.is_empty(), "错误消息不应为空");
}

#[test]
fn error_message_consumed_after_read() {
    trigger_error();
    let mut buf = [0u8; 256];
    let rc1 = unsafe { bt_get_last_error_message(buf.as_mut_ptr(), buf.len()) };
    assert!(rc1 > 0, "首次读取应返回正长度");

    // 再次读取应返回 0（错误已被消费）
    let rc2 = unsafe { bt_get_last_error_message(buf.as_mut_ptr(), buf.len()) };
    assert_eq!(rc2, 0, "错误被消费后应返回 0");
}

#[test]
fn null_buffer_returns_negative_required_length() {
    trigger_error();
    // 传入 null 缓冲区应返回负值 -(len+2)
    let rc = unsafe { bt_get_last_error_message(std::ptr::null_mut(), 0) };
    assert!(rc < 0, "null 缓冲区应返回负值，实际={rc}");
    // |rc| - 2 应等于消息长度
    let required = (-rc) as usize - 2;
    assert!(required > 0, "所需长度应为正");
}

#[test]
fn too_small_buffer_returns_negative() {
    trigger_error();
    let mut tiny = [0u8; 1];
    let rc = unsafe { bt_get_last_error_message(tiny.as_mut_ptr(), tiny.len()) };
    assert!(rc < 0, "缓冲区过小应返回负值，实际={rc}");
}

#[test]
fn null_buffer_with_zero_len_after_no_error() {
    // 先清空可能残留的错误
    let mut buf = [0u8; 256];
    let _ = unsafe { bt_get_last_error_message(buf.as_mut_ptr(), buf.len()) };
    // 无错误时 null 缓冲区也应返回 0
    let rc = unsafe { bt_get_last_error_message(std::ptr::null_mut(), 0) };
    assert_eq!(rc, 0, "无错误时 null 缓冲区应返回 0");
}

#[test]
fn error_message_contains_expected_text() {
    // 用 bt_get_tree 的 null 参数触发 "invalid input" 错误
    trigger_error();
    let mut buf = [0u8; 256];
    let rc = unsafe { bt_get_last_error_message(buf.as_mut_ptr(), buf.len()) };
    assert!(rc > 0);
    let msg = std::str::from_utf8(&buf[..rc as usize]).expect("UTF-8");
    assert!(
        msg.contains("invalid") || msg.contains("null") || msg.contains("input"),
        "错误消息应包含描述性文本，实际='{msg}'"
    );
}

#[test]
fn error_message_with_exact_sized_buffer() {
    // 先探测所需长度
    trigger_error();
    let probe_rc = unsafe { bt_get_last_error_message(std::ptr::null_mut(), 0) };
    assert!(probe_rc < 0);
    let msg_len = (-probe_rc) as usize - 2;

    // 再次触发相同的错误
    trigger_error();
    // 缓冲区恰好等于 msg_len（不含终止符）应返回负值
    let mut exact = vec![0u8; msg_len];
    let rc = unsafe { bt_get_last_error_message(exact.as_mut_ptr(), exact.len()) };
    assert!(rc < 0, "缓冲区长度 == msg_len（无法放终止符）应返回负值");

    // 缓冲区 == msg_len + 1（恰好放下消息 + 终止符）应成功
    trigger_error();
    let mut exact_ok = vec![0u8; msg_len + 1];
    let rc = unsafe { bt_get_last_error_message(exact_ok.as_mut_ptr(), exact_ok.len()) };
    assert_eq!(rc, msg_len as isize, "缓冲区 == msg_len+1 应成功并返回长度");
}

#[test]
fn error_message_repeated_trigger_and_read() {
    // 多次触发-读取循环，验证线程本地存储不会累积或损坏
    for i in 0..5 {
        trigger_error();
        let mut buf = [0u8; 256];
        let rc = unsafe { bt_get_last_error_message(buf.as_mut_ptr(), buf.len()) };
        assert!(rc > 0, "第 {i} 次读取应返回正长度");
    }
}
