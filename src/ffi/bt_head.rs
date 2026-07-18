//! # HEAD 引用读取
//!
//! 提供 [`bt_get_head`] / [`bt_release_head`]：直接读取 `.git/HEAD` 文件，
//! 返回 HEAD 的 OID（detached 状态）或符号引用名称（如 `refs/heads/main`）。

use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::heap_free_u8;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};

/// HEAD 解析结果。
///
/// # 字段
/// - `oid20`：20 字节 OID；当 HEAD 是符号引用（`ref: …`）时为全零。
///   字节序与原版 DLL 一致：每 4 字节组内反序（小端 chunk 表示大端 u32）。
/// - `_pad`：4 字节对齐填充，使 `ref_name` 落在偏移 0x18（与原版 ABI 一致）。
/// - `ref_name`：符号引用目标（NUL 终止 UTF-8）；detached 时为 `null`。
///
/// # 内存所有权
/// `ref_name` 通过进程堆分配，必须用 [`bt_release_head`] 释放。
#[repr(C)]
pub struct BtHead {
    pub oid20: [u8; 20],
    _pad: [u8; 4], // keep `ref_name` at offset 0x18
    pub ref_name: *mut c_char,
}

/// 读取仓库 HEAD。
///
/// 行为：
/// - HEAD 形如 `ref: refs/heads/main` → `oid20` 全零、`ref_name` = `"refs/heads/main"`；
/// - HEAD 形如 40 字符 SHA-1 → `oid20` 填充、`ref_name` = `null`；
/// - HEAD 为空或读取失败 → 返回错误。
///
/// # 参数
/// - `git_dir_path`：仓库 `.git` 目录（NUL 终止 UTF-8）。
/// - `out_head`：输出 [`BtHead`]，调用前可未初始化。
///
/// # 返回值
/// - `0`：成功。
/// - `1`：参数非法、HEAD 不存在/为空、UTF-8/OID 解析失败或内存不足。
///
/// # 内存所有权
/// 输出的 `ref_name` 通过进程堆分配，必须用 [`bt_release_head`] 释放。
#[no_mangle]
pub unsafe extern "C" fn bt_get_head(git_dir_path: *const c_char, out_head: *mut BtHead) -> c_int {
    if git_dir_path.is_null() || out_head.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_head).oid20 = [0u8; 20];
        (*out_head).ref_name = core::ptr::null_mut();
    }

    let git_dir_bytes = unsafe { CStr::from_ptr(git_dir_path) }.to_bytes();
    let git_dir_str = match std::str::from_utf8(git_dir_bytes) {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8 git_dir_path");
            return 1;
        }
    };
    let git_dir = PathBuf::from(git_dir_str);

    match get_head_impl(&git_dir) {
        Ok((oid20, ref_name_opt)) => {
            unsafe { (*out_head).oid20 = oid20 };
            if let Some(ref_name) = ref_name_opt {
                let p = unsafe { crate::ffi::winheap::heap_alloc_c_string(&ref_name) };
                if p.is_null() {
                    set_last_error_str("insufficient memory");
                    return 1;
                }
                unsafe { (*out_head).ref_name = p };
            }
            0
        }
        Err(msg) => {
            set_last_error_str(&format!("read head in '{}': {msg}", git_dir.display()));
            1
        }
    }
}

/// 释放 [`bt_get_head`] 返回的 [`BtHead`] 中的 `ref_name` 字符串。
///
/// 仅释放 `ref_name`，不释放 `BtHead` 结构体本身（结构体通常由调用方在栈上持有）。
/// 释放前会把首字节置 `0` 作为“毒化”标志，与原版 DLL 行为一致。
/// 传入 `null` 安全。
///
/// # 内存所有权
/// 仅可释放由 [`bt_get_head`] 填充的 `ref_name`。
#[no_mangle]
pub unsafe extern "C" fn bt_release_head(head: *mut BtHead) {
    if head.is_null() {
        return;
    }
    let p = std::ptr::replace(&mut (*head).ref_name, core::ptr::null_mut()) as *mut u8;
    if p.is_null() {
        return;
    }
    unsafe { *p = 0 };
    unsafe { heap_free_u8(p) };
}

