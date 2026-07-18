//! 端到端测试：`bt_get_tree` 的 Git 树对象读取。
//!
//! 覆盖 HEAD tree 读取、多文件 tree、子目录 tree、null 输入、
//! 释放函数安全性等场景。

mod common;

use biturbo::ffi::bt_get_tree::{bt_get_tree, bt_release_tree, BtTree};
use common::{git_oid_to_btoid, make_test_repo};
use std::ffi::CStr;
use std::ptr;

/// 创建一个零初始化的 `BtTree` 作为 out 参数。
fn zeroed_tree() -> BtTree {
    unsafe { std::mem::zeroed() }
}

/// 从 `BtTree` 读取 (filename, kind) 列表。
fn read_entries(tree: &BtTree) -> Vec<(String, u16, biturbo::ffi::types::BtOid)> {
    if tree.entries.is_null() || tree.entries_len <= 0 {
        return Vec::new();
    }
    let entries = unsafe { std::slice::from_raw_parts(tree.entries, tree.entries_len as usize) };
    entries
        .iter()
        .map(|e| {
            let name = unsafe { CStr::from_ptr(e.filename).to_string_lossy().into_owned() };
            (name, e.kind, e.treeish)
        })
        .collect()
}

#[test]
fn get_tree_head_returns_readme() {
    // HEAD tree 应包含 README.md
    let repo = make_test_repo();
    let tree_oid = repo.head_tree_oid();
    let bt_oid = git_oid_to_btoid(tree_oid);
    let git_dir = repo.git_dir_cstr();

    let mut tree = zeroed_tree();
    let rc = unsafe { bt_get_tree(git_dir.as_ptr(), &bt_oid, &mut tree) };
    assert_eq!(rc, 0, "get_tree 应返回 0");
    assert!(tree.entries_len >= 1, "应至少有 1 个条目");

    let entries = read_entries(&tree);
    let has_readme = entries.iter().any(|(n, _, _)| n == "README.md");
    assert!(has_readme, "应包含 README.md: {:?}", entries);

    unsafe { bt_release_tree(&mut tree) };
}

#[test]
fn get_tree_multiple_files() {
    // 添加多个文件后，tree 应包含所有顶层文件
    let repo = make_test_repo();
    repo.write_file("file1.txt", "content1");
    repo.write_file("file2.txt", "content2");
    repo.write_file("src/main.rs", "fn main() {}");
    repo.commit_all("添加多文件");

    let tree_oid = repo.head_tree_oid();
    let bt_oid = git_oid_to_btoid(tree_oid);
    let git_dir = repo.git_dir_cstr();

    let mut tree = zeroed_tree();
    unsafe {
        bt_get_tree(git_dir.as_ptr(), &bt_oid, &mut tree);
    }
    let entries = read_entries(&tree);
    let names: Vec<&str> = entries.iter().map(|(n, _, _)| n.as_str()).collect();
    assert!(names.contains(&"README.md"), "应包含 README.md: {:?}", names);
    assert!(names.contains(&"file1.txt"), "应包含 file1.txt: {:?}", names);
    assert!(names.contains(&"file2.txt"), "应包含 file2.txt: {:?}", names);
    assert!(names.contains(&"src"), "应包含 src 目录: {:?}", names);

    unsafe { bt_release_tree(&mut tree) };
}

#[test]
fn get_tree_regular_file_kind() {
    // 普通文件的 kind 应为 0o100644 (33188)
    let repo = make_test_repo();
    let tree_oid = repo.head_tree_oid();
    let bt_oid = git_oid_to_btoid(tree_oid);
    let git_dir = repo.git_dir_cstr();

    let mut tree = zeroed_tree();
    unsafe {
        bt_get_tree(git_dir.as_ptr(), &bt_oid, &mut tree);
    }
    let entries = read_entries(&tree);
    let readme = entries.iter().find(|(n, _, _)| n == "README.md");
    assert!(readme.is_some());
    let (_, kind, _) = readme.unwrap();
    assert_eq!(*kind, 0o100644, "普通文件 kind 应为 0o100644");

    unsafe { bt_release_tree(&mut tree) };
}

