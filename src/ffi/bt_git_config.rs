//! # Git 配置文件解析
//!
//! 提供 [`bt_get_git_config`] / [`bt_release_git_config`]：
//! 把 Git 风格 INI 配置文件解析为 section / subsection / kv 三元组列表，
//! 通过进程堆分配返回，供 C 侧按需读取。

use crate::ffi::error::set_last_error_str;
use crate::ffi::types::{BtGitConfig, BtGitConfigEntry, BtGitConfigKv};
use crate::ffi::winheap::{heap_alloc, heap_alloc_c_string};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::path::PathBuf;

/// 解析 Git 风格配置文件，返回 section 列表。
///
/// # 参数
/// - `config_path`：配置文件路径（NUL 终止 UTF-8）。
/// - `out_cfg`：输出 [`BtGitConfig`]，调用前可未初始化。
///
/// # 返回值
/// - `0`：成功（含文件为空时返回零长度结果）。
/// - `1`：参数非法、文件读取失败、配置非 UTF-8 或内存不足。
///
/// # 内存所有权
/// 输出的 `ptr` 数组、每个 entry 的 `a`/`b`/`kv_ptr` 及其中 `k`/`v` 字符串
/// 均通过进程堆分配，必须用 [`bt_release_git_config`] 一次性释放。
#[no_mangle]
pub unsafe extern "C" fn bt_get_git_config(config_path: *const c_char, out_cfg: *mut BtGitConfig) -> c_int {
    if config_path.is_null() || out_cfg.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_cfg).ptr = core::ptr::null_mut();
        (*out_cfg).len = 0;
        (*out_cfg).cap = 0;
    }

    let path_bytes = unsafe { CStr::from_ptr(config_path) }.to_bytes();
    let path_str = match std::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8 config path");
            return 1;
        }
    };

    let path = PathBuf::from(path_str);
    let content = match std::fs::read(&path) {
        Ok(c) => c,
        Err(e) => {
            set_last_error_str(&format!("open git config at '{}': {e}", path.display()));
            return 1;
        }
    };

    let entries = match parse_config_content(&content) {
        Ok(e) => e,
        Err(msg) => {
            set_last_error_str(&format!("parse config: {msg}"));
            return 1;
        }
    };

    if entries.is_empty() {
        return 0;
    }

    // Allocate memory using process heap
    let entry_bytes = entries.len() * std::mem::size_of::<BtGitConfigEntry>();
    let entry_ptr = unsafe { heap_alloc(entry_bytes) } as *mut BtGitConfigEntry;
    if entry_ptr.is_null() {
        set_last_error_str("insufficient memory");
        return 1;
    }

    for (i, entry) in entries.iter().enumerate() {
        let entry_dst = unsafe { &mut *entry_ptr.add(i) };
        entry_dst.a = unsafe { heap_alloc_c_string(&entry.section) };
        entry_dst.b = unsafe { heap_alloc_c_string(&entry.subsection) };

        if entry.kvs.is_empty() {
            entry_dst.kv_ptr = core::ptr::null_mut();
            entry_dst.kv_len = 0;
            entry_dst.kv_cap = 0;
        } else {
            let kv_bytes = entry.kvs.len() * std::mem::size_of::<BtGitConfigKv>();
            let kv_ptr = unsafe { heap_alloc(kv_bytes) } as *mut BtGitConfigKv;
            if kv_ptr.is_null() {
                // Free already allocated parts of this and previous entries
                // to avoid memory leaks
                for prev_idx in 0..=i {
                    let prev = unsafe { &mut *entry_ptr.add(prev_idx) };
                    if !prev.a.is_null() { crate::ffi::winheap::heap_free(prev.a as _); }
                    if !prev.b.is_null() { crate::ffi::winheap::heap_free(prev.b as _); }
                    if !prev.kv_ptr.is_null() {
                        for kv_idx in 0..prev.kv_len {
                            let kv = unsafe { &mut *prev.kv_ptr.add(kv_idx) };
                            if !kv.k.is_null() { crate::ffi::winheap::heap_free(kv.k as _); }
                            if !kv.v.is_null() { crate::ffi::winheap::heap_free(kv.v as _); }
                        }
                        if prev.kv_cap != 0 {
                            crate::ffi::winheap::heap_free(prev.kv_ptr as _);
                        }
                    }
                }
                crate::ffi::winheap::heap_free(entry_ptr as _);
                set_last_error_str("insufficient memory");
                return 1;
            }

            for (j, kv) in entry.kvs.iter().enumerate() {
                let kv_dst = unsafe { &mut *kv_ptr.add(j) };
                kv_dst.k = unsafe { heap_alloc_c_string(&kv.k) };
                kv_dst.v = unsafe { heap_alloc_c_string(&kv.v) };
            }

            entry_dst.kv_ptr = kv_ptr;
            entry_dst.kv_len = entry.kvs.len();
            entry_dst.kv_cap = entry.kvs.len();
        }
    }

    unsafe {
        (*out_cfg).ptr = entry_ptr;
        (*out_cfg).len = entries.len();
        (*out_cfg).cap = entries.len();
    }

    0
}

