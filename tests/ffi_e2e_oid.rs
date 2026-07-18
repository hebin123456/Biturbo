//! 端到端测试：`bt_oid_from_str` 的 SHA-1 字符串解析。
//!
//! 覆盖合法输入、null 指针、长度非法、非法字符等场景。

use biturbo::ffi::bt_oid::bt_oid_from_str;
use std::ffi::CString;

#[test]
fn parse_valid_sha1_succeeds() {
    // 合法的 40 字符十六进制 SHA-1
    let hex = "01234567ffffffffffffffffffffffffffffffff";
    let input = CString::new(hex).unwrap();
    let mut out = [0u8; 20];
    let rc = unsafe { bt_oid_from_str(input.as_ptr(), out.as_mut_ptr()) };
    assert_eq!(rc, 0, "合法 SHA-1 应返回 0");

    // bt_oid_from_str 按 4 字节为一组、组内反序输出
    // 原始字节: [01, 23, 45, 67, ff, ff, ff, ff, ...]
    // word 0 输出: [67, 45, 23, 01]
    // word 1..4 输出: [ff, ff, ff, ff]
    assert_eq!(out[0], 0x67);
    assert_eq!(out[1], 0x45);
    assert_eq!(out[2], 0x23);
    assert_eq!(out[3], 0x01);
    for &b in &out[4..20] {
        assert_eq!(b, 0xff, "word 1..4 应全为 0xff");
    }
}

#[test]
fn parse_all_zeros_sha1() {
    let hex = "0000000000000000000000000000000000000000";
    let input = CString::new(hex).unwrap();
    let mut out = [0xABu8; 20];
    let rc = unsafe { bt_oid_from_str(input.as_ptr(), out.as_mut_ptr()) };
    assert_eq!(rc, 0);
    assert_eq!(out, [0u8; 20], "全零 SHA-1 输出应全为零");
}

#[test]
fn parse_all_f_sha1() {
    let hex = "ffffffffffffffffffffffffffffffffffffffff";
    let input = CString::new(hex).unwrap();
    let mut out = [0u8; 20];
    let rc = unsafe { bt_oid_from_str(input.as_ptr(), out.as_mut_ptr()) };
    assert_eq!(rc, 0);
    assert_eq!(out, [0xFFu8; 20], "全 f SHA-1 输出应全为 0xff");
}

#[test]
fn parse_null_input_returns_error() {
    let mut out = [0u8; 20];
    let rc = unsafe { bt_oid_from_str(std::ptr::null(), out.as_mut_ptr()) };
    assert_eq!(rc, 1, "null 输入应返回 1");
}

#[test]
fn parse_null_output_returns_error() {
    let input = CString::new("0000000000000000000000000000000000000000").unwrap();
    let rc = unsafe { bt_oid_from_str(input.as_ptr(), std::ptr::null_mut()) };
    assert_eq!(rc, 1, "null 输出应返回 1");
}

#[test]
fn parse_wrong_length_returns_error() {
    // 39 字符——太短
    let short = CString::new("0123456789abcdef0123456789abcdef0123456").unwrap();
    let mut out = [0u8; 20];
    let rc = unsafe { bt_oid_from_str(short.as_ptr(), out.as_mut_ptr()) };
    assert_eq!(rc, 1, "39 字符应返回 1");

    // 41 字符——太长
    let long = CString::new("0123456789abcdef0123456789abcdef012345678").unwrap();
    let rc = unsafe { bt_oid_from_str(long.as_ptr(), out.as_mut_ptr()) };
    assert_eq!(rc, 1, "41 字符应返回 1");
}

#[test]
fn parse_invalid_chars_returns_error() {
    let bad = CString::new("0123456789abcdef0123456789abcdef0123456g").unwrap();
    let mut out = [0u8; 20];
    let rc = unsafe { bt_oid_from_str(bad.as_ptr(), out.as_mut_ptr()) };
    assert_eq!(rc, 1, "含非法字符 'g' 应返回 1");
}

#[test]
fn parse_uppercase_hex_accepted() {
    let hex = "0123456789ABCDEF0123456789ABCDEF01234567";
    let input = CString::new(hex).unwrap();
    let mut out = [0u8; 20];
    let rc = unsafe { bt_oid_from_str(input.as_ptr(), out.as_mut_ptr()) };
    assert_eq!(rc, 0, "大写十六进制应被接受");
    // 验证输出非全零
    assert!(out.iter().any(|&b| b != 0));
}

#[test]
fn parse_word_swap_pattern() {
    // 使用可辨识的 pattern 验证 4 字节组内反序
    // hex "112233445566778899aabbccddeeff0011223344"
    // 原始字节: [11, 22, 33, 44, 55, 66, 77, 88, 99, aa, bb, cc, dd, ee, ff, 00, 11, 22, 33, 44]
    // 输出 word 0: [44, 33, 22, 11]
    // 输出 word 1: [88, 77, 66, 55]
    // ...
    let hex = "112233445566778899aabbccddeeff0011223344";
    let input = CString::new(hex).unwrap();
    let mut out = [0u8; 20];
    let rc = unsafe { bt_oid_from_str(input.as_ptr(), out.as_mut_ptr()) };
    assert_eq!(rc, 0);
    assert_eq!(out[0..4], [0x44, 0x33, 0x22, 0x11]);
    assert_eq!(out[4..8], [0x88, 0x77, 0x66, 0x55]);
    assert_eq!(out[8..12], [0xcc, 0xbb, 0xaa, 0x99]);
    assert_eq!(out[12..16], [0x00, 0xff, 0xee, 0xdd]);
    assert_eq!(out[16..20], [0x44, 0x33, 0x22, 0x11]);
}
