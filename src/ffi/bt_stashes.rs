//! # Git Stash 列表读取
//!
//! 提供 [`bt_get_repository_stashes`] / [`bt_release_repository_stashes`]：
//! 通过 `refs/stash` reflog 枚举仓库的 stash 列表，返回每条 stash 的 OID、
//! 首父提交、作者信息（去重后单独存放）与主题。

use crate::ffi::error::set_last_error_str;
use crate::ffi::types::BtOid;
use crate::ffi::winheap::{heap_alloc, heap_alloc_c_string, heap_free};
use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::Path;

/// 去重后的作者身份（name + email）。
///
/// # 字段
/// - `name` / `email`：NUL 终止 UTF-8 字符串（进程堆分配）。
///
/// # 内存所有权
/// 由 [`bt_release_repository_stashes`] 与所在 [`BtRepositoryStashes`] 一并释放。
#[repr(C)]
pub struct BtIdentity {
    pub name: *mut c_char,
    pub email: *mut c_char,
}

/// 单条 stash 信息。
///
/// # 字段
/// - `reflog_id`：在 `refs/stash` reflog 中的下标（0 = 最新）。
/// - `oid`：stash 提交 OID。
/// - `first_parent`：stash 提交的首父 OID（通常是基础提交）。
/// - `author_index`：在 [`BtRepositoryStashes::identities`] 中的下标。
/// - `author_time`：作者时间戳（Unix 秒）。
/// - `subject`：stash 提交主题（NUL 终止 UTF-8）。
#[repr(C)]
pub struct BtStash {
    pub reflog_id: i32,
    pub oid: BtOid,
    pub first_parent: BtOid,
    pub author_index: i64,
    pub author_time: i64,
    pub subject: *mut c_char,
}

/// stash 列表批量结果。
///
/// # 字段
/// - `stashes` / `stashes_len` / `stashes_cap`：[`BtStash`] 数组。
/// - `identities` / `identities_len` / `identities_cap`：去重后的 [`BtIdentity`] 数组，
///   被 `stashes` 中的 `author_index` 引用。
///
/// # 内存所有权
/// `stashes`、`identities` 数组及其中所有字符串均通过进程堆分配，
/// 必须用 [`bt_release_repository_stashes`] 一次性释放。
#[repr(C)]
pub struct BtRepositoryStashes {
    pub stashes: *mut BtStash,
    pub stashes_len: i64,
    pub stashes_cap: i64,
    pub identities: *mut BtIdentity,
    pub identities_len: i64,
    pub identities_cap: i64,
}

