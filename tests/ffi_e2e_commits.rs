//! 端到端测试：`bt_commits` 的提交图遍历与查询。
//!
//! 覆盖缓存生命周期、`bt_get_commits`、`bt_get_commit_subgraph`、
//! `bt_get_commit_subgraph_2`、`bt_get_behind_ahead_counts`、
//! `bt_find_fartherest_tip`、`bt_search_commits`，以及释放函数安全性。

mod common;

use biturbo::ffi::bt_commits::{
    bt_find_fartherest_tip, bt_get_behind_ahead_counts, bt_get_commit_subgraph,
    bt_get_commit_subgraph_2, bt_get_commits, bt_new_commit_graph_cache,
    bt_release_commit_graph_cache, bt_release_commit_storage, bt_search_commits,
    BtBehindAheadCounts, BtCommitStorage, BtOidPair, BtSearchCommitsResult,
};
use biturbo::ffi::bt_release_vec::{bt_release_behind_ahead_counts, bt_release_search_commits};
use biturbo::ffi::types::{BtBuf, BtOid};
use common::{git_oid_to_btoid, make_test_repo};
use std::ffi::CString;
use std::ptr;

/// 创建一个零初始化的 `BtCommitStorage` 作为 out 参数。
fn zeroed_storage() -> BtCommitStorage {
    unsafe { std::mem::zeroed() }
}

/// 创建一个包含 3 个线性提交的仓库，返回 (c1, c2, c3) 的 git2::Oid。
fn make_linear_repo() -> (common::TestRepo, git2::Oid, git2::Oid, git2::Oid) {
    let repo = make_test_repo(); // c1
    let c1 = repo.head_oid();
    repo.write_file("file2.txt", "content2");
    let c2 = repo.commit_all("second commit");
    repo.write_file("file3.txt", "content3");
    let c3 = repo.commit_all("third commit");
    (repo, c1, c2, c3)
}

// ---------- 缓存生命周期 ----------

#[test]
fn cache_lifecycle_create_and_release() {
    // 创建缓存后 inner 不为 null，释放后应为 null
    let id = CString::new("/tmp/test").unwrap();
    let cache = unsafe { bt_new_commit_graph_cache(id.as_ptr()) };
    assert!(!cache.is_null(), "新建缓存不应为 null");
    let mut cache = cache;
    unsafe { bt_release_commit_graph_cache(&mut cache) };
    assert!(cache.is_null(), "释放后缓存应为 null");
}

#[test]
fn cache_release_null_is_safe() {
    unsafe { bt_release_commit_graph_cache(ptr::null_mut()) };
}

#[test]
fn cache_create_with_null_identifier() {
    // null identifier 应仍能创建缓存（内部用空字符串）
    let cache = unsafe { bt_new_commit_graph_cache(ptr::null()) };
    assert!(!cache.is_null(), "null identifier 仍应创建缓存");
    let mut cache = cache;
    unsafe { bt_release_commit_graph_cache(&mut cache) };
}

#[test]
fn cache_double_release_safe() {
    let id = CString::new("/tmp/test2").unwrap();
    let mut cache = unsafe { bt_new_commit_graph_cache(id.as_ptr()) };
    unsafe { bt_release_commit_graph_cache(&mut cache) };
    // 第二次释放：cache 已为 null，应安全返回
    unsafe { bt_release_commit_graph_cache(&mut cache) };
    assert!(cache.is_null());
}

// ---------- bt_get_commits ----------

