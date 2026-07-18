//! 集成测试通用辅助模块。
//!
//! 提供 `TestRepo`：在临时目录中创建真实 Git 仓库，并暴露常用的提交、
//! 分支、tag 创建辅助方法。每个 `tests/ffi_e2e_*.rs` 通过 `mod common;` 引入。
//!
//! 由于每个集成测试文件以独立 crate 形式编译本模块，不同测试仅使用辅助方法的
//! 不同子集，故全局允许 dead_code 以避免大量“未使用”告警。

#![allow(dead_code)]

use biturbo::ffi::types::BtOid;
use git2::{IndexAddOption, Oid, Repository, Signature};
use std::ffi::CString;
use std::path::PathBuf;

/// 测试用 Git 仓库，持有 `Repository` 与临时目录（保持存活）。
///
/// `_temp` 字段保持临时目录存活；`TestRepo` 被 drop 后临时目录自动清理。
pub struct TestRepo {
    /// 已打开的 git2 仓库句柄。
    pub repo: Repository,
    /// 临时目录（drop 时自动删除）。
    pub _temp: tempfile::TempDir,
}

impl TestRepo {
    /// 返回 `.git` 目录的绝对路径字符串。
    pub fn git_dir_string(&self) -> String {
        self.repo
            .path()
            .to_str()
            .expect("git 路径应为 UTF-8")
            .to_string()
    }

    /// 返回 `.git` 目录路径的 `CString`，可直接传给 FFI 函数。
    pub fn git_dir_cstr(&self) -> CString {
        CString::new(self.git_dir_string()).expect("路径不应含 NUL")
    }

    /// 返回工作目录的绝对路径。
    pub fn workdir(&self) -> PathBuf {
        self.repo.workdir().expect("应有工作目录").to_path_buf()
    }

    /// 写入一个文件到工作目录（不 commit）。
    pub fn write_file(&self, name: &str, content: &str) {
        let path = self.workdir().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("创建目录失败");
        }
        std::fs::write(&path, content).expect("写入文件失败");
    }

    /// 把工作目录中所有变更加入索引并 commit 到 HEAD，返回新 commit OID。
    pub fn commit_all(&self, message: &str) -> Oid {
        let sig = Signature::now("测试用户", "test@example.com").expect("签名创建失败");
        let mut index = self.repo.index().expect("获取索引失败");
        index
            .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .expect("add_all 失败");
        index.write().expect("写入索引失败");
        let tree_oid = index.write_tree().expect("write_tree 失败");
        let tree = self.repo.find_tree(tree_oid).expect("查找 tree 失败");

        // 首次 commit 时 HEAD 不存在，parents 为空
        let head = self.repo.head().ok();
        // 将 peeled commit 提到 match 外部，保证 parents 的借用在其使用期内有效
        let head_commit = head.as_ref().map(|h| h.peel_to_commit().expect("peel 失败"));
        let parents: Vec<&git2::Commit> = match &head_commit {
            Some(c) => vec![c],
            None => vec![],
        };

        self.repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .expect("commit 失败")
    }

    /// 在指定 commit 上创建一个分支。
    pub fn create_branch(&self, name: &str, commit_oid: Oid) {
        let commit = self.repo.find_commit(commit_oid).expect("查找 commit 失败");
        self.repo.branch(name, &commit, false).expect("创建分支失败");
    }

    /// 创建 annotated tag 指向指定 commit，返回 tag 对象的 OID。
    pub fn create_annotated_tag(&self, name: &str, message: &str, commit_oid: Oid) -> Oid {
        let sig = Signature::now("测试用户", "test@example.com").expect("签名创建失败");
        let commit = self.repo.find_commit(commit_oid).expect("查找 commit 失败");
        self.repo
            .tag(name, &commit.as_object(), &sig, message, false)
            .expect("创建 annotated tag 失败")
    }

    /// 创建 lightweight tag 指向指定 commit。
    pub fn create_lightweight_tag(&self, name: &str, commit_oid: Oid) -> Oid {
        let obj = self.repo.find_object(commit_oid, None).expect("查找对象失败");
        self.repo
            .tag_lightweight(name, &obj, false)
            .expect("创建 lightweight tag 失败")
    }

    /// 把 HEAD 切到 detached，指向指定 commit。
    pub fn detach_head(&self, commit_oid: Oid) {
        self.repo
            .set_head_detached(commit_oid)
            .expect("detach HEAD 失败");
    }

    /// 返回 HEAD 指向的 commit OID。
    pub fn head_oid(&self) -> Oid {
        self.repo
            .head()
            .expect("HEAD 不存在")
            .peel_to_commit()
            .expect("peel 失败")
            .id()
    }

    /// 返回 HEAD commit 的 tree OID。
    pub fn head_tree_oid(&self) -> Oid {
        self.repo
            .head()
            .expect("HEAD 不存在")
            .peel_to_tree()
            .expect("peel 失败")
            .id()
    }
}

/// 创建一个带初始 commit（含一个文件）的测试仓库。
pub fn make_test_repo() -> TestRepo {
    let temp = tempfile::tempdir().expect("创建临时目录失败");
    let repo = Repository::init(&temp).expect("初始化仓库失败");

    // 设置 user.name / user.email，避免 commit 失败
    {
        let mut config = repo.config().expect("获取 config 失败");
        config
            .set_str("user.name", "测试用户")
            .expect("设置 user.name 失败");
        config
            .set_str("user.email", "test@example.com")
            .expect("设置 user.email 失败");
    }

    let tr = TestRepo { repo, _temp: temp };
    tr.write_file("README.md", "# 测试仓库\n");
    tr.commit_all("初始提交");
    tr
}

/// 把 `git2::Oid` 转换为 `BtOid`。
pub fn git_oid_to_btoid(oid: Oid) -> BtOid {
    let bytes = oid.as_bytes();
    BtOid::from_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16], bytes[17], bytes[18], bytes[19],
    ])
}

/// 从 `BtOid` 转回 `git2::Oid`。
pub fn btoid_to_git_oid(oid: &BtOid) -> Oid {
    Oid::from_bytes(&oid.to_bytes()).expect("BtOid 转 git2::Oid 失败")
}
