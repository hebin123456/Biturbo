//! # FFI 子模块集合
//!
//! 本模块聚合了所有面向 C ABI 的导出函数实现。各子模块按职责拆分，
//! 共同组成 `biturbo.dll` 的 `bt_*` 系列接口及兼容 `libgit2.dll` 的辅助符号。
//!
//! 通用约定：
//! - 所有 FFI out 参数返回的缓冲区均通过 Windows 进程堆分配，
//!   必须使用对应的 `bt_release_*` 函数释放，不能与 C 的 `free` 混用。
//! - 错误码统一为 `BT_OK=0`、`BT_ERR=1`、`BT_ERR_CANCELED=2`（具体以各文件常量为准）。
//! - 失败时可通过 [`bt_error`] 模块的 `bt_get_last_error_message` 取回最近一次错误描述。

/// 取消令牌的创建、取消与释放。
pub mod bt_cancellation;
/// 提交图遍历、子图查询、behind/ahead 计数与提交搜索。
pub mod bt_commits;
/// 跨 FFI 边界的线程本地错误信息读写辅助。
pub mod bt_error;
/// 提交者时间戳批量查询。
pub mod bt_committer_times;
/// 内嵌图像（TGA）解码为 BMP。
pub mod bt_decode_image;
/// Git 配置文件解析与释放。
pub mod bt_git_config;
/// Git 树对象条目读取与释放。
pub mod bt_get_tree;
/// HEAD 引用读取与释放。
pub mod bt_head;
/// 基于 diff 文本的轻量级语法高亮。
pub mod bt_highlight_syntax;
/// 矩形 Treemap 布局算法。
pub mod bt_layout_treemap;
/// Unified diff / patch 文本词法解析。
pub mod bt_parse_patch;
/// 子进程派生、管道读写与取消令牌管理。
pub mod bt_process;
/// 仓库引用（refs）枚举与释放。
pub mod bt_references;
/// 仓库管理器（TOML 配置）的加载、保存与释放。
pub mod bt_repository_manager;
/// Revision 头信息（作者、时间、主题）批量读取与释放。
pub mod bt_revision_headers;
/// Git stash 列表读取与释放。
pub mod bt_stashes;
/// Tag 详情读取与释放。
pub mod bt_tags;
/// Markdown 转 HTML 渲染与释放。
pub mod bt_markdown;
/// 40 字符十六进制 SHA-1 字符串解析为 20 字节 OID。
pub mod bt_oid;
/// 通用 `BtBuf` 缓冲区释放函数集合（多个导出共享同一实现）。
pub mod bt_release_vec;
/// 错误信息线程本地存储的内部实现。
pub mod error;
/// 跨 FFI 边界共享的 C 兼容类型定义（`BtOid`、`BtBuf` 等）。
pub mod types;
/// Windows 进程堆（kernel32 `HeapAlloc`/`HeapFree`）封装。
pub mod winheap;
/// 强制链接 `libz-sys`，确保 zlib 符号被导出。
pub mod zlib_touch;
