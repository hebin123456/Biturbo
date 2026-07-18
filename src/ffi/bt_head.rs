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

