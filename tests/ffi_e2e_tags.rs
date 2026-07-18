//! 端到端测试：`bt_tags` 的 annotated tag 详情读取。
//!
//! 覆盖 annotated tag 详情读取、tagger 信息、lightweight tag 失败、
//! null 输入、释放函数安全性等场景。

mod common;

use biturbo::ffi::bt_tags::{bt_get_tag_details, bt_release_tag_details, BtTagDetails};
use common::{git_oid_to_btoid, make_test_repo};
use std::ffi::CStr;
use std::ptr;

/// 创建一个零初始化的 `BtTagDetails` 作为 out 参数。
fn zeroed_details() -> BtTagDetails {
    unsafe { std::mem::zeroed() }
}

#[test]
fn get_tag_details_annotated_tag() {
    // 创建 annotated tag，读取详情，验证各字段
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    let tag_oid = repo.create_annotated_tag("v1.0.0", "release 1.0.0", commit_oid);

    let git_dir = repo.git_dir_cstr();
    let tag_btoid = git_oid_to_btoid(tag_oid);
    let mut details = zeroed_details();
    let rc = unsafe { bt_get_tag_details(git_dir.as_ptr(), tag_btoid, &mut details) };
    assert_eq!(rc, 0, "annotated tag 详情应返回 0");

    // tag_object_oid 应指向 commit
    let expected_commit = git_oid_to_btoid(commit_oid);
    assert_eq!(
        details.tag_object_oid.to_bytes(),
        expected_commit.to_bytes(),
        "tag_object_oid 应匹配 commit OID"
    );

    // name 应为 "v1.0.0"
    assert!(!details.name.is_null());
    let name = unsafe { CStr::from_ptr(details.name).to_string_lossy().into_owned() };
    assert_eq!(name, "v1.0.0");

    // message 应为 "release 1.0.0"
    assert!(!details.message.is_null());
    let message = unsafe { CStr::from_ptr(details.message).to_string_lossy().into_owned() };
    assert_eq!(message, "release 1.0.0");

    unsafe { bt_release_tag_details(&mut details) };
}

#[test]
fn get_tag_details_tagger_info() {
    // 验证 tagger_name / tagger_email / tagger_time
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    let tag_oid = repo.create_annotated_tag("v2.0", "tag v2", commit_oid);

    let git_dir = repo.git_dir_cstr();
    let tag_btoid = git_oid_to_btoid(tag_oid);
    let mut details = zeroed_details();
    unsafe {
        bt_get_tag_details(git_dir.as_ptr(), tag_btoid, &mut details);
    }

    // tagger_name 应为 "测试用户"（由 make_test_repo 设置）
    assert!(!details.tagger_name.is_null());
    let tagger_name = unsafe { CStr::from_ptr(details.tagger_name).to_string_lossy().into_owned() };
    assert_eq!(tagger_name, "测试用户");

    // tagger_email 应为 "test@example.com"
    assert!(!details.tagger_email.is_null());
    let tagger_email = unsafe { CStr::from_ptr(details.tagger_email).to_string_lossy().into_owned() };
    assert_eq!(tagger_email, "test@example.com");

    // tagger_time 应为正数（当前 Unix 时间戳）
    assert!(details.tagger_time > 0, "tagger_time 应为正数");

    unsafe { bt_release_tag_details(&mut details) };
}

#[test]
fn get_tag_details_lightweight_tag_fails() {
    // lightweight tag 不是 tag 对象，find_tag 应失败
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    // create_lightweight_tag 返回的是 commit OID（无独立 tag 对象）
    let tag_ref_oid = repo.create_lightweight_tag("lw-tag", commit_oid);

    let git_dir = repo.git_dir_cstr();
    let tag_btoid = git_oid_to_btoid(tag_ref_oid);
    let mut details = zeroed_details();
    let rc = unsafe { bt_get_tag_details(git_dir.as_ptr(), tag_btoid, &mut details) };
    assert_eq!(rc, 1, "lightweight tag 不是 tag 对象，应返回 1");
}

#[test]
fn get_tag_details_nonexistent_oid_fails() {
    // 不存在的 OID 应返回 1
    let repo = make_test_repo();
    let git_dir = repo.git_dir_cstr();
    // 全零 OID（不可能存在）
    let zero_oid = biturbo::ffi::types::BtOid {
        s0: 0,
        s1: 0,
        s2: 0,
        s3: 0,
        s4: 0,
    };
    let mut details = zeroed_details();
    let rc = unsafe { bt_get_tag_details(git_dir.as_ptr(), zero_oid, &mut details) };
    assert_eq!(rc, 1, "不存在的 OID 应返回 1");
}

#[test]
fn get_tag_details_null_path_returns_error() {
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    let tag_oid = repo.create_annotated_tag("null-path", "test", commit_oid);
    let tag_btoid = git_oid_to_btoid(tag_oid);
    let mut details = zeroed_details();
    let rc = unsafe { bt_get_tag_details(ptr::null(), tag_btoid, &mut details) };
    assert_eq!(rc, 1, "null path 应返回 1");
}

#[test]
fn get_tag_details_null_out_returns_error() {
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    let tag_oid = repo.create_annotated_tag("null-out", "test", commit_oid);
    let git_dir = repo.git_dir_cstr();
    let tag_btoid = git_oid_to_btoid(tag_oid);
    let rc = unsafe { bt_get_tag_details(git_dir.as_ptr(), tag_btoid, ptr::null_mut()) };
    assert_eq!(rc, 1, "null out 应返回 1");
}

#[test]
fn release_tag_details_null_is_safe() {
    unsafe { bt_release_tag_details(ptr::null_mut()) };
}

#[test]
fn release_tag_details_after_get() {
    // 正常 get 后 release 应不泄漏
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    let tag_oid = repo.create_annotated_tag("release-test", "msg", commit_oid);
    let git_dir = repo.git_dir_cstr();
    let tag_btoid = git_oid_to_btoid(tag_oid);
    let mut details = zeroed_details();
    unsafe {
        bt_get_tag_details(git_dir.as_ptr(), tag_btoid, &mut details);
        bt_release_tag_details(&mut details);
    }
}

#[test]
fn get_tag_details_message_trimmed() {
    // message 应被 trim（首尾空白被去除）
    let repo = make_test_repo();
    let commit_oid = repo.head_oid();
    let tag_oid = repo.create_annotated_tag("trim-tag", "  trimmed message  ", commit_oid);
    let git_dir = repo.git_dir_cstr();
    let tag_btoid = git_oid_to_btoid(tag_oid);
    let mut details = zeroed_details();
    unsafe {
        bt_get_tag_details(git_dir.as_ptr(), tag_btoid, &mut details);
    }
    let message = unsafe { CStr::from_ptr(details.message).to_string_lossy().into_owned() };
    assert_eq!(message, "trimmed message", "message 应被 trim");
    unsafe { bt_release_tag_details(&mut details) };
}
