//! # Biturbo
//!
//! Rust 编写的 `cdylib`，内嵌 vendored `libgit2` + `zlib`，在其之上提供一套面向
//! 上层应用的 `bt_*` C ABI 接口。
//!
//! 本 crate 编译为 `cdylib`，导出符号由 [`biturbo.def`](https://github.com/hebin123456/Biturbo/blob/master/biturbo.def)
//! 声明（共 93 个），实现拆分在 [`ffi`] 模块下的各子模块中。
//!
//! > **注意**：Biturbo **不**导出 libgit2 的 `git_*` C API，因此**不是**
//! > `libgit2.dll` 的兼容替代品；它只导出 zlib 符号与自身的 `bt_*` 接口。
//!
//! ## 特性
//!
//! - **内嵌 vendored libgit2 / zlib** — 通过 `git2` crate（vendored-libgit2）静态
//!   链接 libgit2 与 zlib，单一动态库自包含，无需额外运行时依赖。
//! - **zlib 符号导出** — 导出与 zlib 1.x ABI 兼容的符号
//!   （`compress`、`deflate`、`inflate`、`adler32`、`crc32`、`zlibVersion` 等）。
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
//! 支持 **Windows / Linux / macOS 三平台 x64**：
//! - [`ffi::winheap`] 在 Windows 上通过 `kernel32` 的 `HeapAlloc` / `HeapFree`
//!   管理内存，在 Linux/macOS 上通过 libc `malloc` / `free`。
//! - `build.rs` 按 `CARGO_CFG_TARGET_OS` 分叉：Windows 用 MSVC `/DEF:`，
//!   Linux 用 ld `--version-script`，macOS 用 `-exported_symbols_list`，
//!   三者都以 `biturbo.def` 作为符号真源。
//!
//! ## FFI 内存管理约定
//!
//! 由 Biturbo 分配并通过 out 参数返回的缓冲区（如 `BtCommitsResult`、
//! `BtLayoutTreemapResult` 等），**必须**使用对应的 `bt_release_*` 函数释放。
//! 混用 C 侧的 `free` / `delete` 会导致堆损坏——Windows 上 Biturbo 用进程堆，
//! Linux/macOS 上用 libc `malloc`，调用方都应通过 `bt_release_*` 释放。
//!
//! ## 模块组织
//!
//! 实现按职责拆分在 [`ffi`] 下：提交图遍历、引用枚举、树对象读取、Tag 详情、
//! Stash 管理、Revision 头信息、仓库管理器、Git 配置、Patch 解析、Markdown 渲染、
//! 语法高亮、Treemap 布局、图像解码、子进程管理、取消令牌、OID 解析、
//! 通用 Vec 释放、跨平台 FFI 堆分配、zlib 符号导出等。

#![allow(non_snake_case)]

pub mod ffi;