struct ParsedEntry {
    section: String,
    subsection: String,
    kvs: Vec<ParsedKv>,
}

struct ParsedKv {
    k: String,
    v: String,
}

fn parse_config_content(content: &[u8]) -> Result<Vec<ParsedEntry>, String> {
    let mut entries: Vec<ParsedEntry> = Vec::new();
    let text = std::str::from_utf8(content).map_err(|e| format!("non-utf8 config: {e}"))?;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section_content = &trimmed[1..trimmed.len() - 1].trim();
            // Handle subsection e.g. [remote "origin"]
            let mut parts = section_content.splitn(2, |c: char| c.is_whitespace());
            let section = parts.next().unwrap_or("").trim().to_string();
            let mut subsection = String::new();
            if let Some(sub) = parts.next() {
                let sub_trimmed = sub.trim();
                if sub_trimmed.starts_with('"') && sub_trimmed.ends_with('"') {
                    subsection = sub_trimmed[1..sub_trimmed.len() - 1].to_string();
                } else {
                    subsection = sub_trimmed.to_string();
                }
            }

            entries.push(ParsedEntry {
                section,
                subsection,
                kvs: Vec::new(),
            });
        } else if let Some(idx) = trimmed.find('=') {
            let k = trimmed[..idx].trim().to_string();
            let v = trimmed[idx + 1..].trim().to_string();

            if let Some(current) = entries.last_mut() {
                current.kvs.push(ParsedKv { k, v });
            }
        }
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    // parse_config_content 是纯函数：把 Git 风格 INI 配置文本解析为
    // section/subsection/kv 三元组列表。不依赖 winheap 或文件系统，
    // 可在 Linux 沙箱上直接测试。

    #[test]
    fn parse_empty_content_returns_empty() {
        let entries = parse_config_content(b"").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_only_comments_and_blanks_returns_empty() {
        let content = b"\n\
            ; this is a semicolon comment\n\
            # this is a hash comment\n\
            \t\n\
              \n";
        let entries = parse_config_content(content).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_single_section_no_kvs() {
        let entries = parse_config_content(b"[core]\n").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].section, "core");
        assert_eq!(entries[0].subsection, "");
        assert!(entries[0].kvs.is_empty());
    }

    #[test]
    fn parse_section_with_single_kv() {
        let content = b"[user]\nname = Alice\n";
        let entries = parse_config_content(content).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].section, "user");
        assert_eq!(entries[0].kvs.len(), 1);
        assert_eq!(entries[0].kvs[0].k, "name");
        assert_eq!(entries[0].kvs[0].v, "Alice");
    }

    #[test]
    fn parse_section_with_multiple_kvs() {
        let content = b"[user]\nname = Alice\nemail = alice@example.com\n";
        let entries = parse_config_content(content).unwrap();
        assert_eq!(entries[0].kvs.len(), 2);
        assert_eq!(entries[0].kvs[0].k, "name");
        assert_eq!(entries[0].kvs[0].v, "Alice");
        assert_eq!(entries[0].kvs[1].k, "email");
        assert_eq!(entries[0].kvs[1].v, "alice@example.com");
    }

    #[test]
    fn parse_quoted_subsection() {
        // [remote "origin"] -> section=remote, subsection=origin
        let entries = parse_config_content(b"[remote \"origin\"]\nurl = git@example.com:a.git\n").unwrap();
        assert_eq!(entries[0].section, "remote");
        assert_eq!(entries[0].subsection, "origin");
        assert_eq!(entries[0].kvs[0].k, "url");
        assert_eq!(entries[0].kvs[0].v, "git@example.com:a.git");
    }

    #[test]
    fn parse_unquoted_subsection() {
        // 子段未加引号时，原样保留（trim 后）
        let entries = parse_config_content(b"[branch main]\nmerge = refs/heads/main\n").unwrap();
        assert_eq!(entries[0].section, "branch");
        assert_eq!(entries[0].subsection, "main");
    }

    #[test]
    fn parse_multiple_sections_preserve_order() {
        let content = b"[core]\nrepositoryformatversion = 0\n[user]\nname = Bob\n[remote \"up\"]\nurl = up\n";
        let entries = parse_config_content(content).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].section, "core");
        assert_eq!(entries[1].section, "user");
        assert_eq!(entries[2].section, "remote");
        assert_eq!(entries[2].subsection, "up");
    }

    #[test]
    fn parse_kv_before_any_section_is_dropped() {
        // 任何 section 之前的 kv 应被静默丢弃（last_mut 为 None）
        let content = b"orphan = value\n[core]\nfilemode = true\n";
        let entries = parse_config_content(content).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kvs.len(), 1, "orphan kv 应被丢弃");
        assert_eq!(entries[0].kvs[0].k, "filemode");
    }

    #[test]
    fn parse_non_utf8_content_returns_err() {
        // 非法 UTF-8 字节应返回错误
        let bad = [0x80, 0xFF, b'\n', b'[', b'a', b']'];
        assert!(parse_config_content(&bad).is_err());
    }

    #[test]
    fn parse_crlf_line_endings() {
        // Windows CRLF 行尾应被正确处理（lines() 会剥掉 \r\n）
        let content = b"[core]\r\nfilemode = true\r\n";
        let entries = parse_config_content(content).unwrap();
        assert_eq!(entries[0].section, "core");
        assert_eq!(entries[0].kvs[0].k, "filemode");
        assert_eq!(entries[0].kvs[0].v, "true");
    }

    #[test]
    fn parse_kv_with_extra_whitespace_around_equals() {
        let entries = parse_config_content(b"[core]\n   filemode   =   true   \n").unwrap();
        assert_eq!(entries[0].kvs[0].k, "filemode");
        assert_eq!(entries[0].kvs[0].v, "true");
    }

    #[test]
    fn parse_kv_value_with_spaces_preserved() {
        // 值中的内部空格应被保留（仅首尾被 trim）
        let entries = parse_config_content(b"[core]\nname = Alice Bob Carol\n").unwrap();
        assert_eq!(entries[0].kvs[0].v, "Alice Bob Carol");
    }

    #[test]
    fn parse_value_containing_equals_sign() {
        // 值中含 '=' 时，仅以首个 '=' 作为分隔
        let entries = parse_config_content(b"[core]\nurl = https://x?a=1&b=2\n").unwrap();
        assert_eq!(entries[0].kvs[0].k, "url");
        assert_eq!(entries[0].kvs[0].v, "https://x?a=1&b=2");
    }

    #[test]
    fn parse_line_without_equals_outside_section_is_ignored() {
        // 既非 section 也非 kv（无 '='）的行应被忽略
        let content = b"stray line\n[core]\nfilemode = true\n";
        let entries = parse_config_content(content).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kvs.len(), 1);
    }

    #[test]
    fn parse_section_with_trailing_whitespace_inside_brackets() {
        // section 名后带空格再闭合 ']'：trim 后应正确解析
        let entries = parse_config_content(b"[ core ]\nfilemode = true\n").unwrap();
        assert_eq!(entries[0].section, "core");
    }

    #[test]
    fn parse_empty_value() {
        // 空值（'=' 后无内容）应得到空字符串
        let entries = parse_config_content(b"[core]\nkey =\n").unwrap();
        assert_eq!(entries[0].kvs[0].k, "key");
        assert_eq!(entries[0].kvs[0].v, "");
    }

    #[test]
    fn parse_empty_key_with_equals() {
        // 极端：键为空（'=' 在行首）
        let entries = parse_config_content(b"[core]\n= value\n").unwrap();
        assert_eq!(entries[0].kvs[0].k, "");
        assert_eq!(entries[0].kvs[0].v, "value");
    }

    #[test]
    fn parse_section_name_case_preserved() {
        // Git 配置 section 名大小写不敏感，但本解析器原样保留
        let entries = parse_config_content(b"[CoRe]\n").unwrap();
        assert_eq!(entries[0].section, "CoRe");
    }
}