/// 枚举仓库的 stash 列表。
///
/// 若仓库没有 `refs/stash` reflog，则视为“无 stash”直接返回 `0`
/// 而不是错误。空结果时不会进行任何内存分配。
///
/// # 参数
/// - `_working_dir_path`：保留参数，当前未使用。
/// - `git_dir_path`：仓库 `.git` 目录（NUL 终止 UTF-8）。
/// - `out_result`：输出 [`BtRepositoryStashes`]，调用前可未初始化。
///
/// # 返回值
/// - `0`：成功（含无 stash 情况）。
/// - `1`：参数非法或仓库/内存错误。
///
/// # 内存所有权
/// 输出的 `stashes` 与 `identities` 数组及其中字符串均通过进程堆分配，
/// 必须用 [`bt_release_repository_stashes`] 释放。
#[no_mangle]
pub unsafe extern "C" fn bt_get_repository_stashes(
    _working_dir_path: *const c_char,
    git_dir_path: *const c_char,
    out_result: *mut BtRepositoryStashes,
) -> c_int {
    if git_dir_path.is_null() || out_result.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_result).stashes = core::ptr::null_mut();
        (*out_result).stashes_len = 0;
        (*out_result).stashes_cap = 0;
        (*out_result).identities = core::ptr::null_mut();
        (*out_result).identities_len = 0;
        (*out_result).identities_cap = 0;
    }

    let git_dir_bytes = unsafe { CStr::from_ptr(git_dir_path) }.to_bytes();
    let git_dir_str = match std::str::from_utf8(git_dir_bytes) {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8 git_dir_path");
            return 1;
        }
    };
    let git_dir = Path::new(git_dir_str);

    let repo = match git2::Repository::open(git_dir) {
        Ok(r) => r,
        Err(e) => {
            set_last_error_str(&format!("failed to open repository: {e}"));
            return 1;
        }
    };

    let reflog = match repo.reflog("refs/stash") {
        Ok(rl) => rl,
        Err(_) => {
            // No stashes is not an error
            return 0;
        }
    };

    let mut stashes_list = Vec::new();
    let mut identity_map = HashMap::new();
    let mut identity_list = Vec::new();

    for (index, entry) in reflog.iter().enumerate() {
        let commit_id = entry.id_new();
        let commit = match repo.find_commit(commit_id) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let bytes = commit_id.as_bytes();
        let oid = BtOid::from_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
            bytes[16], bytes[17], bytes[18], bytes[19]
        ]);

        let first_parent_oid = if commit.parent_count() > 0 {
            if let Ok(pid) = commit.parent_id(0) {
                let p_bytes = pid.as_bytes();
                BtOid::from_bytes([
                    p_bytes[0], p_bytes[1], p_bytes[2], p_bytes[3], p_bytes[4], p_bytes[5], p_bytes[6], p_bytes[7],
                    p_bytes[8], p_bytes[9], p_bytes[10], p_bytes[11], p_bytes[12], p_bytes[13], p_bytes[14], p_bytes[15],
                    p_bytes[16], p_bytes[17], p_bytes[18], p_bytes[19]
                ])
            } else {
                BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 }
            }
        } else {
            BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 }
        };

        let author = commit.author();
        let author_name = author.name().unwrap_or("");
        let author_email = author.email().unwrap_or("");
        let author_time = author.when().seconds();

        let id_key = (author_name.to_string(), author_email.to_string());
        let author_index = *identity_map.entry(id_key.clone()).or_insert_with(|| {
            let idx = identity_list.len() as i64;
            identity_list.push(id_key);
            idx
        });

        let subject = commit.summary().ok().flatten().unwrap_or("");
        let subject_ptr = unsafe { heap_alloc_c_string(subject) };

        stashes_list.push(BtStash {
            reflog_id: index as i32,
            oid,
            first_parent: first_parent_oid,
            author_index,
            author_time,
            subject: subject_ptr,
        });
    }

    if stashes_list.is_empty() {
        return 0;
    }

    // Allocate stashes on heap
    let stashes_alloc_bytes = stashes_list.len() * std::mem::size_of::<BtStash>();
    let stashes_ptr = unsafe { heap_alloc(stashes_alloc_bytes) } as *mut BtStash;
    if stashes_ptr.is_null() {
        for s in stashes_list {
            unsafe { heap_free(s.subject as _) };
        }
        set_last_error_str("insufficient memory");
        return 1;
    }

    // Allocate identities on heap
    let mut identities_list = Vec::new();
    for (name, email) in identity_list {
        let name_ptr = unsafe { heap_alloc_c_string(&name) };
        let email_ptr = unsafe { heap_alloc_c_string(&email) };
        identities_list.push(BtIdentity {
            name: name_ptr,
            email: email_ptr,
        });
    }

    let identities_alloc_bytes = identities_list.len() * std::mem::size_of::<BtIdentity>();
    let identities_ptr = unsafe { heap_alloc(identities_alloc_bytes) } as *mut BtIdentity;
    if identities_ptr.is_null() {
        for s in stashes_list {
            unsafe { heap_free(s.subject as _) };
        }
        unsafe { heap_free(stashes_ptr as _) };
        for id in identities_list {
            unsafe {
                heap_free(id.name as _);
                heap_free(id.email as _);
            }
        }
        set_last_error_str("insufficient memory");
        return 1;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(stashes_list.as_ptr(), stashes_ptr, stashes_list.len());
        core::ptr::copy_nonoverlapping(identities_list.as_ptr(), identities_ptr, identities_list.len());

        (*out_result).stashes = stashes_ptr;
        (*out_result).stashes_len = stashes_list.len() as i64;
        (*out_result).stashes_cap = stashes_list.len() as i64;
        (*out_result).identities = identities_ptr;
        (*out_result).identities_len = identities_list.len() as i64;
        (*out_result).identities_cap = identities_list.len() as i64;
    }

    0
}

