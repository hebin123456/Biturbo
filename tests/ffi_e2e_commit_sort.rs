//! Sorting regressions for the public bt_get_commits ABI.

mod common;

use biturbo::ffi::bt_commits::{
    bt_get_commits, bt_new_commit_graph_cache, bt_release_commit_graph_cache,
    bt_release_commit_storage, BtCommitStorage,
};
use biturbo::ffi::types::BtOid;
use common::{git_oid_to_btoid, make_test_repo, TestRepo};
use std::{
    ffi::{c_void, CString},
    ptr,
};

fn commit_at(repo: &TestRepo, message: &str, time: i64, parents: &[git2::Oid]) -> git2::Oid {
    let tree = repo.repo.find_tree(repo.head_tree_oid()).unwrap();
    // Deliberately use a different author date: Git --date-order uses committer dates.
    let author = git2::Signature::new(
        "Author",
        "author@example.com",
        &git2::Time::new(1000 - time, 0),
    )
    .unwrap();
    let committer = git2::Signature::new(
        "Committer",
        "committer@example.com",
        &git2::Time::new(time, 0),
    )
    .unwrap();
    let parents: Vec<_> = parents
        .iter()
        .map(|oid| repo.repo.find_commit(*oid).unwrap())
        .collect();
    let refs: Vec<_> = parents.iter().collect();
    repo.repo
        .commit(None, &author, &committer, message, &tree, &refs)
        .unwrap()
}

struct History {
    repo: TestRepo,
    root: git2::Oid,
    a1: git2::Oid,
    a2: git2::Oid,
    b1: git2::Oid,
    b2: git2::Oid,
    merge: git2::Oid,
}

impl History {
    fn new() -> Self {
        let repo = make_test_repo();
        let root = commit_at(&repo, "root", 100, &[]);
        let a1 = commit_at(&repo, "a1", 200, &[root]);
        let a2 = commit_at(&repo, "a2", 400, &[a1]);
        let b1 = commit_at(&repo, "b1", 300, &[root]);
        let b2 = commit_at(&repo, "b2", 500, &[b1]);
        let merge = commit_at(&repo, "merge", 600, &[a2, b2]);
        Self {
            repo,
            root,
            a1,
            a2,
            b1,
            b2,
            merge,
        }
    }

    fn date_order(&self) -> Vec<git2::Oid> {
        vec![self.merge, self.b2, self.a2, self.b1, self.a1, self.root]
    }
}

fn read_commits(
    repo: &TestRepo,
    tips: &[git2::Oid],
    date_order: bool,
    required: &[git2::Oid],
    page_size: i64,
    skip_pages: i64,
    cache: *mut *mut c_void,
) -> (Vec<git2::Oid>, Vec<Vec<git2::Oid>>, bool) {
    let path = repo.git_dir_cstr();
    let tips: Vec<BtOid> = tips.iter().copied().map(git_oid_to_btoid).collect();
    let required: Vec<BtOid> = required.iter().copied().map(git_oid_to_btoid).collect();
    let mut result: BtCommitStorage = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        bt_get_commits(
            path.as_ptr(),
            tips.as_ptr(),
            tips.len() as i64,
            date_order as u8,
            page_size,
            skip_pages,
            1,
            required.as_ptr(),
            required.len() as i64,
            cache,
            ptr::null_mut(),
            &mut result,
        )
    };
    assert_eq!(rc, 0);
    let mut commits = Vec::new();
    let mut parents = Vec::new();
    // Empty pages can have null pointers; do not construct slices from them.
    if result.indexes_len > 0 {
        let flat = unsafe { std::slice::from_raw_parts(result.oids, result.oids_len as usize) };
        let indexes =
            unsafe { std::slice::from_raw_parts(result.indexes, result.indexes_len as usize) };
        for (i, &index) in indexes.iter().enumerate() {
            let start = index as usize;
            let end = indexes.get(i + 1).map_or(flat.len(), |next| *next as usize);
            commits.push(git2::Oid::from_bytes(&flat[start].to_bytes()).unwrap());
            parents.push(
                flat[start + 1..end]
                    .iter()
                    .map(|oid| git2::Oid::from_bytes(&oid.to_bytes()).unwrap())
                    .collect(),
            );
        }
    }
    let more = result.has_more != 0;
    unsafe { bt_release_commit_storage(&mut result) };
    (commits, parents, more)
}

