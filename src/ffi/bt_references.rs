//! # 仓库引用枚举
//!
//! 提供 [`bt_get_references`] / [`bt_release_references`]：
//! 从 `.git` 目录直接读取 packed-refs、loose refs、HEAD 以及特殊引用
//! （`ORIG_HEAD`、`FETCH_HEAD`、`MERGE_HEAD` 等），并按原版 DLL 的扁平布局
//! 打包为 5 个 `BtBuf` + 1 个哈希值返回。

use crate::ffi::error::set_last_error_str;
use crate::ffi::types::{BtBuf, BtReferences, BtOid};
use crate::ffi::winheap::{heap_alloc, heap_free};
use std::collections::{hash_map::DefaultHasher, BTreeMap};
use std::ffi::CStr;
use std::hash::Hasher;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};

struct NativeRefEntry {
    name: String,
    symref: String,
    has_oid: bool,
    oid: BtOid,
    has_peeled_oid: bool,
    peeled_oid: BtOid,
}

/// 枚举仓库的所有引用。
///
/// 返回的 [`BtReferences`] 使用 5 个 `BtBuf` + 1 个哈希：
/// - `a`：所有非符号引用的名称拼接（UTF-8 字节串，无分隔符）。
/// - `b`：每个引用名称在 `a` 中的结束偏移（i64 数组）。
/// - `c`：每个引用对应的 OID（[`BtOid`] 数组，tags 已 peel 到 commit）。
/// - `d`：所有符号引用的“名称 + symref 目标”拼接（UTF-8 字节串）。
/// - `e`：每个 symref 段在 `d` 中的结束偏移（i64 数组）。
/// - `hash`：用于变更检测的稳定哈希值，编码了引用集、OID 和 `include_tags` 标志。
///
/// # 参数
/// - `git_dir_path`：仓库 `.git` 目录（NUL 终止 UTF-8）。
/// - `include_tags`：非零时**排除** `refs/tags/*`（命名沿用原版语义，实际为“跳过 tags”）。
/// - `out_refs`：输出 [`BtReferences`]，调用前可未初始化。
///
/// # 返回值
/// - `0`：成功。
/// - `1`：参数非法或仓库/内存错误。
///
/// # 内存所有权
/// `out_refs` 中的所有 `BtBuf.ptr` 通过进程堆分配，
/// 必须用 [`bt_release_references`] 一次性释放，不能单独 `free`。
#[no_mangle]
pub unsafe extern "C" fn bt_get_references(
    git_dir_path: *const c_char,
    include_tags: u8,
    out_refs: *mut BtReferences,
) -> c_int {
    if git_dir_path.is_null() || out_refs.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    // Initialize output
    unsafe {
        (*out_refs).a = BtBuf { ptr: core::ptr::null_mut(), len: 0, cap: 0 };
        (*out_refs).b = BtBuf { ptr: core::ptr::null_mut(), len: 0, cap: 0 };
        (*out_refs).c = BtBuf { ptr: core::ptr::null_mut(), len: 0, cap: 0 };
        (*out_refs).d = BtBuf { ptr: core::ptr::null_mut(), len: 0, cap: 0 };
        (*out_refs).e = BtBuf { ptr: core::ptr::null_mut(), len: 0, cap: 0 };
        (*out_refs).hash = 0;
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
    let repo = match git2::Repository::open(&git_dir) {
        Ok(r) => r,
        Err(e) => {
            set_last_error_str(&format!("failed to open repository: {e}"));
            return 1;
        }
    };

    let mut refs = BTreeMap::new();

    // 1. Read packed references
    collect_packed_refs(&git_dir, &mut refs);

    // 2. Read loose references
    collect_loose_refs(&git_dir, "refs", &mut refs);

    // 3. Special ref: HEAD
    let head_path = git_dir.join("HEAD");
    if head_path.exists() {
        if let Ok(content_bytes) = std::fs::read(&head_path) {
            let head = String::from_utf8_lossy(&content_bytes).trim().to_string();
            if !head.is_empty() {
                let mut head_entry = NativeRefEntry {
                    name: "HEAD".to_string(),
                    symref: String::new(),
                    has_oid: false,
                    oid: BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 },
                    has_peeled_oid: false,
                    peeled_oid: BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 },
                };
                if head.starts_with("ref: ") {
                    head_entry.symref = head[5..].trim().to_string();
                } else {
                    if let Some(parsed) = parse_hex_oid(&head) {
                        head_entry.oid = parsed;
                        head_entry.has_oid = true;
                    }
                }
                if head_entry.has_oid || !head_entry.symref.is_empty() {
                    add_or_update_ref(&mut refs, head_entry);
                }
            }
        }
    }

    // Collect special heads
    let special_heads = [
        "ORIG_HEAD",
        "FETCH_HEAD",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_HEAD",
    ];
    for &sh in &special_heads {
        let path = git_dir.join(sh);
        if path.exists() {
            if let Ok(content_bytes) = std::fs::read(&path) {
                let text = String::from_utf8_lossy(&content_bytes).trim().to_string();
                if !text.is_empty() {
                    let mut entry = NativeRefEntry {
                        name: sh.to_string(),
                        symref: String::new(),
                        has_oid: false,
                        oid: BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 },
                        has_peeled_oid: false,
                        peeled_oid: BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 },
                    };
                    if text.starts_with("ref: ") {
                        entry.symref = text[5..].trim().to_string();
                    } else {
                        if let Some(parsed) = parse_hex_oid(&text) {
                            entry.oid = parsed;
                            entry.has_oid = true;
                        }
                    }
                    if entry.has_oid || !entry.symref.is_empty() {
                        add_or_update_ref(&mut refs, entry);
                    }
                }
            }
        }
    }

    let mut names_data = String::new();
    let mut name_offsets = Vec::new();
    let mut oids = Vec::new();
    let mut symrefs_data = String::new();
    let mut symref_offsets = Vec::new();

    let skip_tags = include_tags != 0;

    for (_, entry) in &refs {
        let ref_name = &entry.name;
        if ref_name == "FETCH_HEAD" || ref_name == "MERGE_HEAD" {
            continue;
        }
        if !entry.symref.is_empty() {
            symrefs_data.push_str(ref_name);
            symref_offsets.push(symrefs_data.len() as i64);
            symrefs_data.push_str(&entry.symref);
            symref_offsets.push(symrefs_data.len() as i64);
            continue;
        }
        if skip_tags && ref_name.starts_with("refs/tags/") {
            continue;
        }
        if !entry.has_oid {
            continue;
        }
        let mut oid = entry.oid;
        if ref_name.starts_with("refs/tags/") {
            if entry.has_peeled_oid {
                oid = entry.peeled_oid;
            } else {
                oid = peel_tag_object(&repo, entry.oid);
            }
        }
        names_data.push_str(ref_name);
        name_offsets.push(names_data.len() as i64);
        oids.push(oid);
    }

    if !assign_bytes(unsafe { &mut (*out_refs).a }, &names_data, names_data.capacity()) ||
       !assign_vector(unsafe { &mut (*out_refs).b }, &name_offsets, name_offsets.capacity()) ||
       !assign_vector(unsafe { &mut (*out_refs).c }, &oids, oids.capacity()) ||
       !assign_bytes(unsafe { &mut (*out_refs).d }, &symrefs_data, symrefs_data.capacity()) ||
       !assign_vector(unsafe { &mut (*out_refs).e }, &symref_offsets, symref_offsets.capacity()) {
        bt_release_references(out_refs);
        set_last_error_str("insufficient memory");
        return 1;
    }

    unsafe {
        (*out_refs).hash = legacy_reference_hash(&repo, &refs, include_tags);
    }

    0
}