#[test]
fn get_commits_single_tip_returns_commits() {
    // 单 tip 调用应返回至少 1 个提交组
    let (repo, _c1, _c2, c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let tip = git_oid_to_btoid(c3);
    let tips = [tip];

    let mut result = zeroed_storage();
    let rc = unsafe {
        bt_get_commits(
            git_dir.as_ptr(),
            tips.as_ptr(),
            tips.len() as i64,
            0,      // date_order
            100,    // page_size
            0,      // skip_pages
            1,      // min_pages
            ptr::null(),
            0,
            ptr::null_mut(), // cache_handle
            ptr::null_mut(), // cancellation_token
            &mut result,
        )
    };
    assert_eq!(rc, 0, "get_commits 应返回 0");
    assert!(result.indexes_len >= 1, "应至少返回 1 个提交组");
    assert!(!result.oids.is_null(), "oids 不应为 null");
    assert!(!result.indexes.is_null(), "indexes 不应为 null");

    unsafe { bt_release_commit_storage(&mut result) };
}

#[test]
fn get_commits_null_out_returns_error() {
    let (repo, _c1, _c2, c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let tip = git_oid_to_btoid(c3);
    let tips = [tip];
    let rc = unsafe {
        bt_get_commits(
            git_dir.as_ptr(),
            tips.as_ptr(),
            tips.len() as i64,
            0,
            100,
            0,
            1,
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    assert_eq!(rc, 1, "null out 应返回 1");
}

#[test]
fn get_commits_empty_tips_returns_error() {
    // tips 和 required_oids 都为空时应返回 1
    let (repo, _c1, _c2, _c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let mut result = zeroed_storage();
    let rc = unsafe {
        bt_get_commits(
            git_dir.as_ptr(),
            ptr::null(),
            0,
            0,
            100,
            0,
            1,
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut result,
        )
    };
    assert_eq!(rc, 1, "空 tips 应返回 1");
}

#[test]
fn get_commits_with_cache_handle() {
    // 使用共享缓存句柄调用 get_commits
    let (repo, _c1, _c2, c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let tip = git_oid_to_btoid(c3);
    let tips = [tip];

    let id = CString::new(repo.git_dir_string()).unwrap();
    let mut cache = unsafe { bt_new_commit_graph_cache(id.as_ptr()) };

    let mut result = zeroed_storage();
    let rc = unsafe {
        bt_get_commits(
            git_dir.as_ptr(),
            tips.as_ptr(),
            tips.len() as i64,
            0,
            100,
            0,
            1,
            ptr::null(),
            0,
            &mut cache,
            ptr::null_mut(),
            &mut result,
        )
    };
    assert_eq!(rc, 0, "使用缓存应返回 0");
    assert!(result.indexes_len >= 1);

    unsafe {
        bt_release_commit_storage(&mut result);
        bt_release_commit_graph_cache(&mut cache);
    }
}

#[test]
fn get_commits_first_oid_is_tip() {
    // 第一组的第一个 OID 应为 tip
    let (repo, _c1, _c2, c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let tip = git_oid_to_btoid(c3);
    let tips = [tip];

    let mut result = zeroed_storage();
    unsafe {
        bt_get_commits(
            git_dir.as_ptr(),
            tips.as_ptr(),
            tips.len() as i64,
            0,
            100,
            0,
            1,
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut result,
        );
    }
    // 第一组的起始索引是 indexes[0] = 0，第一个 OID 是 oids[0]
    let first_oid = unsafe { *result.oids };
    assert_eq!(
        first_oid.to_bytes(),
        tip.to_bytes(),
        "第一个 OID 应为 tip"
    );
    unsafe { bt_release_commit_storage(&mut result) };
}

// ---------- bt_get_commit_subgraph ----------

#[test]
fn get_commit_subgraph_returns_commits() {
    let (repo, _c1, _c2, c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let tip = git_oid_to_btoid(c3);

    let mut result = zeroed_storage();
    let rc = unsafe {
        bt_get_commit_subgraph(git_dir.as_ptr(), &tip, ptr::null_mut(), &mut result)
    };
    assert_eq!(rc, 0, "get_commit_subgraph 应返回 0");
    assert!(result.indexes_len >= 1, "应至少返回 1 个提交组");

    unsafe { bt_release_commit_storage(&mut result) };
}

#[test]
fn get_commit_subgraph_null_oid_returns_error() {
    let (repo, _c1, _c2, _c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let mut result = zeroed_storage();
    let rc = unsafe {
        bt_get_commit_subgraph(git_dir.as_ptr(), ptr::null(), ptr::null_mut(), &mut result)
    };
    assert_eq!(rc, 1, "null oid 应返回 1");
}

// ---------- bt_get_commit_subgraph_2 ----------

#[test]
fn get_commit_subgraph_2_range() {
    // subgraph_2(src=c1, dst=c3) 应返回 c3, c2（不含 c1）
    let (repo, c1, _c2, c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let src = git_oid_to_btoid(c1);
    let dst = git_oid_to_btoid(c3);

    let mut result = zeroed_storage();
    let rc = unsafe {
        bt_get_commit_subgraph_2(git_dir.as_ptr(), &src, &dst, ptr::null_mut(), &mut result)
    };
    assert_eq!(rc, 0, "get_commit_subgraph_2 应返回 0");
    assert!(result.indexes_len >= 1, "应至少返回 1 个提交组");

    unsafe { bt_release_commit_storage(&mut result) };
}

#[test]
fn get_commit_subgraph_2_null_src_returns_error() {
    let (repo, _c1, _c2, c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let dst = git_oid_to_btoid(c3);
    let mut result = zeroed_storage();
    let rc = unsafe {
        bt_get_commit_subgraph_2(git_dir.as_ptr(), ptr::null(), &dst, ptr::null_mut(), &mut result)
    };
    assert_eq!(rc, 1, "null src 应返回 1");
}

#[test]
fn get_commit_subgraph_2_null_dst_returns_error() {
    let (repo, c1, _c2, _c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let src = git_oid_to_btoid(c1);
    let mut result = zeroed_storage();
    let rc = unsafe {
        bt_get_commit_subgraph_2(git_dir.as_ptr(), &src, ptr::null(), ptr::null_mut(), &mut result)
    };
    assert_eq!(rc, 1, "null dst 应返回 1");
}

// ---------- bt_get_behind_ahead_counts ----------

/// 创建一个有分叉历史的仓库：
/// c1 -> c2 (master)
/// c1 -> c3 (feature)
fn make_diverged_repo() -> (common::TestRepo, git2::Oid, git2::Oid, git2::Oid) {
    let repo = make_test_repo(); // c1 on master
    let c1 = repo.head_oid();

    // master: c1 -> c2
    repo.write_file("master.txt", "m");
    let c2 = repo.commit_all("master commit");

    // feature: c1 -> c3（detach 到 c1 后提交）
    repo.detach_head(c1);
    repo.write_file("feature.txt", "f");
    let c3 = repo.commit_all("feature commit");

    (repo, c1, c2, c3)
}

#[test]
fn get_behind_ahead_counts_diverged() {
    // c2 相对 c3：ahead=1 (c2), behind=1 (c3)
    let (repo, _c1, c2, c3) = make_diverged_repo();
    let git_dir = repo.git_dir_cstr();
    let pairs = [BtOidPair {
        left: git_oid_to_btoid(c2),
        right: git_oid_to_btoid(c3),
    }];

    let mut result = BtBehindAheadCounts {
        items: ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_get_behind_ahead_counts(
            git_dir.as_ptr(),
            pairs.as_ptr(),
            pairs.len() as i64,
            ptr::null_mut(),
            &mut result,
        )
    };
    assert_eq!(rc, 0, "get_behind_ahead_counts 应返回 0");
    assert_eq!(result.items_len, 1, "应返回 1 个计数");

    let item = unsafe { *result.items };
    assert_eq!(item.left, 1, "ahead (c2 相对 c3) 应为 1");
    assert_eq!(item.right, 1, "behind (c2 相对 c3) 应为 1");

    unsafe { bt_release_behind_ahead_counts(&mut result as *mut BtBehindAheadCounts as *mut BtBuf) };
}

#[test]
fn get_behind_ahead_counts_same_commit() {
    // 同一 commit 相对自身：ahead=0, behind=0
    let (repo, _c1, _c2, c3) = make_diverged_repo();
    let git_dir = repo.git_dir_cstr();
    let oid = git_oid_to_btoid(c3);
    let pairs = [BtOidPair { left: oid, right: oid }];

    let mut result = BtBehindAheadCounts {
        items: ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    unsafe {
        bt_get_behind_ahead_counts(
            git_dir.as_ptr(),
            pairs.as_ptr(),
            pairs.len() as i64,
            ptr::null_mut(),
            &mut result,
        );
    }
    let item = unsafe { *result.items };
    assert_eq!(item.left, 0, "同一 commit ahead 应为 0");
    assert_eq!(item.right, 0, "同一 commit behind 应为 0");

    unsafe { bt_release_behind_ahead_counts(&mut result as *mut BtBehindAheadCounts as *mut BtBuf) };
}

#[test]
fn get_behind_ahead_counts_empty_pairs_returns_zero() {
    // 空对列表应返回 0（成功，空结果）
    let (repo, _c1, _c2, _c3) = make_diverged_repo();
    let git_dir = repo.git_dir_cstr();
    let mut result = BtBehindAheadCounts {
        items: ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_get_behind_ahead_counts(
            git_dir.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            &mut result,
        )
    };
    assert_eq!(rc, 0, "空对列表应返回 0");
    assert_eq!(result.items_len, 0);
}

#[test]
fn get_behind_ahead_counts_null_out_returns_error() {
    let (repo, _c1, c2, c3) = make_diverged_repo();
    let git_dir = repo.git_dir_cstr();
    let pairs = [BtOidPair {
        left: git_oid_to_btoid(c2),
        right: git_oid_to_btoid(c3),
    }];
    let rc = unsafe {
        bt_get_behind_ahead_counts(
            git_dir.as_ptr(),
            pairs.as_ptr(),
            pairs.len() as i64,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    assert_eq!(rc, 1, "null out 应返回 1");
}

// ---------- bt_find_fartherest_tip ----------

/// 创建分叉历史：
/// c1 -> c2 (master)
/// c1 -> c3 -> c4 (feature)
fn make_multi_tip_repo() -> (common::TestRepo, git2::Oid, git2::Oid, git2::Oid, git2::Oid) {
    let repo = make_test_repo(); // c1
    let c1 = repo.head_oid();

    // master: c1 -> c2
    repo.write_file("m.txt", "m");
    let c2 = repo.commit_all("master 2");

    // feature: c1 -> c3 -> c4
    repo.detach_head(c1);
    repo.write_file("f1.txt", "f1");
    let c3 = repo.commit_all("feature 1");
    repo.write_file("f2.txt", "f2");
    let c4 = repo.commit_all("feature 2");

    (repo, c1, c2, c3, c4)
}

#[test]
fn find_fartherest_tip_picks_most_ahead() {
    // tips = [c2, c4], base = c1
    // c2 ahead of c1 = 1, c4 ahead of c1 = 2
    // 最远 tip 应为 c4
    let (repo, c1, c2, _c3, c4) = make_multi_tip_repo();
    let git_dir = repo.git_dir_cstr();
    let tips = [git_oid_to_btoid(c2), git_oid_to_btoid(c4)];
    let base = git_oid_to_btoid(c1);

    let mut out = BtOid {
        s0: 0,
        s1: 0,
        s2: 0,
        s3: 0,
        s4: 0,
    };
    let rc = unsafe {
        bt_find_fartherest_tip(
            git_dir.as_ptr(),
            ptr::null(),
            tips.as_ptr(),
            tips.len() as i64,
            &base,
            ptr::null_mut(),
            &mut out,
        )
    };
    assert_eq!(rc, 0, "find_fartherest_tip 应返回 0");
    let expected = git_oid_to_btoid(c4);
    assert_eq!(out.to_bytes(), expected.to_bytes(), "最远 tip 应为 c4");
}

#[test]
fn find_fartherest_tip_empty_tips_returns_base() {
    // 空 tips 时应返回 base_oid
    let (repo, c1, _c2, _c3, _c4) = make_multi_tip_repo();
    let git_dir = repo.git_dir_cstr();
    let base = git_oid_to_btoid(c1);

    let mut out = BtOid {
        s0: 0,
        s1: 0,
        s2: 0,
        s3: 0,
        s4: 0,
    };
    let rc = unsafe {
        bt_find_fartherest_tip(
            git_dir.as_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            &base,
            ptr::null_mut(),
            &mut out,
        )
    };
    assert_eq!(rc, 0, "空 tips 应返回 0");
    assert_eq!(out.to_bytes(), base.to_bytes(), "空 tips 时应返回 base");
}

#[test]
fn find_fartherest_tip_null_base_returns_error() {
    let (repo, _c1, c2, _c3, c4) = make_multi_tip_repo();
    let git_dir = repo.git_dir_cstr();
    let tips = [git_oid_to_btoid(c2), git_oid_to_btoid(c4)];

    let mut out = BtOid {
        s0: 0,
        s1: 0,
        s2: 0,
        s3: 0,
        s4: 0,
    };
    let rc = unsafe {
        bt_find_fartherest_tip(
            git_dir.as_ptr(),
            ptr::null(),
            tips.as_ptr(),
            tips.len() as i64,
            ptr::null(),
            ptr::null_mut(),
            &mut out,
        )
    };
    assert_eq!(rc, 1, "null base 应返回 1");
}

#[test]
fn find_fartherest_tip_null_out_returns_error() {
    let (repo, c1, c2, _c3, c4) = make_multi_tip_repo();
    let git_dir = repo.git_dir_cstr();
    let tips = [git_oid_to_btoid(c2), git_oid_to_btoid(c4)];
    let base = git_oid_to_btoid(c1);
    let rc = unsafe {
        bt_find_fartherest_tip(
            git_dir.as_ptr(),
            ptr::null(),
            tips.as_ptr(),
            tips.len() as i64,
            &base,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    assert_eq!(rc, 1, "null out 应返回 1");
}

// ---------- bt_search_commits ----------

#[test]
fn search_commits_by_message() {
    // 搜索 "second" 应匹配第二个 commit
    let (repo, _c1, c2, c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let oids = [git_oid_to_btoid(c2), git_oid_to_btoid(c3)];
    let query = CString::new("second").unwrap();

    let mut result = BtSearchCommitsResult {
        matches: ptr::null_mut(),
        matches_len: 0,
        matches_cap: 0,
    };
    let rc = unsafe {
        bt_search_commits(
            git_dir.as_ptr(),
            oids.as_ptr(),
            oids.len() as i64,
            query.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            &mut result,
        )
    };
    assert_eq!(rc, 0, "search_commits 应返回 0");
    assert_eq!(result.matches_len, 1, "应匹配 1 个 commit（含 'second'）");

    // 匹配的 OID 应为 c2
    let matched = unsafe { *result.matches };
    assert_eq!(matched.to_bytes(), git_oid_to_btoid(c2).to_bytes());

    unsafe { bt_release_search_commits(&mut result as *mut BtSearchCommitsResult as *mut BtBuf) };
}

#[test]
fn search_commits_empty_query_matches_all() {
    // 空 query 应匹配所有传入的 OID
    let (repo, _c1, c2, c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let oids = [git_oid_to_btoid(c2), git_oid_to_btoid(c3)];

    let mut result = BtSearchCommitsResult {
        matches: ptr::null_mut(),
        matches_len: 0,
        matches_cap: 0,
    };
    unsafe {
        bt_search_commits(
            git_dir.as_ptr(),
            oids.as_ptr(),
            oids.len() as i64,
            ptr::null(), // null query 等同于空字符串
            ptr::null(),
            0,
            ptr::null_mut(),
            &mut result,
        );
    }
    assert_eq!(result.matches_len, 2, "空 query 应匹配全部 2 个 commit");

    unsafe { bt_release_search_commits(&mut result as *mut BtSearchCommitsResult as *mut BtBuf) };
}

#[test]
fn search_commits_no_match_returns_empty() {
    // 不存在的关键词应返回 0 匹配
    let (repo, _c1, c2, c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let oids = [git_oid_to_btoid(c2), git_oid_to_btoid(c3)];
    let query = CString::new("nonexistent_keyword_xyz").unwrap();

    let mut result = BtSearchCommitsResult {
        matches: ptr::null_mut(),
        matches_len: 0,
        matches_cap: 0,
    };
    unsafe {
        bt_search_commits(
            git_dir.as_ptr(),
            oids.as_ptr(),
            oids.len() as i64,
            query.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            &mut result,
        );
    }
    assert_eq!(result.matches_len, 0, "不存在的关键词应返回 0 匹配");

    unsafe { bt_release_search_commits(&mut result as *mut BtSearchCommitsResult as *mut BtBuf) };
}

#[test]
fn search_commits_null_out_returns_error() {
    let (repo, _c1, c2, c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let oids = [git_oid_to_btoid(c2), git_oid_to_btoid(c3)];
    let query = CString::new("test").unwrap();
    let rc = unsafe {
        bt_search_commits(
            git_dir.as_ptr(),
            oids.as_ptr(),
            oids.len() as i64,
            query.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    assert_eq!(rc, 1, "null out 应返回 1");
}

#[test]
fn search_commits_empty_oids_returns_zero() {
    // 空 OID 列表应返回 0（成功，空结果）
    let (repo, _c1, _c2, _c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let query = CString::new("anything").unwrap();

    let mut result = BtSearchCommitsResult {
        matches: ptr::null_mut(),
        matches_len: 0,
        matches_cap: 0,
    };
    let rc = unsafe {
        bt_search_commits(
            git_dir.as_ptr(),
            ptr::null(),
            0,
            query.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            &mut result,
        )
    };
    assert_eq!(rc, 0, "空 OID 列表应返回 0");
    assert_eq!(result.matches_len, 0);
}

// ---------- 释放函数 ----------

#[test]
fn release_commit_storage_null_is_safe() {
    unsafe { bt_release_commit_storage(ptr::null_mut()) };
}

#[test]
fn release_commit_storage_after_get() {
    // 正常 get 后 release 应不泄漏
    let (repo, _c1, _c2, c3) = make_linear_repo();
    let git_dir = repo.git_dir_cstr();
    let tip = git_oid_to_btoid(c3);
    let tips = [tip];

    let mut result = zeroed_storage();
    unsafe {
        bt_get_commits(
            git_dir.as_ptr(),
            tips.as_ptr(),
            tips.len() as i64,
            0,
            100,
            0,
            1,
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut result,
        );
        bt_release_commit_storage(&mut result);
    }
}
