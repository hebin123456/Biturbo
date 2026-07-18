//! 端到端测试：`bt_repository_manager` 的 TOML 配置加载与保存。
//!
//! 覆盖 `bt_get_repository_manager`（含文件不存在时返回空配置）、
//! `bt_save_repository_manager`（写入后读回验证）、释放函数安全性、
//! 以及 null 输入等场景。

use biturbo::ffi::bt_repository_manager::{
    bt_get_repository_manager, bt_release_repository_manager, bt_save_repository_manager,
    BtRepositoryManager,
};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

/// 创建一个零初始化的 `BtRepositoryManager` 作为 out 参数。
fn zeroed_manager() -> BtRepositoryManager {
    unsafe { std::mem::zeroed() }
}

/// 把 C 字符串指针数组转为 Rust `Vec<String>`。
/// 接受 `*mut *mut c_char` 以匹配 `BtRepositoryManager` 中字符串数组的字段类型。
unsafe fn cstr_array_to_strings(ptr: *mut *mut c_char, len: i64) -> Vec<String> {
    if ptr.is_null() || len <= 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len as usize {
        let p = *ptr.add(i);
        if p.is_null() {
            out.push(String::new());
        } else {
            out.push(CStr::from_ptr(p).to_string_lossy().into_owned());
        }
    }
    out
}

#[test]
fn get_nonexistent_file_returns_empty_success() {
    // 文件不存在时应返回 0（成功），且输出为空配置
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("nonexistent.toml");
    let path_c = CString::new(path.to_str().unwrap()).unwrap();
    let mut result = zeroed_manager();
    let rc = unsafe { bt_get_repository_manager(path_c.as_ptr(), &mut result) };
    assert_eq!(rc, 0, "文件不存在应返回 0");
    assert_eq!(result.scan_depth, 5, "默认 scan_depth 应为 5");
    assert_eq!(result.source_dirs_len, 0);
    assert_eq!(result.ignore_len, 0);
    assert_eq!(result.repositories_len, 0);
    // 无需释放（空配置不分配内存）
}

#[test]
fn save_and_get_roundtrip_single_repo() {
    // 保存单个仓库配置后读回，验证字段一致
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let path_c = CString::new(path.to_str().unwrap()).unwrap();

    let source_dir = CString::new("/home/user/repos").unwrap();
    let source_dirs: Vec<*const c_char> = vec![source_dir.as_ptr()];

    let repo_path = CString::new("/home/user/repos/project-a").unwrap();
    let repo_alias = CString::new("Project A").unwrap();
    let paths: Vec<*const c_char> = vec![repo_path.as_ptr()];
    let aliases: Vec<*const c_char> = vec![repo_alias.as_ptr()];
    let opened: Vec<u32> = vec![3];
    let colors: Vec<u8> = vec![1]; // Red

    let rc = unsafe {
        bt_save_repository_manager(
            path_c.as_ptr(),
            source_dirs.as_ptr(),
            source_dirs.len() as i64,
            7, // scan_depth
            ptr::null(),
            0,
            paths.as_ptr(),
            paths.len() as i64,
            aliases.as_ptr(),
            aliases.len() as i64,
            opened.as_ptr(),
            opened.len() as i64,
            colors.as_ptr(),
            colors.len() as i64,
        )
    };
    assert_eq!(rc, 0, "save 应返回 0");

    // 读回验证
    let mut result = zeroed_manager();
    let rc = unsafe { bt_get_repository_manager(path_c.as_ptr(), &mut result) };
    assert_eq!(rc, 0, "get 应返回 0");
    assert_eq!(result.scan_depth, 7, "scan_depth 应为 7");
    assert_eq!(result.source_dirs_len, 1);
    assert_eq!(result.repositories_len, 1);

    // 验证 source_dirs
    let sd = unsafe { cstr_array_to_strings(result.source_dirs, result.source_dirs_len) };
    assert_eq!(sd, vec!["/home/user/repos"]);

    // 验证 repository 条目
    assert!(!result.repositories.is_null());
    let repo = unsafe { &*result.repositories };
    let p_str = unsafe { CStr::from_ptr(repo.path).to_string_lossy().into_owned() };
    assert_eq!(p_str, "/home/user/repos/project-a");
    let a_str = unsafe { CStr::from_ptr(repo.alias).to_string_lossy().into_owned() };
    assert_eq!(a_str, "Project A");
    assert_eq!(repo.opened, 3);
    assert_eq!(repo.color, 1);

    unsafe { bt_release_repository_manager(&mut result) };
}