fn legacy_reference_hash(
    repo: &git2::Repository,
    refs: &BTreeMap<String, NativeRefEntry>,
    include_tags: u8,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    let entries = refs
        .values()
        .filter(|entry| entry.name != "FETCH_HEAD" && entry.name != "MERGE_HEAD")
        .collect::<Vec<_>>();
    write_usize(&mut hasher, entries.len());
    for entry in entries {
        write_len_prefixed_bytes(&mut hasher, entry.name.as_bytes());
        if entry.symref.is_empty() {
            write_u64(&mut hasher, 0);
            let oid = if entry.name.starts_with("refs/tags/") {
                if entry.has_peeled_oid {
                    entry.peeled_oid
                } else {
                    peel_tag_object(repo, entry.oid)
                }
            } else {
                entry.oid
            };
            hasher.write(&oid.to_bytes());
        } else {
            write_u64(&mut hasher, 1);
            write_len_prefixed_bytes(&mut hasher, entry.symref.as_bytes());
        }
    }
    hasher.write(&[include_tags]);
    hasher.finish()
}

fn write_len_prefixed_bytes(hasher: &mut DefaultHasher, bytes: &[u8]) {
    write_usize(hasher, bytes.len());
    hasher.write(bytes);
}

fn write_usize(hasher: &mut DefaultHasher, value: usize) {
    hasher.write(&value.to_ne_bytes());
}

