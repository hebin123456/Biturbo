# Biturbo

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)
[![Release](https://img.shields.io/github/v/release/hebin123456/Biturbo)](https://github.com/hebin123456/Biturbo/releases)

> A Rust-based drop-in replacement for Git for Windows' `libgit2.dll`, with an
> extended high-performance Git repository API on top.

Biturbo 是一个用 Rust 编写的 `cdylib`，作为 Git for Windows 中 `libgit2.dll`
的兼容替代品。它在保持与原版 zlib / libgit2 ABI 完全兼容的同时，额外提供了一套
面向上层应用的 `bt_*` C ABI 接口，涵盖提交图遍历、引用枚举、树对象读取、Tag 详情、
Stash 管理、差异解析、语法高亮、Markdown 渲染、矩形 Treemap 布局、子进程管理等能力。

## 特性

- **zlib / libgit2 兼容** — 导出与原版 `libgit2.dll` 相同的 zlib 符号
  （`compress`、`deflate`、`inflate`、`adler32`、`crc32`、`zlibVersion` 等），
  可作为 `libgit2.dll` 的透明替代。
- **高性能 Git 操作** — `bt_*` 系列 C ABI 函数覆盖提交查询、引用枚举、树对象读取、
  Tag 详情、Stash 管理、Revision 头信息、提交者统计等。
- **附加能力** — 语法高亮、Markdown → HTML 渲染（表格、围栏代码块、任务列表）、
  矩形 Treemap 布局、图像解码。
- **子进程管理** — `bt_spawn_with_output` / `bt_spawn_with_callback`，配合
  取消令牌（cancellation token）支持可控的子进程派生与管道通信。
- **线程安全** — 错误信息通过线程本地缓冲区保存，每个线程独立读取
  （`bt_get_last_error_message`）。
- **FFI 内存模型** — 由 Biturbo 分配的内存必须由 Biturbo 对应的 `bt_release_*`
  函数释放；分配使用 Windows 进程堆，与 C 侧 `free` 不互通。

## 平台支持

当前**仅支持 Windows x64（MSVC）**：

- [build.rs](./build.rs) 使用 MSVC 的 `/DEF:` 链接参数导出 `biturbo.def` 声明的符号。
- [src/ffi/winheap.rs](./src/ffi/winheap.rs) 通过 `kernel32` 的 `HeapAlloc` / `HeapFree`
  管理内存，作为整个 FFI 内存模型的基础。

Linux / macOS 暂不支持（需要重写内存分配层并替换 Windows 进程 API，会破坏现有
FFI 内存契约）。

## 下载

预编译的 `biturbo.dll` 可从 [Releases](https://github.com/hebin123456/Biturbo/releases)
页获取。每个 Release 附带：

- `biturbo-windows-x64-<version>.zip` — 含 `biturbo.dll`、`biturbo.def`、`biturbo.dll.lib`
- `biturbo.dll` — 单独的 DLL
- `biturbo.def` — 导出符号定义

## 构建

### 前置要求

- [Rust toolchain](https://rustup.rs/)（stable），target `x86_64-pc-windows-msvc`
- Visual Studio Build Tools（MSVC + Windows SDK），提供 `link.exe` 和 `dumpbin`
- CMake 与 C 编译器（用于 vendored 构建 `libgit2-sys` 和 `libz-sys`）

### 编译

```bash
cargo build --release --target x86_64-pc-windows-msvc
```

产物位于 `target/x86_64-pc-windows-msvc/release/biturbo.dll`。

### 单元测试

```bash
cargo test --lib --release --target x86_64-pc-windows-msvc
```

> `tests/` 目录下的集成测试依赖原版 `biturbo.dll`（不在本仓库中），CI 默认只跑
> `--lib` 单元测试。

## 用法

### 作为 libgit2.dll 的替代

将编译好的 `biturbo.dll` 重命名为 `libgit2.dll`（或直接放置）到 Git for Windows 的
`bin/` 目录下即可透明替换。Biturbo 导出全部 zlib 符号，保留原始 ABI 兼容性。

### 导出符号

所有导出符号定义在 [`biturbo.def`](./biturbo.def) 中（共 93 个），分为：

| 类别 | 代表符号 | 说明 |
|------|----------|------|
| **zlib 兼容** | `compress`, `deflate`, `inflate`, `adler32`, `crc32`, `zlibVersion` | 完全兼容 zlib 1.x ABI |
| **Git 操作** | `bt_get_commits`, `bt_get_references`, `bt_get_tree`, `bt_get_head`, `bt_get_tag_details`, `bt_get_stashes` | Git 仓库元数据查询 |
| **工具函数** | `bt_md_to_html`, `bt_highlight_syntax`, `bt_layout_treemap`, `bt_decode_image` | 文本/图像数据处理 |
| **进程管理** | `bt_spawn_with_output`, `bt_spawn_with_callback`, `bt_kill_process_cancellation_token` | 子进程管理 |
| **内存管理** | `bt_release_*` 系列 | 释放由 Biturbo 分配的内存 |

### 内存管理约定

由 Biturbo 分配并通过 out 参数返回的缓冲区（如 `BtCommitsResult`、
`BtLayoutTreemapResult` 等），**必须**使用对应的 `bt_release_*` 函数释放。混用
C 侧的 `free` / `delete` 会导致堆损坏，因为 Biturbo 使用 Windows 进程堆分配。

## CI / Release

本仓库有两条 GitHub Actions 流水线（[`.github/workflows/`](./.github/workflows/)）：

| 流水线 | 触发条件 | 作用 |
|--------|----------|------|
| **Release** | 推送 `v*` tag | 构建 release DLL、跑单元测试、校验 PE 头、校验导出符号、打包并发布 GitHub Release（DLL 只构建一次） |
| **Bump & Tag** | 手动触发 | 自动 bump 版本（patch/minor/major）、改 `Cargo.toml`、打 tag（间接触发 Release） |

> 推送到 `master` 分支**不会**触发任何流水线。发版统一通过打 `v*` tag 进行；一次 tag 只触发一条流水线，DLL 只构建一次。

### 发新版本

GitHub 仓库 → Actions → "Bump & Tag" → Run workflow → 选择版本类型
（patch / minor / major）。流水线会自动完成版本号修改、提交、打 tag，并触发
Release 流水线发布新版本。

## 项目结构

```
src/
├── lib.rs                          # 库入口，声明 ffi 模块
├── bin/
│   └── perf_compare.rs             # 性能对比测试工具
└── ffi/
    ├── mod.rs                      # 模块声明
    ├── types.rs                    # 公用 C ABI 类型定义
    ├── error.rs / bt_error.rs      # 错误类型 + 线程安全错误缓冲区
    ├── bt_commits.rs               # 提交图遍历
    ├── bt_committer_times.rs       # 提交者统计
    ├── bt_references.rs            # 引用（分支/标签）枚举
    ├── bt_get_tree.rs              # 树对象递归读取
    ├── bt_head.rs                  # HEAD 查询
    ├── bt_tags.rs                  # Tag 详情
    ├── bt_stashes.rs               # Stash 管理
    ├── bt_revision_headers.rs      # Revision 头信息
    ├── bt_repository_manager.rs    # 仓库管理器（保存/加载）
    ├── bt_git_config.rs            # Git 配置读取
    ├── bt_parse_patch.rs           # Patch/diff 解析
    ├── bt_markdown.rs              # Markdown → HTML
    ├── bt_highlight_syntax.rs      # 语法高亮
    ├── bt_layout_treemap.rs        # 矩形 Treemap 布局
    ├── bt_decode_image.rs          # 图像解码
    ├── bt_process.rs               # 子进程 spawn
    ├── bt_cancellation.rs          # 取消令牌
    ├── bt_oid.rs                   # OID 字符串解析
    ├── bt_release_vec.rs           # 通用 Vec 释放
    ├── winheap.rs                  # Windows 堆内存分配
    └── zlib_touch.rs               # zlib 符号导出
tests/
├── abi_compare.rs
├── commit_graph_smoke.rs
├── git_repo_compare.rs
└── zlib_more_compare.rs
```

## 许可证

[MIT](./LICENSE)
