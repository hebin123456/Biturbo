//! 端到端测试：`bt_references` 的仓库引用枚举。
//!
//! 覆盖符号引用（HEAD）、分支引用、tag 引用、`include_tags` 标志、
//! null 输入、释放函数安全性等场景。

mod common;

use biturbo::ffi::bt_references::{bt_get_references, bt_release_references};
use biturbo::ffi::types::{BtBuf, BtOid, BtReferences};
use common::make_test_repo;
use std::ptr;

/// 创建一个零初始化的 `BtReferences` 作为 out 参数。
fn zeroed_refs() -> BtReferences {
    unsafe { std::mem::zeroed() }
}

/// 从 `a`（名称拼接）和 `b`（结束偏移 i64 数组）解析出引用名称列表。
fn parse_names(a: &BtBuf, b: &BtBuf) -> Vec<String> {
    if b.ptr.is_null() || b.len == 0 {
        return Vec::new();
    }
    let a_bytes = unsafe { std::slice::from_raw_parts(a.ptr as *const u8, a.len) };
    let offsets = unsafe { std::slice::from_raw_parts(b.ptr as *const i64, b.len) };
    let mut names = Vec::with_capacity(offsets.len());
    let mut prev: usize = 0;
    for &end in offsets {
        let end = end as usize;
        let name = String::from_utf8_lossy(&a_bytes[prev..end]).into_owned();
        names.push(name);
        prev = end;
    }
    names
}

/// 从 `c`（BtOid 数组）解析出 OID 列表。
fn parse_oids(c: &BtBuf) -> Vec<BtOid> {
    if c.ptr.is_null() || c.len == 0 {
        return Vec::new();
    }
    let oids = unsafe { std::slice::from_raw_parts(c.ptr as *const BtOid, c.len) };
    oids.to_vec()
}

/// 从 `d`（symref 名称+目标拼接）和 `e`（成对偏移 i64 数组）解析出 (name, symref) 列表。
fn parse_symrefs(d: &BtBuf, e: &BtBuf) -> Vec<(String, String)> {
    if e.ptr.is_null() || e.len == 0 {
        return Vec::new();
    }
    let d_bytes = unsafe { std::slice::from_raw_parts(d.ptr as *const u8, d.len) };
    let offsets = unsafe { std::slice::from_raw_parts(e.ptr as *const i64, e.len) };
    let mut result = Vec::new();
    let mut prev: usize = 0;
    let mut i = 0;
    while i + 1 < offsets.len() {
        let name_end = offsets[i] as usize;
        let symref_end = offsets[i + 1] as usize;
        let name = String::from_utf8_lossy(&d_bytes[prev..name_end]).into_owned();
        let symref = String::from_utf8_lossy(&d_bytes[name_end..symref_end]).into_owned();
        result.push((name, symref));
        prev = symref_end;
        i += 2;
    }
    result
}

#[test]
fn get_references_returns_head_symref() {
    // 普通仓库的 HEAD 应作为符号引用出现在 d/e 中
    let repo = make_test_repo();
    let git_dir = repo.git_dir_cstr();
    let mut refs = zeroed_refs();
    let rc = unsafe { bt_get_references(git_dir.as_ptr(), 0, &mut refs) };
    assert_eq!(rc, 0, "get_references 应返回 0");

    let symrefs = parse_symrefs(&refs.d, &refs.e);
    let head_symref = symrefs
        .iter()
        .find(|(name, _)| name == "HEAD");
    assert!(head_symref.is_some(), "应包含 HEAD 符号引用");
    let (_, target) = head_symref.unwrap();
    assert!(
        target.starts_with("refs/heads/"),
        "HEAD symref 目标应以 refs/heads/ 开头，实际: {:?}",
        target
    );

    unsafe { bt_release_references(&mut refs) };
}

#[test]
fn get_references_includes_branch_ref() {
    // include_tags=0 时应包含分支引用
    let repo = make_test_repo();
    let git_dir = repo.git_dir_cstr();
    let mut refs = zeroed_refs();
    unsafe {
        bt_get_references(git_dir.as_ptr(), 0, &mut refs);
    }

    let names = parse_names(&refs.a, &refs.b);
    let oids = parse_oids(&refs.c);
    assert_eq!(names.len(), oids.len(), "名称数与 OID 数应一致");

    let has_branch = names.iter().any(|n| n.starts_with("refs/heads/"));
    assert!(has_branch, "应包含至少一个 refs/heads/ 引用: {:?}", names);

    unsafe { bt_release_references(&mut refs) };
}