/// 释放 [`bt_get_repository_stashes`] 返回的 [`BtRepositoryStashes`]。
///
/// 会先释放每条 stash 的 `subject`、每个 identity 的 `name`/`email`，
/// 再释放 `stashes` 与 `identities` 数组本身。传入 `null` 安全。
///
/// # 内存所有权
/// 仅可释放由 [`bt_get_repository_stashes`] 填充的结构。
#[no_mangle]
pub unsafe extern "C" fn bt_release_repository_stashes(p: *mut BtRepositoryStashes) {
    if p.is_null() {
        return;
    }
    let stashes_ptr = (*p).stashes;
    let stashes_len = (*p).stashes_len;
    let stashes_cap = (*p).stashes_cap;

    let identities_ptr = (*p).identities;
    let identities_len = (*p).identities_len;
    let identities_cap = (*p).identities_cap;

    if !stashes_ptr.is_null() {
        for i in 0..stashes_len {
            let s = &mut *stashes_ptr.add(i as usize);
            let s_subject = s.subject;
            if !s_subject.is_null() {
                heap_free(s_subject as _);
            }
        }
        if stashes_cap != 0 {
            heap_free(stashes_ptr as _);
        }
    }

    if !identities_ptr.is_null() {
        for i in 0..identities_len {
            let id = &mut *identities_ptr.add(i as usize);
            let id_name = id.name;
            if !id_name.is_null() {
                heap_free(id_name as _);
            }
            let id_email = id.email;
            if !id_email.is_null() {
                heap_free(id_email as _);
            }
        }
        if identities_cap != 0 {
            heap_free(identities_ptr as _);
        }
    }
}

/// 把 40 字符十六进制 SHA-1 字符串解析为 [`BtOid`]。
///
/// 输入长度必须为 40，仅接受 `[0-9a-fA-F]` 字符；非法输入返回 `None`。
/// 输出字节序由 `BtOid::from_bytes` 决定（按 4 字节大端 u32 解释）。
#[allow(dead_code)]
pub fn parse_sha_to_btoid(sha: &str) -> Option<BtOid> {
    let b = sha.as_bytes();
    if b.len() != 40 { return None; }
    let mut raw = [0u8; 20];
    for i in 0..20 {
        let hi = hex_nibble(b[i * 2])?;
        let lo = hex_nibble(b[i * 2 + 1])?;
        raw[i] = (hi << 4) | lo;
    }
    Some(BtOid::from_bytes(raw))
}