#[test]
fn get_tree_directory_kind() {
    // 目录的 kind 应为 0o040000 (16384)
    let repo = make_test_repo();
    repo.write_file("subdir/file.txt", "content");
    repo.commit_all("添加子目录");

    let tree_oid = repo.head_tree_oid();
    let bt_oid = git_oid_to_btoid(tree_oid);
    let git_dir = repo.git_dir_cstr();

    let mut tree = zeroed_tree();
    unsafe {
        bt_get_tree(git_dir.as_ptr(), &bt_oid, &mut tree);
    }
    let entries = read_entries(&tree);
    let subdir = entries.iter().find(|(n, _, _)| n == "subdir");
    assert!(subdir.is_some(), "应包含 subdir: {:?}", entries);
    let (_, kind, _) = subdir.unwrap();
    assert_eq!(*kind, 0o040000, "目录 kind 应为 0o040000");

    unsafe { bt_release_tree(&mut tree) };
}

#[test]
fn get_tree_entry_oid_valid() {
    // tree 条目的 treeish（子对象 OID）应能被 git2 解析回有效 OID
    let repo = make_test_repo();
    let tree_oid = repo.head_tree_oid();
    let bt_oid = git_oid_to_btoid(tree_oid);
    let git_dir = repo.git_dir_cstr();

    let mut tree = zeroed_tree();
    unsafe {
        bt_get_tree(git_dir.as_ptr(), &bt_oid, &mut tree);
    }
    let entries = read_entries(&tree);
    let readme = entries.iter().find(|(n, _, _)| n == "README.md");
    assert!(readme.is_some());
    let (_, _, treeish) = readme.unwrap();
    // treeish 应能转回 git2::Oid
    let git2_oid = git2::Oid::from_bytes(&treeish.to_bytes());
    assert!(git2_oid.is_ok(), "treeish 应为有效 OID");

    unsafe { bt_release_tree(&mut tree) };
}

#[test]
fn get_tree_null_path_returns_error() {
    let repo = make_test_repo();
    let tree_oid = repo.head_tree_oid();
    let bt_oid = git_oid_to_btoid(tree_oid);
    let mut tree = zeroed_tree();
    let rc = unsafe { bt_get_tree(ptr::null(), &bt_oid, &mut tree) };
    assert_eq!(rc, 1, "null path 应返回 1");
}

#[test]
fn get_tree_null_oid_returns_error() {
    let repo = make_test_repo();
    let git_dir = repo.git_dir_cstr();
    let mut tree = zeroed_tree();
    let rc = unsafe { bt_get_tree(git_dir.as_ptr(), ptr::null(), &mut tree) };
    assert_eq!(rc, 1, "null oid 应返回 1");
}

#[test]
fn get_tree_null_out_returns_error() {
    let repo = make_test_repo();
    let tree_oid = repo.head_tree_oid();
    let bt_oid = git_oid_to_btoid(tree_oid);
    let git_dir = repo.git_dir_cstr();
    let rc = unsafe { bt_get_tree(git_dir.as_ptr(), &bt_oid, ptr::null_mut()) };
    assert_eq!(rc, 1, "null out 应返回 1");
}

#[test]
fn get_tree_nonexistent_oid_fails() {
    // 不存在的 OID 应返回 1
    let repo = make_test_repo();
    let git_dir = repo.git_dir_cstr();
    let zero_oid = biturbo::ffi::types::BtOid {
        s0: 0,
        s1: 0,
        s2: 0,
        s3: 0,
        s4: 0,
    };
    let mut tree = zeroed_tree();
    let rc = unsafe { bt_get_tree(git_dir.as_ptr(), &zero_oid, &mut tree) };
    assert_eq!(rc, 1, "不存在的 OID 应返回 1");
}

#[test]
fn release_tree_null_is_safe() {
    unsafe { bt_release_tree(ptr::null_mut()) };
}

#[test]
fn release_tree_after_get_safe() {
    // 正常 get 后 release 应不泄漏
    let repo = make_test_repo();
    let tree_oid = repo.head_tree_oid();
    let bt_oid = git_oid_to_btoid(tree_oid);
    let git_dir = repo.git_dir_cstr();
    let mut tree = zeroed_tree();
    unsafe {
        bt_get_tree(git_dir.as_ptr(), &bt_oid, &mut tree);
        bt_release_tree(&mut tree);
    }
}
