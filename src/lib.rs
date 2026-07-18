//! # Biturbo
//!
//! Rust 实现的 Git for Windows `libgit2.dll` 兼容替代品，并在保持 zlib / libgit2
//! ABI 完全兼容的基础上，额外提供一套面向上层应用的 `bt_*` C ABI 接口。
//!
//! 本 crate 编译为 `cdylib`，导出符号由 [`biturbo.def`](https://github.com/hebin123456/Biturbo/blob/master/biturbo.def)
//! 声明（共 93 个），实现拆分在 [`ffi`] 模块下的各子模块中。
//!
//! ## 特性
//!
//! - **zlib / libgit2 兼容** — 导出与原版 `libgit2.dll` 相同的 zlib 符号
//!   （`compress`、`deflate`、`inflate`、`adler32`、`crc32`、`zlibVersion` 等），
//!   可作为 `libgit2.dll` 的透明替代。
//! - **高性能 Git 操作** — `bt_*` 系列 C ABI 函数覆盖提交查询、引用枚举、树对象读取、
//!   Tag 详情、Stash 管理、Revision 头信息、提交者统计等。
//! - **附加能力** — 语法高亮、Markdown → HTML 渲染、矩形 Treemap 布局、图像解码。
//! - **子进程管理** — `bt_spawn_with_output` / `bt_spawn_with_callback`，配合
//!   取消令牌支持可控的子进程派生与管道通信。
//! - **线程安全** — 错误信息通过线程本地缓冲区保存，每个线程独立读取
//!   （`bt_get_last_error_message`）。
//!
//! ## 平台支持
//!
//! 当前仅支持 **Windows x64（MSVC）**：
//! - [`ffi::winheap`] 通过 `kernel32` 的 `HeapAlloc` / `HeapFree` 管理内存，
//!   作为整个 FFI 内存模型的基础。
//! - `build.rs` 使用 MSVC 的 `/DEF:` 链接参数导出 `biturbo.def` 声明的符号。
//!
//! ## FFI 内存管理约定
//!
//! 由 Biturbo 分配并通过 out 参数返回的缓冲区（如 `BtCommitsResult`、
//! `BtLayoutTreemapResult` 等），**必须**使用对应的 `bt_release_*` 函数释放。
//! 混用 C 侧的 `free` / `delete` 会导致堆损坏，因为 Biturbo 使用 Windows 进程堆分配。
//!
//! ## 模块组织
//!
//! 实现按职责拆分在 [`ffi`] 下：提交图遍历、引用枚举、树对象读取、Tag 详情、
//! Stash 管理、Revision 头信息、仓库管理器、Git 配置、Patch 解析、Markdown 渲染、
//! 语法高亮、Treemap 布局、图像解码、子进程管理、取消令牌、OID 解析、
//! 通用 Vec 释放、Windows 堆内存、zlib 符号导出等。

#![allow(non_snake_case)]

pub mod ffi;