fn get_head_impl(git_dir: &Path) -> Result<([u8; 20], Option<String>), String> {
    let head_path = git_dir.join("HEAD");
    let head_bytes = std::fs::read(&head_path).map_err(|e| format!("open HEAD: {e}"))?;
    let head_trimmed = trim_ascii_whitespace(&head_bytes);
    if head_trimmed.is_empty() {
        return Err("empty HEAD".to_string());
    }

    if let Some(rest) = head_trimmed.strip_prefix(b"ref: ") {
        let ref_bytes = trim_ascii_whitespace(rest);
        let ref_name = std::str::from_utf8(ref_bytes)
            .map_err(|_| "non-utf8 ref name".to_string())?
            .to_string();
        Ok(([0u8; 20], Some(ref_name)))
    } else {
        let oid_hex = std::str::from_utf8(head_trimmed)
            .map_err(|_| "non-utf8 detached oid".to_string())?;
        let oid20 = parse_oid_swapped(oid_hex)?;
        Ok((oid20, None))
    }
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let mut s = 0;
    let mut e = bytes.len();
    while s < e && bytes[s].is_ascii_whitespace() {
        s += 1;
    }
    while e > s && bytes[e - 1].is_ascii_whitespace() {
        e -= 1;
    }
    &bytes[s..e]
}