#[allow(dead_code)]
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

    // parse_sha_to_btoid 是纯函数：把 40 字符十六进制 SHA-1 解析为 BtOid。
    // 输出经 BtOid::from_bytes 解释（按 4 字节大端 u32）。
    // 不依赖 winheap 或 git2，可在 Linux 沙箱上直接测试。

    #[test]
    fn parse_sha_all_zeros() {
        let oid = parse_sha_to_btoid("0000000000000000000000000000000000000000").unwrap();
        assert_eq!((oid.s0, oid.s1, oid.s2, oid.s3, oid.s4), (0, 0, 0, 0, 0));
    }

    #[test]
    fn parse_sha_all_f_lowercase() {
        let oid = parse_sha_to_btoid("ffffffffffffffffffffffffffffffffffffffff").unwrap();
        assert_eq!(oid.s0, 0xFFFFFFFF);
        assert_eq!(oid.s1, 0xFFFFFFFF);
        assert_eq!(oid.s2, 0xFFFFFFFF);
        assert_eq!(oid.s3, 0xFFFFFFFF);
        assert_eq!(oid.s4, 0xFFFFFFFF);
    }

    #[test]
    fn parse_sha_all_f_uppercase() {
        let oid = parse_sha_to_btoid("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF").unwrap();
        assert_eq!(oid.s0, 0xFFFFFFFF);
        assert_eq!(oid.s4, 0xFFFFFFFF);
    }

    #[test]
    fn parse_sha_mixed_case() {
        // 大小写混用应被接受，且结果与全小写一致
        let lower = parse_sha_to_btoid("abcdef0123456789abcdef0123456789abcdef01").unwrap();
        let mixed = parse_sha_to_btoid("AbCdEf0123456789abcdef0123456789abcdef01").unwrap();
        assert_eq!(lower.s0, mixed.s0);
        assert_eq!(lower, mixed);
    }

    #[test]
    fn parse_sha_known_value_each_word() {
        // 每个 word 取易识别的值，验证大端 u32 解释
        let oid = parse_sha_to_btoid("0102030405060708090a0b0c0d0e0f1011121314").unwrap();
        assert_eq!(oid.s0, 0x01020304);
        assert_eq!(oid.s1, 0x05060708);
        assert_eq!(oid.s2, 0x090a0b0c);
        assert_eq!(oid.s3, 0x0d0e0f10);
        assert_eq!(oid.s4, 0x11121314);
    }

    #[test]
    fn parse_sha_leading_zeros_preserved() {
        // 前导零不应丢失
        let oid = parse_sha_to_btoid("0000000100000002000000030000000400000005").unwrap();
        assert_eq!(oid.s0, 0x00000001);
        assert_eq!(oid.s1, 0x00000002);
        assert_eq!(oid.s2, 0x00000003);
        assert_eq!(oid.s3, 0x00000004);
        assert_eq!(oid.s4, 0x00000005);
    }

    #[test]
    fn parse_sha_too_short_returns_none() {
        assert!(parse_sha_to_btoid("000000000000000000000000000000000000000").is_none());
    }

    #[test]
    fn parse_sha_too_long_returns_none() {
        // 注意：parse_sha_to_btoid 用 len != 40 严格判等，多一个字符也返回 None
        assert!(parse_sha_to_btoid("00000000000000000000000000000000000000000").is_none());
    }

    #[test]
    fn parse_sha_empty_returns_none() {
        assert!(parse_sha_to_btoid("").is_none());
    }

    #[test]
    fn parse_sha_invalid_char_returns_none() {
        // 含 'g' 等非十六进制字符
        assert!(parse_sha_to_btoid("gggggggggggggggggggggggggggggggggggggggg").is_none());
        // 仅末字符非法
        assert!(parse_sha_to_btoid("000000000000000000000000000000000000000z").is_none());
        // 含空格
        assert!(parse_sha_to_btoid("0000000000 000000000000000000000000000000").is_none());
    }

    #[test]
    fn parse_sha_roundtrip_with_to_bytes() {
        // 解析后再 to_bytes 应还原为原始字节
        let hex = "deadbeefcafebabe1234567890abcdefdeadbeef";
        let oid = parse_sha_to_btoid(hex).unwrap();
        let bytes = oid.to_bytes();
        let reconstructed: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(reconstructed, hex);
    }

    #[test]
    fn parse_sha_word_boundary_independence() {
        // 修改第 0 个 word 不应影响其他 word
        let base = parse_sha_to_btoid("0102030405060708090a0b0c0d0e0f1011121314").unwrap();
        let modified = parse_sha_to_btoid("ffffffff05060708090a0b0c0d0e0f1011121314").unwrap();
        assert_ne!(base.s0, modified.s0);
        assert_eq!(base.s1, modified.s1);
        assert_eq!(base.s2, modified.s2);
        assert_eq!(base.s3, modified.s3);
        assert_eq!(base.s4, modified.s4);
    }

    #[test]
    fn parse_sha_exactly_40_chars_boundary() {
        // 恰好 40 字符应成功（16+16+8 = 40）
        let s = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(s.len(), 40);
        assert!(parse_sha_to_btoid(s).is_some());
    }

    #[test]
    fn parse_sha_consistent_with_btoid_from_bytes() {
        // parse_sha_to_btoid 结果应与手动 from_bytes 一致
        let hex = "1234567890abcdef1234567890abcdef12345678";
        let via_parse = parse_sha_to_btoid(hex).unwrap();
        let mut raw = [0u8; 20];
        for i in 0..20 {
            let hi = hex_nibble(hex.as_bytes()[i * 2]).unwrap();
            let lo = hex_nibble(hex.as_bytes()[i * 2 + 1]).unwrap();
            raw[i] = (hi << 4) | lo;
        }
        let via_from_bytes = BtOid::from_bytes(raw);
        assert_eq!(via_parse, via_from_bytes);
    }

    #[test]
    fn hex_nibble_helper_boundaries() {
        // 验证内部 hex_nibble 的边界行为
        assert_eq!(hex_nibble(b'0'), Some(0));
        assert_eq!(hex_nibble(b'9'), Some(9));
        assert_eq!(hex_nibble(b'a'), Some(10));
        assert_eq!(hex_nibble(b'f'), Some(15));
        assert_eq!(hex_nibble(b'A'), Some(10));
        assert_eq!(hex_nibble(b'F'), Some(15));
        assert_eq!(hex_nibble(b'g'), None);
        assert_eq!(hex_nibble(b'G'), None);
        assert_eq!(hex_nibble(b':'), None);
        assert_eq!(hex_nibble(b' '), None);
    }
}