#[test]
fn save_and_get_multiple_repos_preserve_order() {
    // 多个仓库应保持顺序与颜色
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("multi.toml");
    let path_c = CString::new(path.to_str().unwrap()).unwrap();

    let p1 = CString::new("/r1").unwrap();
    let p2 = CString::new("/r2").unwrap();
    let p3 = CString::new("/r3").unwrap();
    let paths: Vec<*const c_char> = vec![p1.as_ptr(), p2.as_ptr(), p3.as_ptr()];

    let a1 = CString::new("one").unwrap();
    let a2 = CString::new("two").unwrap();
    let a3 = CString::new("three").unwrap();
    let aliases: Vec<*const c_char> = vec![a1.as_ptr(), a2.as_ptr(), a3.as_ptr()];

    let opened: Vec<u32> = vec![1, 2, 3];
    let colors: Vec<u8> = vec![1, 4, 6]; // Red, Green, Violet

    let rc = unsafe {
        bt_save_repository_manager(
            path_c.as_ptr(),
            ptr::null(),
            0,
            5,
            ptr::null(),
            0,
            paths.as_ptr(),
            paths.len() as i64,
            aliases.as_ptr(),
            aliases.len() as i64,
            opened.as_ptr(),
            opened.len() as i64,
            colors.as_ptr(),
            colors.len() as i64,
        )
    };
    assert_eq!(rc, 0);

    let mut result = zeroed_manager();
    unsafe {
        bt_get_repository_manager(path_c.as_ptr(), &mut result);
    }
    assert_eq!(result.repositories_len, 3);
    for i in 0..3 {
        let repo = unsafe { &*result.repositories.add(i) };
        let alias = unsafe { CStr::from_ptr(repo.alias).to_string_lossy().into_owned() };
        assert_eq!(alias, ["one", "two", "three"][i]);
        assert_eq!(repo.opened, (i as u32) + 1);
        assert_eq!(repo.color, [1u8, 4, 6][i]);
    }
    unsafe { bt_release_repository_manager(&mut result) };
}

#[test]
fn save_with_ignore_rules_roundtrip() {
    // ignore 规则应正确保存与读回
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ignore.toml");
    let path_c = CString::new(path.to_str().unwrap()).unwrap();

    let ig1 = CString::new("*.tmp").unwrap();
    let ig2 = CString::new("node_modules/").unwrap();
    let ignore: Vec<*const c_char> = vec![ig1.as_ptr(), ig2.as_ptr()];

    let rc = unsafe {
        bt_save_repository_manager(
            path_c.as_ptr(),
            ptr::null(),
            0,
            3,
            ignore.as_ptr(),
            ignore.len() as i64,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
        )
    };
    assert_eq!(rc, 0);

    let mut result = zeroed_manager();
    unsafe {
        bt_get_repository_manager(path_c.as_ptr(), &mut result);
    }
    assert_eq!(result.scan_depth, 3);
    assert_eq!(result.ignore_len, 2);
    let ig = unsafe { cstr_array_to_strings(result.ignore, result.ignore_len) };
    assert_eq!(ig, vec!["*.tmp", "node_modules/"]);
    unsafe { bt_release_repository_manager(&mut result) };
}

#[test]
fn get_null_path_returns_error() {
    // path 为 null 应返回 1
    let mut result = zeroed_manager();
    let rc = unsafe { bt_get_repository_manager(ptr::null(), &mut result) };
    assert_eq!(rc, 1, "null path 应返回 1");
}

#[test]
fn get_null_out_result_returns_error() {
    // out_result 为 null 应返回 1
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("x.toml");
    let path_c = CString::new(path.to_str().unwrap()).unwrap();
    let rc = unsafe { bt_get_repository_manager(path_c.as_ptr(), ptr::null_mut()) };
    assert_eq!(rc, 1, "null out_result 应返回 1");
}

#[test]
fn save_null_path_returns_error() {
    // path 为 null 应返回 1
    let rc = unsafe {
        bt_save_repository_manager(
            ptr::null(),
            ptr::null(),
            0,
            5,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
        )
    };
    assert_eq!(rc, 1, "null path 应返回 1");
}

#[test]
fn release_null_is_safe() {
    // 传入 null 应直接返回
    unsafe { bt_release_repository_manager(ptr::null_mut()) };
}

#[test]
fn release_after_empty_get_is_safe() {
    // 文件不存在时 get 返回空配置（无内存分配），release 应安全
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("empty.toml");
    let path_c = CString::new(path.to_str().unwrap()).unwrap();
    let mut result = zeroed_manager();
    unsafe {
        bt_get_repository_manager(path_c.as_ptr(), &mut result);
        bt_release_repository_manager(&mut result);
    }
}

#[test]
fn save_empty_config_loads_defaults() {
    // 保存空配置后读回应得到默认 scan_depth=5、空列表
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("empty2.toml");
    let path_c = CString::new(path.to_str().unwrap()).unwrap();

    let rc = unsafe {
        bt_save_repository_manager(
            path_c.as_ptr(),
            ptr::null(),
            0,
            5,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
        )
    };
    assert_eq!(rc, 0);

    let mut result = zeroed_manager();
    unsafe {
        bt_get_repository_manager(path_c.as_ptr(), &mut result);
    }
    assert_eq!(result.scan_depth, 5);
    assert_eq!(result.source_dirs_len, 0);
    assert_eq!(result.ignore_len, 0);
    assert_eq!(result.repositories_len, 0);
    unsafe { bt_release_repository_manager(&mut result) };
}