#[test]
fn date_order_single_tip_interleaves_branches() {
    let h = History::new();
    let (commits, parents, more) =
        read_commits(&h.repo, &[h.merge], true, &[], 100, 0, ptr::null_mut());
    assert_eq!(commits, h.date_order());
    assert_eq!(parents[0], vec![h.a2, h.b2]);
    assert!(!more);
}

#[test]
fn date_order_multiple_tips_interleaves_branches() {
    let h = History::new();
    let expected = vec![h.b2, h.a2, h.b1, h.a1, h.root];
    for tips in [vec![h.a2, h.b2], vec![h.b2, h.a2], vec![h.a2, h.b2, h.b2]] {
        let (commits, _, _) = read_commits(&h.repo, &tips, true, &[], 100, 0, ptr::null_mut());
        assert_eq!(commits, expected);
    }
}

#[test]
fn date_order_required_head_uses_dates_and_ignores_page_limit() {
    let h = History::new();
    // ForkPlus supplies HEAD as a required OID during ordinary repository refreshes.
    let (commits, _, more) = read_commits(
        &h.repo,
        &[h.merge, h.merge],
        true,
        &[h.merge],
        1,
        0,
        ptr::null_mut(),
    );
    assert_eq!(commits, h.date_order());
    assert!(!more);
}

#[test]
fn date_order_pagination_preserves_complete_order() {
    let h = History::new();
    for tips in [vec![h.merge], vec![h.merge, h.merge]] {
        let mut all = Vec::new();
        for page in 0..3 {
            let (commits, _, more) =
                read_commits(&h.repo, &tips, true, &[], 2, page, ptr::null_mut());
            assert_eq!(commits.len(), 2);
            assert_eq!(more, page < 2);
            all.extend(commits);
        }
        assert_eq!(all, h.date_order());
        let (commits, _, more) = read_commits(&h.repo, &tips, true, &[], 2, 3, ptr::null_mut());
        assert!(commits.is_empty());
        assert!(!more);
    }
}

#[test]
fn date_order_keeps_parents_after_children_with_clock_skew() {
    let h = History::new();
    let child = commit_at(&h.repo, "backdated child", 50, &[h.a2]);
    let (commits, _, _) = read_commits(&h.repo, &[child], true, &[], 100, 0, ptr::null_mut());
    assert_eq!(commits, vec![child, h.a2, h.a1, h.root]);
}

#[test]
fn date_order_can_switch_with_shared_cache() {
    let h = History::new();
    let id = CString::new(h.repo.git_dir_string()).unwrap();
    let mut cache = unsafe { bt_new_commit_graph_cache(id.as_ptr()) };
    let tips = [h.merge, h.merge];
    let (topo, _, _) = read_commits(&h.repo, &tips, false, &[h.merge], 100, 0, &mut cache);
    let (dates, _, _) = read_commits(&h.repo, &tips, true, &[h.merge], 100, 0, &mut cache);
    let (topo_again, _, _) = read_commits(&h.repo, &tips, false, &[h.merge], 100, 0, &mut cache);
    unsafe { bt_release_commit_graph_cache(&mut cache) };
    assert_eq!(dates, h.date_order());
    assert_ne!(topo, dates);
    assert_eq!(topo, topo_again);
}

#[test]
fn date_order_matches_git_log() {
    let h = History::new();
    let backdated_child = commit_at(&h.repo, "backdated child", 50, &[h.a2]);
    for tips in [vec![h.merge], vec![h.a2, h.b2], vec![backdated_child, h.b2]] {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(h.repo.workdir())
            .args(["log", "--date-order", "--format=%H", "--no-show-signature"])
            .args(tips.iter().map(|oid| oid.to_string()))
            .arg("--")
            .output()
            .expect("Git must be available for the compatibility regression");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected: Vec<_> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| git2::Oid::from_str(line).unwrap())
            .collect();
        let (commits, _, _) = read_commits(&h.repo, &tips, true, &[], 100, 0, ptr::null_mut());
        assert_eq!(commits, expected);
    }
}