fn write_u64(hasher: &mut DefaultHasher, value: u64) {
    hasher.write(&value.to_ne_bytes());
}

unsafe fn assign_bytes(buf: &mut BtBuf, data: &str, cap: usize) -> bool {
    if data.is_empty() {
        buf.ptr = core::ptr::null_mut();
        buf.len = 0;
        buf.cap = 0;
        return true;
    }
    let bytes = data.as_bytes();
    let cap = cap.max(bytes.len());
    let ptr = heap_alloc(cap);
    if ptr.is_null() {
        return false;
    }
    core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
    buf.ptr = ptr as *mut _;
    buf.len = bytes.len();
    buf.cap = cap;
    true
}

unsafe fn assign_vector<T: Copy>(buf: &mut BtBuf, values: &[T], cap: usize) -> bool {
    if values.is_empty() {
        buf.ptr = core::ptr::null_mut();
        buf.len = 0;
        buf.cap = 0;
        return true;
    }
    let cap = cap.max(values.len());
    let bytes_len = cap * std::mem::size_of::<T>();
    let ptr = heap_alloc(bytes_len);
    if ptr.is_null() {
        return false;
    }
    core::ptr::copy_nonoverlapping(
        values.as_ptr() as *const u8,
        ptr,
        values.len() * std::mem::size_of::<T>(),
    );
    buf.ptr = ptr as *mut _;
    buf.len = values.len();
    buf.cap = cap;
    true
}

fn collect_loose_refs(
    git_dir: &Path,
    relative_dir: &str,
    refs: &mut BTreeMap<String, NativeRefEntry>,
) {
    let full_path = git_dir.join(relative_dir);
    if let Ok(entries) = std::fs::read_dir(full_path) {
        for entry in entries.flatten() {
            if let Ok(ft) = entry.file_type() {
                let name = entry.file_name().to_string_lossy().to_string();
                let child_relative = format!("{relative_dir}/{name}");
                if ft.is_dir() {
                    collect_loose_refs(git_dir, &child_relative, refs);
                } else {
                    if let Ok(content_bytes) = std::fs::read(entry.path()) {
                        let text = String::from_utf8_lossy(&content_bytes).trim().to_string();
                        let mut item = NativeRefEntry {
                            name: child_relative,
                            symref: String::new(),
                            has_oid: false,
                            oid: BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 },
                            has_peeled_oid: false,
                            peeled_oid: BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 },
                        };
                        if text.starts_with("ref: ") {
                            item.symref = text[5..].trim().to_string();
                        } else {
                            if let Some(parsed) = parse_hex_oid(&text) {
                                item.oid = parsed;
                                item.has_oid = true;
                            }
                        }
                        if item.has_oid || !item.symref.is_empty() {
                            add_or_update_ref(refs, item);
                        }
                    }
                }
            }
        }
    }
}

