//! # SHA-1 OID 字符串解析
//!
//! 提供 [`bt_oid_from_str`]：把 40 字符的十六进制 SHA-1 字符串
//! 解析为 20 字节原始 OID，并按原版 `biturbo.dll` 的字节序约定写出。

use crate::ffi::error::set_last_error_str;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

/// 将 40 字符的十六进制 SHA-1 字符串解析为 20 字节 OID。
///
/// 输出布局与原版 DLL 保持一致：每 4 字节为一组，组内字节顺序反转
/// （即以小端字节序存储 4 字节 chunk，与 `BtOid` 的 `s0..s4` 大端 u32
/// 表示在内存中相同）。
///
/// # 参数
/// - `sha_string`: 指向 NUL 终止的 40 字符十六进制字符串的指针。
/// - `out_oid20`: 指向至少 20 字节的输出缓冲区，用于接收解析结果。
///
/// # 返回值
/// - `0`（`BT_OK`）：解析成功。
/// - `1`（`BT_ERR`）：参数为空或字符串长度/字符非法。失败时可通过
///   `bt_get_last_error_message` 取回详细描述。
///
/// # 内存所有权
/// `out_oid20` 由调用方拥有并负责释放，本函数仅写入 20 字节。
#[no_mangle]
pub unsafe extern "C" fn bt_oid_from_str(sha_string: *const c_char, out_oid20: *mut u8) -> c_int {
    if sha_string.is_null() || out_oid20.is_null() {
        set_last_error_str("invalid hash id");
        return 1;
    }

    let bytes = unsafe { CStr::from_ptr(sha_string) }.to_bytes();
    let input = String::from_utf8_lossy(bytes);
    if bytes.len() != 40 {
        set_last_error_str(&format!("parse SHA1 in '{}': OID length must be 40", input));
        return 1;
    }

    let mut raw = [0u8; 20];
    for i in 0..20 {
        let hi = bytes[i * 2];
        let lo = bytes[i * 2 + 1];
        let nib_hi = hex_nibble(hi);
        let nib_lo = hex_nibble(lo);
        match (nib_hi, nib_lo) {
            (Some(a), Some(b)) => raw[i] = (a << 4) | b,
            _ => {
                set_last_error_str(&format!("parse SHA1 in '{}': invalid hash id", input));
                return 1;
            }
        }
    }

    unsafe {
        for word in 0..5 {
            let base = word * 4;
            *out_oid20.add(base + 0) = raw[base + 3];
            *out_oid20.add(base + 1) = raw[base + 2];
            *out_oid20.add(base + 2) = raw[base + 1];
            *out_oid20.add(base + 3) = raw[base + 0];
        }
    }

    0
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_nibble_decimal_digits() {
        assert_eq!(hex_nibble(b'0'), Some(0));
        assert_eq!(hex_nibble(b'9'), Some(9));
    }

    #[test]
    fn hex_nibble_lowercase_hex() {
        assert_eq!(hex_nibble(b'a'), Some(10));
        assert_eq!(hex_nibble(b'f'), Some(15));
    }

    #[test]
    fn hex_nibble_uppercase_hex() {
        assert_eq!(hex_nibble(b'A'), Some(10));
        assert_eq!(hex_nibble(b'F'), Some(15));
    }

    #[test]
    fn hex_nibble_invalid_chars() {
        assert_eq!(hex_nibble(b'g'), None);
        assert_eq!(hex_nibble(b'G'), None);
        assert_eq!(hex_nibble(b' '), None);
        assert_eq!(hex_nibble(b'-'), None);
        assert_eq!(hex_nibble(b':'), None); // char just after '9'
    }

    #[test]
    fn hex_nibble_boundary() {
        // 各区间右边界：'9' -> ':' 之间过渡
        assert_eq!(hex_nibble(b':'), None); // char just after '9'
        // 字母区间右边界：'f' 之后是 'g'
        assert_eq!(hex_nibble(b'g'), None);
        assert_eq!(hex_nibble(b'G'), None);
        // NUL、换行、tab 等控制字符
        assert_eq!(hex_nibble(0), None);
        assert_eq!(hex_nibble(b'\n'), None);
        assert_eq!(hex_nibble(b'\t'), None);
        // 高位字节（非 ASCII）
        assert_eq!(hex_nibble(0xFF), None);
        assert_eq!(hex_nibble(0x80), None);
    }

    #[test]
    fn hex_nibble_full_range_coverage() {
        // 遍历所有 ASCII 字符，验证只有合法 hex 字符返回 Some
        for b in 0u8..=127u8 {
            let r = hex_nibble(b);
            match b {
                b'0'..=b'9' => assert_eq!(r, Some(b - b'0'), "dec {b}"),
                b'a'..=b'f' => assert_eq!(r, Some(b - b'a' + 10), "lower {b}"),
                b'A'..=b'F' => assert_eq!(r, Some(b - b'A' + 10), "upper {b}"),
                _ => assert_eq!(r, None, "non-hex {b}"),
            }
        }
    }

    #[test]
    fn hex_nibble_value_range() {
        // 数字的值域是 0..=9，字母的值域是 10..=15
        assert_eq!(hex_nibble(b'0').unwrap(), 0);
        assert_eq!(hex_nibble(b'9').unwrap(), 9);
        assert_eq!(hex_nibble(b'a').unwrap(), 10);
        assert_eq!(hex_nibble(b'f').unwrap(), 15);
        assert_eq!(hex_nibble(b'A').unwrap(), 10);
        assert_eq!(hex_nibble(b'F').unwrap(), 15);
    }
}

