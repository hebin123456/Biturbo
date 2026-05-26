# Biturbo

> A Rust-based Drop-in Replacement for Git for Windows' `libgit2.dll`

Biturbo 是一个用 Rust 编写的 `cdylib`，旨在兼容替代 Git for Windows 中的 `libgit2.dll`。它不仅提供了原始的 zlib 压缩/解压缩 API（经由 `libz-sys` 暴露），还在此基础上添加了一套高性能的 Git 仓库操作接口，包括提交图遍历、分支/标签管理、差异解析、语法高亮、Markdown 渲染、树形布局等能力。

## 特性

- **兼容 git2 / libgit2** – 导出了与原版 `libgit2.dll` 相同的 zlib 符号（`compress`、`deflate`、`inflate`、`adler32`、`crc32` 等）
- **高性能 Git 操作** – 提供 `bt_*` 系列的 C ABI 函数，涵盖提交查询、引用枚举、树对象读取、Tag 详情、Stash 管理、Revisions 头信息等
- **附加功能** – 语法高亮、Markdown → HTML 渲染（支持表格、围栏代码块、任务列表等）、树形图布局算法（矩形 Treemap）
- **进程管理** – 支持进程派生与管道通信（`bt_spawn_with_output` / `bt_spawn_with_callback`）、取消令牌机制
- **线程安全** – 使用线程本地错误缓冲区（`bt_get_last_error_message`），每个线程独立获取错误信息
- **跨平台构建** – 基于 Cargo 标准工具链，纯 Rust 实现

## 构建

### 前置条件

- [Rust toolchain](https://rustup.rs/) 1.70+
- CMake 与 C 编译器（用于构建 `libgit2-sys` 和 `libz-sys` 的 vendored 源码）

### 编译

```bash
cargo build --release
```

编译产物位于 `target/release/biturbo.dll`（Windows）或 `target/release/libbiturbo.so`（Linux/macOS）。

## 用法

### 作为 libgit2.dll 的替代

将编译好的 `biturbo.dll` 放置到 Git for Windows 的 `bin/` 目录下（替换原 `libgit2.dll`），即可透明升级。Biturbo 导出所有 zlib 符号以及额外的 `bt_*` 函数，保留原始 ABI 兼容性。

### 导出的符号

所有导出符号定义在 [`biturbo.def`](./biturbo.def) 中，分为两类：

| 类别 | 符号 | 说明 |
|------|------|------|
| **zlib 兼容** | `compress`, `deflate`, `inflate`, `adler32`, `crc32`, `zlibVersion` 等 | 完全兼容 zlib 1.x ABI |
| **Git 操作** | `bt_get_commits`, `bt_get_references`, `bt_get_tree`, `bt_get_head`, `bt_get_tag_details` 等 | 高效的 Git 仓库元数据查询 |
| **工具函数** | `bt_md_to_html`, `bt_highlight_syntax`, `bt_layout_treemap`, `bt_decode_image` | 额外的数据处理能力 |
| **进程管理** | `bt_spawn_with_output`, `bt_spawn_with_callback`, `bt_kill_process_cancellation_token` | 子进程管理 |
| **内存管理** | `bt_release_*` 系列 | 释放由 Biturbo 分配的内存 |

## 项目结构

```
src/
├── lib.rs              # 库入口，声明 ffi 模块
├── bin/
│   └── perf_compare.rs # 性能对比测试工具
└── ffi/
    ├── mod.rs           # 模块声明
    ├── types.rs         # 公用类型定义
    ├── bt_error.rs      # 线程安全错误缓冲区
    ├── bt_commits.rs    # 提交图遍历
    ├── bt_committer_times.rs
    ├── bt_references.rs # 引用（分支/标签）枚举
    ├── bt_get_tree.rs   # 树对象递归读取
    ├── bt_head.rs       # HEAD 查询
    ├── bt_tags.rs       # Tag 详情
    ├── bt_stashes.rs    # Stash 管理
    ├── bt_revision_headers.rs
    ├── bt_repository_manager.rs # 仓库管理器（保存/加载）
    ├── bt_git_config.rs
    ├── bt_parse_patch.rs
    ├── bt_markdown.rs   # Markdown → HTML
    ├── bt_highlight_syntax.rs
    ├── bt_layout_treemap.rs
    ├── bt_decode_image.rs
    ├── bt_process.rs    # 子进程 spawn
    ├── bt_cancellation.rs
    ├── bt_oid.rs        # OID 字符串解析
    ├── bt_release_vec.rs
    ├── winheap.rs       # Windows 堆内存分配
    ├── zlib_touch.rs
    └── error.rs         # 错误类型
tests/
├── abi_compare.rs
├── commit_graph_smoke.rs
├── git_repo_compare.rs
└── zlib_more_compare.rs
```

## 测试

```bash
cargo test
```

## 许可证

本项目采用 MIT 许可证