fn parse_oid_swapped(hex40: &str) -> Result<[u8; 20], String> {
    let b = hex40.as_bytes();
    if b.len() != 40 {
        return Err("OID length must be 40".to_string());
    }
    let mut raw = [0u8; 20];
    for i in 0..20 {
        let hi = hex_nibble(b[i * 2]).ok_or_else(|| "invalid hash id".to_string())?;
        let lo = hex_nibble(b[i * 2 + 1]).ok_or_else(|| "invalid hash id".to_string())?;
        raw[i] = (hi << 4) | lo;
    }
    let mut out = [0u8; 20];
    for word in 0..5 {
        let base = word * 4;
        out[base + 0] = raw[base + 3];
        out[base + 1] = raw[base + 2];
        out[base + 2] = raw[base + 1];
        out[base + 3] = raw[base + 0];
    }
    Ok(out)
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

    // 本模块的纯函数测试：hex_nibble / trim_ascii_whitespace / parse_oid_swapped。
    // 这些函数不依赖 winheap 或 git2，可在 Linux 沙箱上直接测试。

    // ---------- hex_nibble ----------

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
        // 边界外字符
        assert_eq!(hex_nibble(b'g'), None);
        assert_eq!(hex_nibble(b'G'), None);
        assert_eq!(hex_nibble(b':'), None); // '9' 之后
        assert_eq!(hex_nibble(b'`'), None); // 'a' 之前
        assert_eq!(hex_nibble(b'@'), None); // 'A' 之前
        // 控制字符与高字节
        assert_eq!(hex_nibble(0), None);
        assert_eq!(hex_nibble(b'\n'), None);
        assert_eq!(hex_nibble(b' '), None);
        assert_eq!(hex_nibble(0xFF), None);
    }

    #[test]
    fn hex_nibble_full_ascii_range() {
        // 遍历所有 ASCII 字符，验证只有合法 hex 字符返回 Some
        for b in 0u8..=127u8 {
            let r = hex_nibble(b);
            match b {
                b'0'..=b'9' => assert_eq!(r, Some(b - b'0')),
                b'a'..=b'f' => assert_eq!(r, Some(b - b'a' + 10)),
                b'A'..=b'F' => assert_eq!(r, Some(b - b'A' + 10)),
                _ => assert_eq!(r, None),
            }
        }
    }

    // ---------- trim_ascii_whitespace ----------

    #[test]
    fn trim_ascii_whitespace_empty() {
        assert_eq!(trim_ascii_whitespace(b""), b"");
    }

    #[test]
    fn trim_ascii_whitespace_no_whitespace() {
        assert_eq!(trim_ascii_whitespace(b"hello"), b"hello");
    }

    #[test]
    fn trim_ascii_whitespace_leading_only() {
        assert_eq!(trim_ascii_whitespace(b"   hello"), b"hello");
    }

    #[test]
    fn trim_ascii_whitespace_trailing_only() {
        assert_eq!(trim_ascii_whitespace(b"hello   "), b"hello");
    }

    #[test]
    fn trim_ascii_whitespace_both_sides() {
        assert_eq!(trim_ascii_whitespace(b"\t hello world \n"), b"hello world");
    }

    #[test]
    fn trim_ascii_whitespace_all_whitespace_becomes_empty() {
        assert_eq!(trim_ascii_whitespace(b"   \t\n\r   "), b"");
    }

    #[test]
    fn trim_ascii_whitespace_internal_preserved() {
        // 内部空白应被保留，仅裁剪首尾
        assert_eq!(trim_ascii_whitespace(b"  a  b  c  "), b"a  b  c");
    }

    #[test]
    fn trim_ascii_whitespace_all_ascii_whitespace_kinds() {
        // 空格、\t、\n、\r、\x0c (form feed)、\x0b (vertical tab) 均为 ASCII 空白
        assert_eq!(trim_ascii_whitespace(b"\x0b\x0c\t\n\r abc \r\n\t\x0c\x0b"), b"abc");
    }

    #[test]
    fn trim_ascii_whitespace_single_char() {
        assert_eq!(trim_ascii_whitespace(b"x"), b"x");
        assert_eq!(trim_ascii_whitespace(b" "), b"");
    }

    // ---------- parse_oid_swapped ----------

    #[test]
    fn parse_oid_swapped_known_value_word_swap() {
        // 输入字节按 4 字节为一组，组内字节反序输出
        // raw = [01,02,03,04 | 05,06,07,08 | 09,0a,0b,0c | 0d,0e,0f,10 | 11,12,13,14]
        let hex = "0102030405060708090a0b0c0d0e0f1011121314";
        let out = parse_oid_swapped(hex).unwrap();
        let expected = [
            0x04, 0x03, 0x02, 0x01, // word 0 反序
            0x08, 0x07, 0x06, 0x05, // word 1 反序
            0x0c, 0x0b, 0x0a, 0x09, // word 2 反序
            0x10, 0x0f, 0x0e, 0x0d, // word 3 反序
            0x14, 0x13, 0x12, 0x11, // word 4 反序
        ];
        assert_eq!(out, expected);
    }

    #[test]
    fn parse_oid_swapped_all_zeros() {
        let out = parse_oid_swapped("0000000000000000000000000000000000000000").unwrap();
        assert_eq!(out, [0u8; 20]);
    }

    #[test]
    fn parse_oid_swapped_all_ff() {
        let out = parse_oid_swapped("ffffffffffffffffffffffffffffffffffffffff").unwrap();
        assert_eq!(out, [0xFFu8; 20]);
    }

    #[test]
    fn parse_oid_swapped_palindrome_word_unchanged() {
        // 回文 word（如 ab cd cd ab）反序后不变
        let hex = "abcdcdababcdcdababcdcdababcdcdababcdcdab";
        let out = parse_oid_swapped(hex).unwrap();
        // 还原回字节应与输入一致
        let raw: Vec<u8> = (0..20).map(|i| {
            let hi = hex_nibble(hex.as_bytes()[i * 2]).unwrap();
            let lo = hex_nibble(hex.as_bytes()[i * 2 + 1]).unwrap();
            (hi << 4) | lo
        }).collect();
        assert_eq!(&out[..], &raw[..], "回文 word 反序后应不变");
    }

    #[test]
    fn parse_oid_swapped_too_short_returns_err() {
        assert!(parse_oid_swapped("0102030405060708090a0b0c0d0e0f101112131").is_err());
    }

    #[test]
    fn parse_oid_swapped_too_long_returns_err() {
        // len != 40 都返回错误
        assert!(parse_oid_swapped("0102030405060708090a0b0c0d0e0f101112131415").is_err());
    }

    #[test]
    fn parse_oid_swapped_empty_returns_err() {
        assert!(parse_oid_swapped("").is_err());
    }

    #[test]
    fn parse_oid_swapped_invalid_char_returns_err() {
        // 40 字符长度但含非法字符
        assert!(parse_oid_swapped("gggggggggggggggggggggggggggggggggggggggg").is_err());
        assert!(parse_oid_swapped("0102030405060708090a0b0c0d0e0f101112131z").is_err());
    }

    #[test]
    fn parse_oid_swapped_uppercase_accepted() {
        // 大写字母应被接受
        let out_lower = parse_oid_swapped("abcdefabcdefabcdefabcdefabcdefabcdefabcd").unwrap();
        let out_upper = parse_oid_swapped("ABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCD").unwrap();
        assert_eq!(out_lower, out_upper);
    }

    #[test]
    fn parse_oid_swapped_each_word_independent() {
        // 仅修改 word 2 不应影响其他 word 的反序结果
        let base = parse_oid_swapped("0102030405060708090a0b0c0d0e0f1011121314").unwrap();
        let modified = parse_oid_swapped("0102030405060708ffffffff0d0e0f1011121314").unwrap();
        assert_eq!(&base[0..8], &modified[0..8], "word 0/1 不应变化");
        assert_ne!(&base[8..12], &modified[8..12], "word 2 应变化");
        assert_eq!(&base[12..20], &modified[12..20], "word 3/4 不应变化");
    }
}