/// 释放 [`bt_get_git_config`] 返回的 [`BtGitConfig`]。
///
/// 会逐个 section 释放 `a`（section 名）、`b`（subsection 名）、
/// 每个 `kv_ptr` 数组中的 `k` / `v` 字符串，最后释放数组本身。
/// 释放前会把首字节置 `0` 作为“毒化”标志，与原版 DLL 行为一致。
/// 传入 `null` 安全。
///
/// # 内存所有权
/// 仅可释放由 [`bt_get_git_config`] 填充的配置。
#[no_mangle]
pub unsafe extern "C" fn bt_release_git_config(cfg: *mut BtGitConfig) {
    if cfg.is_null() {
        return;
    }
    let ptr = std::ptr::replace(&mut (*cfg).ptr, core::ptr::null_mut());
    let len = (*cfg).len;
    let cap = (*cfg).cap;
    (*cfg).len = 0;
    (*cfg).cap = 0;

    if !ptr.is_null() {
        for i in 0..len {
            let entry = &mut *ptr.add(i);
            let entry_a = std::ptr::replace(&mut entry.a, core::ptr::null_mut());
            if !entry_a.is_null() {
                *entry_a = 0; // poison
                crate::ffi::winheap::heap_free(entry_a as *mut c_void);
            }
            let entry_b = std::ptr::replace(&mut entry.b, core::ptr::null_mut());
            if !entry_b.is_null() {
                *entry_b = 0; // poison
                crate::ffi::winheap::heap_free(entry_b as *mut c_void);
            }
            let kv_ptr = std::ptr::replace(&mut entry.kv_ptr, core::ptr::null_mut());
            let kv_len = entry.kv_len;
            let kv_cap = entry.kv_cap;
            entry.kv_len = 0;
            entry.kv_cap = 0;
            if !kv_ptr.is_null() {
                for j in 0..kv_len {
                    let kv = &mut *kv_ptr.add(j);
                    let kv_k = std::ptr::replace(&mut kv.k, core::ptr::null_mut());
                    if !kv_k.is_null() {
                        *kv_k = 0; // poison
                        crate::ffi::winheap::heap_free(kv_k as *mut c_void);
                    }
                    let kv_v = std::ptr::replace(&mut kv.v, core::ptr::null_mut());
                    if !kv_v.is_null() {
                        *kv_v = 0; // poison
                        crate::ffi::winheap::heap_free(kv_v as *mut c_void);
                    }
                }
                if kv_cap != 0 {
                    crate::ffi::winheap::heap_free(kv_ptr as *mut c_void);
                }
            }
        }
        if cap != 0 {
            crate::ffi::winheap::heap_free(ptr as *mut c_void);
        }
    }
}
