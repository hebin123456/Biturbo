//! 端到端测试：`bt_head` 的 HEAD 引用读取。
//!
//! 覆盖符号引用（`ref: refs/heads/...`）、detached HEAD、
//! null 输入、无效仓库、释放函数安全性等场景。

mod common;

use biturbo::ffi::bt_head::{bt_get_head, bt_release_head, BtHead};
use common::make_test_repo;
use std::ffi::CStr;
use std::ptr;

/// 创建一个零初始化的 `BtHead` 作为 out 参数（`_pad` 为私有，无法用字面量构造）。
fn zeroed_head() -> BtHead {
    unsafe { std::mem::zeroed() }
}

/// 把 `git2::Oid` 的 20 字节按 4 字节组内反序，得到 `BtHead.oid20` 的预期字节序。
fn expected_swapped_oid20(oid: git2::Oid) -> [u8; 20] {
    let raw = oid.as_bytes();
    let mut out = [0u8; 20];
    for word in 0..5 {
        let base = word * 4;
        out[base + 0] = raw[base + 3];
        out[base + 1] = raw[base + 2];
        out[base + 2] = raw[base + 1];
        out[base + 3] = raw[base + 0];
    }
    out
}

#[test]
fn get_head_symbolic_ref_returns_ref_name() {
    // 普通 repo 的 HEAD 是符号引用 `ref: refs/heads/<branch>`
    let repo = make_test_repo();
    let git_dir = repo.git_dir_cstr();
    let mut head = zeroed_head();
    let rc = unsafe { bt_get_head(git_dir.as_ptr(), &mut head) };
    assert_eq!(rc, 0, "get_head 应返回 0");

    // 符号引用时 oid20 应全零
    assert_eq!(head.oid20, [0u8; 20], "符号引用时 oid20 应全零");

    // ref_name 不为 null，且以 "refs/heads/" 开头
    assert!(!head.ref_name.is_null(), "符号引用时 ref_name 不应为 null");
    let name = unsafe { CStr::from_ptr(head.ref_name).to_string_lossy().into_owned() };
    assert!(
        name.starts_with("refs/heads/"),
        "ref_name 应以 refs/heads/ 开头，实际: {:?}",
        name
    );

    unsafe { bt_release_head(&mut head) };
}

#[test]
fn get_head_detached_returns_oid() {
    // detach HEAD 后，oid20 应填充（组内反序），ref_name 应为 null
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    repo.detach_head(commit_oid);

    let git_dir = repo.git_dir_cstr();
    let mut head = zeroed_head();
    let rc = unsafe { bt_get_head(git_dir.as_ptr(), &mut head) };
    assert_eq!(rc, 0, "detached HEAD 应返回 0");

    // ref_name 应为 null
    assert!(head.ref_name.is_null(), "detached HEAD 时 ref_name 应为 null");

    // oid20 应与预期组内反序一致
    let expected = expected_swapped_oid20(commit_oid);
    assert_eq!(head.oid20, expected, "oid20 字节序应为组内反序");

    unsafe { bt_release_head(&mut head) };
}

#[test]
fn get_head_oid_nonzero_when_detached() {
    // detached HEAD 时 oid20 不应全零
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    repo.detach_head(commit_oid);

    let git_dir = repo.git_dir_cstr();
    let mut head = zeroed_head();
    unsafe {
        bt_get_head(git_dir.as_ptr(), &mut head);
    }
    assert!(head.oid20.iter().any(|&b| b != 0), "oid20 不应全零");
    unsafe { bt_release_head(&mut head) };
}

#[test]
fn get_head_null_path_returns_error() {
    // git_dir_path 为 null 应返回 1
    let mut head = zeroed_head();
    let rc = unsafe { bt_get_head(ptr::null(), &mut head) };
    assert_eq!(rc, 1, "null path 应返回 1");
}

#[test]
fn get_head_null_out_returns_error() {
    // out_head 为 null 应返回 1
    let repo = make_test_repo();
    let git_dir = repo.git_dir_cstr();
    let rc = unsafe { bt_get_head(git_dir.as_ptr(), ptr::null_mut()) };
    assert_eq!(rc, 1, "null out 应返回 1");
}

#[test]
fn get_head_invalid_repo_returns_error() {
    // 不存在的 .git 目录应返回 1
    let temp = tempfile::tempdir().unwrap();
    let fake_git_dir = temp.path().join("not_a_git_dir");
    std::fs::create_dir_all(&fake_git_dir).unwrap();
    let path_c = std::ffi::CString::new(fake_git_dir.to_str().unwrap()).unwrap();
    let mut head = zeroed_head();
    let rc = unsafe { bt_get_head(path_c.as_ptr(), &mut head) };
    assert_eq!(rc, 1, "无效仓库应返回 1");
}

#[test]
fn release_head_null_is_safe() {
    // 传入 null 应直接返回
    unsafe { bt_release_head(ptr::null_mut()) };
}

#[test]
fn release_head_after_symbolic_ref() {
    // 符号引用 get 后 release，应释放 ref_name
    let repo = make_test_repo();
    let git_dir = repo.git_dir_cstr();
    let mut head = zeroed_head();
    unsafe {
        bt_get_head(git_dir.as_ptr(), &mut head);
        bt_release_head(&mut head);
    }
    // 释放后 ref_name 应为 null
    assert!(head.ref_name.is_null(), "释放后 ref_name 应为 null");
}

#[test]
fn release_head_after_detached() {
    // detached HEAD 时 ref_name 本就为 null，release 应安全（no-op）
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    repo.detach_head(commit_oid);
    let git_dir = repo.git_dir_cstr();
    let mut head = zeroed_head();
    unsafe {
        bt_get_head(git_dir.as_ptr(), &mut head);
        bt_release_head(&mut head);
    }
    assert!(head.ref_name.is_null());
}

#[test]
fn release_head_double_release_safe() {
    // 重复释放应安全（第二次 ref_name 已为 null）
    let repo = make_test_repo();
    let git_dir = repo.git_dir_cstr();
    let mut head = zeroed_head();
    unsafe {
        bt_get_head(git_dir.as_ptr(), &mut head);
        bt_release_head(&mut head);
        bt_release_head(&mut head);
    }
}