fn collect_packed_refs(
    git_dir: &Path,
    refs: &mut BTreeMap<String, NativeRefEntry>,
) {
    let packed_path = git_dir.join("packed-refs");
    if let Ok(content) = std::fs::read_to_string(&packed_path) {
        let mut last_ref = String::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.starts_with('^') {
                if !last_ref.is_empty() {
                    if let Some(peeled) = parse_hex_oid(&trimmed[1..]) {
                        let item = NativeRefEntry {
                            name: last_ref.clone(),
                            symref: String::new(),
                            has_oid: false,
                            oid: BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 },
                            has_peeled_oid: true,
                            peeled_oid: peeled,
                        };
                        add_or_update_ref(refs, item);
                    }
                }
                continue;
            }
            if let Some(space) = trimmed.find(' ') {
                let sha = &trimmed[..space];
                let ref_name = trimmed[space + 1..].trim();
                if let Some(oid) = parse_hex_oid(sha) {
                    if ref_name.starts_with("refs/") {
                        let item = NativeRefEntry {
                            name: ref_name.to_string(),
                            symref: String::new(),
                            has_oid: true,
                            oid,
                            has_peeled_oid: false,
                            peeled_oid: BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 },
                        };
                        add_or_update_ref(refs, item);
                        last_ref = ref_name.to_string();
                    }
                }
            }
        }
    }
}

fn add_or_update_ref(
    refs: &mut BTreeMap<String, NativeRefEntry>,
    entry: NativeRefEntry,
) {
    match refs.get_mut(&entry.name) {
        Some(existing) => {
            if !entry.symref.is_empty() {
                existing.symref = entry.symref;
            }
            if entry.has_oid {
                existing.oid = entry.oid;
                existing.has_oid = true;
            }
            if entry.has_peeled_oid {
                existing.peeled_oid = entry.peeled_oid;
                existing.has_peeled_oid = true;
            }
        }
        None => {
            refs.insert(entry.name.clone(), entry);
        }
    }
}

fn peel_tag_object(repo: &git2::Repository, oid: BtOid) -> BtOid {
    let raw_oid = oid.to_bytes();
    if let Ok(git2_oid) = git2::Oid::from_bytes(&raw_oid) {
        if let Ok(obj) = repo.find_object(git2_oid, None) {
            if let Ok(peeled) = obj.peel(git2::ObjectType::Commit) {
                let peeled_id = peeled.id();
                let bytes = peeled_id.as_bytes();
                return BtOid::from_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                    bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
                    bytes[16], bytes[17], bytes[18], bytes[19]
                ]);
            }
        }
    }
    oid
}

fn parse_hex_oid(hex40: &str) -> Option<BtOid> {
    let b = hex40.as_bytes();
    if b.len() < 40 {
        return None;
    }
    let mut parts = [0u32; 5];
    for p in 0..5 {
        let mut value = 0u32;
        for i in 0..8 {
            let c = b[p * 8 + i];
            let nibble = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return None,
            };
            value = (value << 4) | (nibble as u32);
        }
        parts[p] = value;
    }
    Some(BtOid {
        s0: parts[0],
        s1: parts[1],
        s2: parts[2],
        s3: parts[3],
        s4: parts[4],
    })
}

/// 释放 [`bt_get_references`] 返回的 [`BtReferences`]。
///
/// 会逐个释放 `a`..`e` 五个 `BtBuf.ptr`，**不会**清零结构体字段，
/// 调用方不应在释放后再访问这些字段。传入 `null` 安全。
///
/// # 内存所有权
/// 仅可释放由 [`bt_get_references`] 填充的 `BtReferences`。
#[no_mangle]
pub unsafe extern "C" fn bt_release_references(p: *mut BtReferences) {
    if p.is_null() {
        return;
    }
    
    let a_ptr = (*p).a.ptr;
    if !a_ptr.is_null() {
        heap_free(a_ptr);
    }

    let b_ptr = (*p).b.ptr;
    if !b_ptr.is_null() {
        heap_free(b_ptr);
    }

    let c_ptr = (*p).c.ptr;
    if !c_ptr.is_null() {
        heap_free(c_ptr);
    }

    let d_ptr = (*p).d.ptr;
    if !d_ptr.is_null() {
        heap_free(d_ptr);
    }

    let e_ptr = (*p).e.ptr;
    if !e_ptr.is_null() {
        heap_free(e_ptr);
    }
}