#[test]
fn get_references_includes_tags_when_flag_zero() {
    // include_tags=0 时应包含 tag 引用
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    repo.create_lightweight_tag("v1.0", commit_oid);

    let git_dir = repo.git_dir_cstr();
    let mut refs = zeroed_refs();
    unsafe {
        bt_get_references(git_dir.as_ptr(), 0, &mut refs);
    }

    let names = parse_names(&refs.a, &refs.b);
    let has_tag = names.iter().any(|n| n == "refs/tags/v1.0");
    assert!(has_tag, "include_tags=0 应包含 refs/tags/v1.0: {:?}", names);

    unsafe { bt_release_references(&mut refs) };
}

#[test]
fn get_references_excludes_tags_when_flag_nonzero() {
    // include_tags!=0 时应排除 tag 引用
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    repo.create_lightweight_tag("v2.0", commit_oid);

    let git_dir = repo.git_dir_cstr();
    let mut refs = zeroed_refs();
    unsafe {
        bt_get_references(git_dir.as_ptr(), 1, &mut refs);
    }

    let names = parse_names(&refs.a, &refs.b);
    let has_tag = names.iter().any(|n| n.starts_with("refs/tags/"));
    assert!(!has_tag, "include_tags=1 应排除所有 tags: {:?}", names);

    // 仍应包含分支引用
    let has_branch = names.iter().any(|n| n.starts_with("refs/heads/"));
    assert!(has_branch, "仍应包含分支引用: {:?}", names);

    unsafe { bt_release_references(&mut refs) };
}

#[test]
fn get_references_oid_matches_commit() {
    // 分支引用的 OID 应与 HEAD commit OID 一致（tags 被 peel 到 commit）
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    let git_dir = repo.git_dir_cstr();
    let mut refs = zeroed_refs();
    unsafe {
        bt_get_references(git_dir.as_ptr(), 0, &mut refs);
    }

    let names = parse_names(&refs.a, &refs.b);
    let oids = parse_oids(&refs.c);
    let expected = common::git_oid_to_btoid(commit_oid);

    let branch_idx = names.iter().position(|n| n.starts_with("refs/heads/"));
    assert!(branch_idx.is_some());
    let oid = &oids[branch_idx.unwrap()];
    assert_eq!(oid.to_bytes(), expected.to_bytes(), "分支 OID 应匹配 HEAD commit");

    unsafe { bt_release_references(&mut refs) };
}

#[test]
fn get_references_hash_nonzero() {
    // hash 应为非零值（编码了引用集与 include_tags 标志）
    let repo = make_test_repo();
    let git_dir = repo.git_dir_cstr();
    let mut refs = zeroed_refs();
    unsafe {
        bt_get_references(git_dir.as_ptr(), 0, &mut refs);
    }
    assert_ne!(refs.hash, 0, "hash 不应为零");
    unsafe { bt_release_references(&mut refs) };
}

#[test]
fn get_references_hash_changes_with_include_tags() {
    // 同一仓库，include_tags=0 和 1 时 hash 应不同
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    repo.create_lightweight_tag("hash-test", commit_oid);
    let git_dir = repo.git_dir_cstr();

    let mut refs0 = zeroed_refs();
    unsafe { bt_get_references(git_dir.as_ptr(), 0, &mut refs0) };
    let hash0 = refs0.hash;
    unsafe { bt_release_references(&mut refs0) };

    let mut refs1 = zeroed_refs();
    unsafe { bt_get_references(git_dir.as_ptr(), 1, &mut refs1) };
    let hash1 = refs1.hash;
    unsafe { bt_release_references(&mut refs1) };

    assert_ne!(hash0, hash1, "include_tags 不同时 hash 应不同");
}

#[test]
fn get_references_null_path_returns_error() {
    let mut refs = zeroed_refs();
    let rc = unsafe { bt_get_references(ptr::null(), 0, &mut refs) };
    assert_eq!(rc, 1, "null path 应返回 1");
}

#[test]
fn get_references_null_out_returns_error() {
    let repo = make_test_repo();
    let git_dir = repo.git_dir_cstr();
    let rc = unsafe { bt_get_references(git_dir.as_ptr(), 0, ptr::null_mut()) };
    assert_eq!(rc, 1, "null out 应返回 1");
}

#[test]
fn release_references_null_is_safe() {
    unsafe { bt_release_references(ptr::null_mut()) };
}

#[test]
fn release_references_after_get_safe() {
    // 正常 get 后 release 应不泄漏、不崩溃
    let repo = make_test_repo();
    let git_dir = repo.git_dir_cstr();
    let mut refs = zeroed_refs();
    unsafe {
        bt_get_references(git_dir.as_ptr(), 0, &mut refs);
        bt_release_references(&mut refs);
    }
}
