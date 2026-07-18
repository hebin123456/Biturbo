//! # Git 树对象读取
//!
//! 提供 [`bt_get_tree`] / [`bt_release_tree`]：根据 OID 读取一个 Git tree
//! 对象的条目（文件名 + 模式 + 子对象 OID），并以扁平数组形式跨 FFI 边界返回。

use crate::ffi::error::set_last_error_str;
use crate::ffi::types::BtOid;
use crate::ffi::winheap::{heap_alloc, heap_alloc_c_string, heap_free};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::Path;

/// 单个 tree 条目。
///
/// # 字段
/// - `kind`：条目模式（`filemode`），低 16 位。如 `0o100644` → 文件，
///   `0o040000` → 目录（子树），`0o120000` → 符号链接。
/// - `_pad`：对齐填充，未使用。
/// - `filename`：NUL 终止的 UTF-8 文件名（进程堆分配）。
/// - `treeish`：子对象的 OID。
///
/// # 内存所有权
/// `filename` 与所在 [`BtTree`] 一并由 [`bt_release_tree`] 释放。
#[repr(C)]
pub struct BtTreeItem {
    pub kind: u16,
    _pad: u16,
    pub filename: *mut c_char,
    pub treeish: BtOid,
}

/// tree 条目扁平数组。
///
/// # 内存所有权
/// `entries` 通过进程堆分配，必须用 [`bt_release_tree`] 释放。
#[repr(C)]
pub struct BtTree {
    pub entries: *mut BtTreeItem,
    pub entries_len: i64,
    pub entries_cap: i64,
}

/// 读取给定 OID 对应的 tree 对象，输出所有顶层条目。
///
/// # 参数
/// - `git_dir_path`：仓库 `.git` 目录（NUL 终止 UTF-8）。
/// - `oid_ptr`：指向待读取 tree 的 OID；为 `null` 返回错误。
/// - `out_result`：输出 [`BtTree`]，调用前可未初始化。
///
/// # 返回值
/// - `0`：成功（包括空 tree）。
/// - `1`：参数非法或仓库/tree/内存错误。
///
/// # 内存所有权
/// 输出的 `entries` 数组及其中每个 `filename` 都通过进程堆分配，
/// 必须用 [`bt_release_tree`] 一次性释放。
#[no_mangle]
pub unsafe extern "C" fn bt_get_tree(
    git_dir_path: *const c_char,
    oid_ptr: *const BtOid,
    out_result: *mut BtTree,
) -> c_int {
    if git_dir_path.is_null() || oid_ptr.is_null() || out_result.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_result).entries = core::ptr::null_mut();
        (*out_result).entries_len = 0;
        (*out_result).entries_cap = 0;
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

    let raw_oid = unsafe { (*oid_ptr).to_bytes() };
    let git2_oid = match git2::Oid::from_bytes(&raw_oid) {
        Ok(o) => o,
        Err(e) => {
            set_last_error_str(&format!("failed to parse OID: {e}"));
            return 1;
        }
    };

    let tree = match repo.find_tree(git2_oid) {
        Ok(t) => t,
        Err(e) => {
            set_last_error_str(&format!("failed to find tree: {e}"));
            return 1;
        }
    };

    let mut entries: Vec<BtTreeItem> = Vec::new();
    for entry in tree.iter() {
        let name = entry.name().unwrap_or("");
        let filename_ptr = unsafe { heap_alloc_c_string(name) };
        if filename_ptr.is_null() {
            for ent in &entries {
                unsafe { heap_free(ent.filename as _) };
            }
            set_last_error_str("insufficient memory");
            return 1;
        }

        let child_id = entry.id();
        let bytes = child_id.as_bytes();
        let bt_oid = BtOid::from_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
            bytes[16], bytes[17], bytes[18], bytes[19]
        ]);

        entries.push(BtTreeItem {
            kind: entry.filemode() as u16,
            _pad: 0,
            filename: filename_ptr,
            treeish: bt_oid,
        });
    }

    if entries.is_empty() {
        return 0;
    }

    let alloc_bytes = entries.len() * std::mem::size_of::<BtTreeItem>();
    let entries_ptr = unsafe { heap_alloc(alloc_bytes) } as *mut BtTreeItem;
    if entries_ptr.is_null() {
        for ent in entries {
            unsafe { heap_free(ent.filename as _) };
        }
        set_last_error_str("insufficient memory");
        return 1;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(entries.as_ptr(), entries_ptr, entries.len());
        (*out_result).entries = entries_ptr;
        (*out_result).entries_len = entries.len() as i64;
        (*out_result).entries_cap = entries.len() as i64;
    }

    0
}

/// 释放 [`bt_get_tree`] 返回的 [`BtTree`]。
///
/// 会逐个释放每个 `BtTreeItem::filename`，最后释放 `entries` 数组本身。
/// 调用后结构体内的字段会被清零，重复释放安全。传入 `null` 安全。
///
/// # 内存所有权
/// 仅可释放由 [`bt_get_tree`] 填充的 `BtTree`。
#[no_mangle]
pub unsafe extern "C" fn bt_release_tree(p: *mut BtTree) {
    if p.is_null() {
        return;
    }
    let entries_ptr = std::ptr::replace(&mut (*p).entries, core::ptr::null_mut());
    let len = (*p).entries_len;
    let cap = (*p).entries_cap;
    (*p).entries_len = 0;
    (*p).entries_cap = 0;

    if !entries_ptr.is_null() {
        for i in 0..len {
            let ent = &mut *entries_ptr.add(i as usize);
            let filename = std::ptr::replace(&mut ent.filename, core::ptr::null_mut());
            if !filename.is_null() {
                heap_free(filename as _);
            }
        }
        if cap != 0 {
            heap_free(entries_ptr as _);
        }
    }
}
