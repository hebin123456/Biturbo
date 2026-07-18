//! # Tag 详情读取
//!
//! 提供 [`bt_get_tag_details`] / [`bt_release_tag_details`]：
//! 根据 OID 读取 annotated tag 对象，返回 tagger 信息、tag 名称与 tag message。

use crate::ffi::error::set_last_error_str;
use crate::ffi::types::BtOid;
use crate::ffi::winheap::{heap_alloc_c_string, heap_free};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::Path;

/// annotated tag 详情。
///
/// # 字段
/// - `tag_object_oid`：tag 指向的目标对象 OID（已 `peel` 到的最终对象）。
/// - `tagger_name` / `tagger_email`：tagger 名字 / 邮箱（NUL 终止 UTF-8）。
/// - `tagger_time`：tagger 时间戳（Unix 秒）。
/// - `name`：tag 名称（NUL 终止 UTF-8）。
/// - `message`：tag message（NUL 终止 UTF-8，已 trim）。
///
/// # 内存所有权
/// 所有 `*mut c_char` 字段均通过进程堆分配，必须用 [`bt_release_tag_details`] 释放。
#[repr(C)]
pub struct BtTagDetails {
    pub tag_object_oid: BtOid,
    pub tagger_name: *mut c_char,
    pub tagger_email: *mut c_char,
    pub tagger_time: i64,
    pub name: *mut c_char,
    pub message: *mut c_char,
}

/// 读取给定 OID 对应的 annotated tag 详情。
///
/// # 参数
/// - `git_dir_path`：仓库 `.git` 目录（NUL 终止 UTF-8）。
/// - `tag_oid`：待读取的 tag OID（按值传递，与原版 ABI 一致）。
/// - `out_result`：输出 [`BtTagDetails`]，调用前可未初始化。
///
/// # 返回值
/// - `0`：成功。
/// - `1`：参数非法或仓库/tag/内存错误。
///
/// # 内存所有权
/// 输出的 4 个字符串字段均通过进程堆分配，必须用 [`bt_release_tag_details`] 释放。
#[no_mangle]
pub unsafe extern "C" fn bt_get_tag_details(
    git_dir_path: *const c_char,
    tag_oid: BtOid,
    out_result: *mut BtTagDetails,
) -> c_int {
    if git_dir_path.is_null() || out_result.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_result).tag_object_oid = BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 };
        (*out_result).tagger_name = core::ptr::null_mut();
        (*out_result).tagger_email = core::ptr::null_mut();
        (*out_result).tagger_time = 0;
        (*out_result).name = core::ptr::null_mut();
        (*out_result).message = core::ptr::null_mut();
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

    let raw_oid = tag_oid.to_bytes();
    let git2_oid = match git2::Oid::from_bytes(&raw_oid) {
        Ok(o) => o,
        Err(e) => {
            set_last_error_str(&format!("failed to parse OID: {e}"));
            return 1;
        }
    };

    let tag = match repo.find_tag(git2_oid) {
        Ok(t) => t,
        Err(e) => {
            set_last_error_str(&format!("failed to find tag: {e}"));
            return 1;
        }
    };

    let target_id = tag.target_id();
    let bytes = target_id.as_bytes();
    let target_oid = BtOid::from_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16], bytes[17], bytes[18], bytes[19]
    ]);

    let tagger = tag.tagger();
    let mut tagger_name_str = String::new();
    let mut tagger_email_str = String::new();
    let mut tagger_time = 0i64;
    if let Some(t) = &tagger {
        tagger_name_str = t.name().unwrap_or("").to_string();
        tagger_email_str = t.email().unwrap_or("").to_string();
        tagger_time = t.when().seconds();
    }

    let name = tag.name().unwrap_or("");
    let message = tag.message().ok().flatten().unwrap_or("");

    let tagger_name_ptr = unsafe { heap_alloc_c_string(&tagger_name_str) };
    let tagger_email_ptr = unsafe { heap_alloc_c_string(&tagger_email_str) };
    let name_ptr = unsafe { heap_alloc_c_string(name) };
    let message_ptr = unsafe { heap_alloc_c_string(message.trim()) };

    unsafe {
        (*out_result).tag_object_oid = target_oid;
        (*out_result).tagger_name = tagger_name_ptr;
        (*out_result).tagger_email = tagger_email_ptr;
        (*out_result).tagger_time = tagger_time;
        (*out_result).name = name_ptr;
        (*out_result).message = message_ptr;
    }

    0
}

/// 释放 [`bt_get_tag_details`] 返回的 [`BtTagDetails`] 中的字符串字段。
///
/// 会逐个释放 `tagger_name` / `tagger_email` / `name` / `message`；
/// 不释放 `BtTagDetails` 结构体本身。传入 `null` 安全。
///
/// # 内存所有权
/// 仅可释放由 [`bt_get_tag_details`] 填充的字段。
#[no_mangle]
pub unsafe extern "C" fn bt_release_tag_details(p: *mut BtTagDetails) {
    if p.is_null() {
        return;
    }
    unsafe {
        let tagger_name = (*p).tagger_name;
        let tagger_email = (*p).tagger_email;
        let name = (*p).name;
        let message = (*p).message;

        if !tagger_name.is_null() {
            heap_free(tagger_name as _);
        }
        if !tagger_email.is_null() {
            heap_free(tagger_email as _);
        }
        if !name.is_null() {
            heap_free(name as _);
        }
        if !message.is_null() {
            heap_free(message as _);
        }
    }
}
