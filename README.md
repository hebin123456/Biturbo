# Biturbo

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)
[![Build](https://github.com/hebin123456/Biturbo/actions/workflows/build-windows.yml/badge.svg)](https://github.com/hebin123456/Biturbo/actions/workflows/build-windows.yml)
[![Release](https://img.shields.io/github/v/release/hebin123456/Biturbo)](https://github.com/hebin123456/Biturbo/releases)

> A Rust-based `cdylib` that bundles vendored `libgit2` + `zlib` and exposes a
> high-performance `bt_*` C ABI on top — Windows / Linux (x64/ARM64) + macOS (ARM64).

Biturbo 是一个用 Rust 编写的 `cdylib`，内嵌 vendored `libgit2` + `zlib`，在其之上
提供了一套面向上层应用的 `bt_*` C ABI 接口，涵盖提交图遍历、引用枚举、树对象读取、
Tag 详情、Stash 管理、差异解析、语法高亮、Markdown 渲染、矩形 Treemap 布局、子进程
管理等能力。

> **注意**：Biturbo **不**导出 libgit2 的 `git_*` C API，因此**不是** `libgit2.dll`
> 的兼容替代品；它只导出 zlib 符号与自身的 `bt_*` 接口。

## 特性

- **内嵌 vendored libgit2 / zlib** — 通过 `git2` crate（vendored-libgit2）静态链接
  libgit2 与 zlib，单一动态库自包含，无需额外运行时依赖。
- **zlib 符号导出** — 导出与 zlib 1.x ABI 兼容的符号
  （`compress`、`deflate`、`inflate`、`adler32`、`crc32`、`zlibVersion` 等），
  可作为 zlib ABI 兼容的动态库使用。
- **高性能 Git 操作** — `bt_*` 系列 C ABI 函数覆盖提交查询、引用枚举、树对象读取、
  Tag 详情、Stash 管理、Revision 头信息、提交者统计等。
- **附加能力** — 语法高亮、Markdown → HTML 渲染（表格、围栏代码块、任务列表）、
  矩形 Treemap 布局、图像解码。
- **子进程管理** — `bt_spawn_with_output` / `bt_spawn_with_callback`，配合
  取消令牌（cancellation token）支持可控的子进程派生与管道通信。
- **线程安全** — 错误信息通过线程本地缓冲区保存，每个线程独立读取
  （`bt_get_last_error_message`）。
- **FFI 内存模型** — 由 Biturbo 分配的内存必须由 Biturbo 对应的 `bt_release_*`
  函数释放；分配器按平台分叉（见下），与 C 侧 `free` 不互通。

## 平台支持

支持 **Windows / Linux (x64/ARM64) + macOS (ARM64/x64) 五平台**：

| 平台 | target | 产物 | FFI 内存分配器 | 符号导出机制 |
|------|--------|------|----------------|--------------|
| Windows x64 | `x86_64-pc-windows-msvc` | `biturbo.dll` | kernel32 `HeapAlloc` / `HeapFree` | MSVC `/DEF:` + 序号 |
| Linux x64   | `x86_64-unknown-linux-gnu` | `libbiturbo.so` | libc `malloc` / `free` | ld `--version-script` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | `libbiturbo.so`（release 资产 `libbiturbo-arm64.so`） | libc `malloc` / `free` | ld `--version-script` |
| macOS ARM64 | `aarch64-apple-darwin` | `libbiturbo.dylib` | libc `malloc` / `free` | `-exported_symbols_list` |
| macOS x64   | `x86_64-apple-darwin` | `libbiturbo.dylib`（release 资产 `libbiturbo-x64.dylib`） | libc `malloc` / `free` | `-exported_symbols_list` |

> macOS x64 在 GitHub Apple Silicon runner（`macos-14`）上跨 arch 构建（Apple
> 工具链原生支持 `-arch x86_64`，无需 Intel runner），vendored libgit2 的 cmake
> 腿需显式指定 `CMAKE_OSX_ARCHITECTURES=x86_64`。
> Linux 同时构建 x64 与 ARM64：ARM64 在 GitHub 原生 ARM64 runner（`ubuntu-22.04-arm`，
> 公开仓库免费）上原生构建，不做交叉编译。两条 Linux 腿均显式使用工具链自带的
> rust-lld（见下方"符号导出"与构建说明），规避 GNU ld 的双版本脚本冲突。

- [src/ffi/winheap.rs](./src/ffi/winheap.rs) 在 Windows 上使用 `kernel32` 的进程堆，
  在 Linux/macOS 上使用 libc `malloc`/`free`；调用方对内部使用的堆无感知，只需保证
  分配/释放都走 Biturbo 的 `bt_*` 接口。
- [build.rs](./build.rs) 按 `CARGO_CFG_TARGET_OS` 分叉：Windows 走 MSVC `/DEF:`，
  Linux 用 ld version script，macOS 用 `-exported_symbols_list`，三者都以
  [biturbo.def](./biturbo.def) 作为符号真源。
- `biturbo.def` 中的 `LIBRARY` 行与 `@ORDINAL` 序号仅 Windows 链接器使用；
  Linux/macOS 忽略这些字段，只读取符号名。
- Linux `biturbo.exports.map` 采用 anonymous 版本脚本（`{ global: ... local: *; };`，
  无版本标签）。注意 rustc 链接 cdylib 时也会注入自己的 anonymous 版本脚本：
  GNU ld 不允许两个 anonymous 脚本共存（`anonymous version tag cannot be combined
  with other version tags`），lld 则按命令行合并两份脚本，且精确符号匹配优先于
  `local: *` 通配，`biturbo.exports.map` 列出的符号仍全部导出。因此 **Linux 构建必须
  用 lld**：x86_64 上 rustc 默认即工具链自带 rust-lld；aarch64（无论原生还是交叉）
  默认走 GNU ld，需要按构建说明显式切换到 rust-lld。

## 下载

预编译产物可从 [Releases](https://github.com/hebin123456/Biturbo/releases) 页获取。
每个 Release 附带五平台产物：

- `biturbo-windows-x64-<version>.zip`   — 含 `biturbo.dll`、`biturbo.def`、`biturbo.dll.lib`
- `biturbo-linux-x64-<version>.zip`     — 含 `libbiturbo.so`、`biturbo.def`
- `biturbo-linux-arm64-<version>.zip`   — 含 `libbiturbo-arm64.so`、`biturbo.def`
- `biturbo-macos-arm64-<version>.zip`   — 含 `libbiturbo.dylib`、`biturbo.def`
- `biturbo-macos-x64-<version>.zip`     — 含 `libbiturbo-x64.dylib`、`biturbo.def`
- 单独的动态库 / `.def` 文件

> Linux ARM64 的 Release 裸资产名为 `libbiturbo-arm64.so`（与 x64 的 `libbiturbo.so`
> 区分以避免同名冲突），使用时重命名为 `libbiturbo.so` 即可（运行时按库名
> `libbiturbo.so` 加载）。macOS x64 同理：裸资产名 `libbiturbo-x64.dylib`，
> 使用时重命名为 `libbiturbo.dylib`。

## 构建

### 前置要求

通用：
- [Rust toolchain](https://rustup.rs/)（stable）
- CMake + C 编译器（用于 vendored 构建 `libgit2-sys` 与 `libz-sys`）

按平台：

| 平台 | target | 额外依赖 |
|------|--------|----------|
| Windows x64 | `x86_64-pc-windows-msvc` | Visual Studio Build Tools（MSVC + Windows SDK），提供 `link.exe` 和 `dumpbin` |
| Linux x64   | `x86_64-unknown-linux-gnu` | `pkg-config`、`ninja-build`（可选，加速 vendored 构建） |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | 原生 ARM64 机器（或自行配置交叉工具链）；`pkg-config`、`ninja-build`（可选）；链接器需走 rust-lld（见下方说明） |
| macOS ARM64 | `aarch64-apple-darwin` | Xcode Command Line Tools；`brew install cmake ninja` |

### 编译

```bash
# Windows x64
cargo build --release --target x86_64-pc-windows-msvc

# Linux x64
cargo build --release --target x86_64-unknown-linux-gnu

# Linux ARM64（在原生 ARM64 机器上构建）
cargo build --release --target aarch64-unknown-linux-gnu

# macOS ARM64
cargo build --release --target aarch64-apple-darwin
```

产物位于 `target/<target>/release/`：`biturbo.dll` / `libbiturbo.so` / `libbiturbo.dylib`。

> **Linux/macOS 注意**：仓库根目录的 [.cargo/config.toml](./.cargo/config.toml) 通过
> `[env]` 设置 `CFLAGS=-fvisibility=default`，用于覆盖 libz-sys 在 zlib 编译时
> 传入的 `-fvisibility=hidden`（否则所有 zlib 符号都会变成 local，无法被
> cdylib 重导出）。如果你在 CI 之外构建并需要自定义 `CFLAGS`，请确保
> 同时带上 `-fvisibility=default`。

> **Linux 链接器注意**：本地在 **Linux ARM64** 上构建时，rustc 默认走 GNU ld，
> 会与 `biturbo.exports.map` 触发双版本脚本冲突；需显式切换到工具链自带的
> rust-lld（x86_64 上 rustc 默认即 rust-lld，无需处理；CI 已自动配置）：
>
> ```bash
> LD_DIR="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/gcc-ld"
> RUSTFLAGS="-C link-arg=-B$LD_DIR -C link-arg=-fuse-ld=lld" \
>   cargo build --release --target aarch64-unknown-linux-gnu
> ```
>
> 交叉编译到 aarch64 时把路径中的 host 换成 `aarch64-unknown-linux-gnu` 即可
> （`gcc-ld` 目录按链接的 target 而非 host 定位）。

### 单元测试

```bash
cargo test --release --target x86_64-pc-windows-msvc
cargo test --release --target x86_64-unknown-linux-gnu
cargo test --release --target aarch64-unknown-linux-gnu
cargo test --release --target aarch64-apple-darwin
```

## 用法

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
C 侧的 `free` / `delete` 会导致堆损坏——Windows 上 Biturbo 用进程堆，Linux/macOS
上用 libc `malloc`，调用方都应通过 `bt_release_*` 释放。

## CI / Release

本仓库有三条 GitHub Actions 流水线（[`.github/workflows/`](./.github/workflows/)）：

| 流水线 | 触发条件 | 作用 |
|--------|----------|------|
| **Build** | 推送 `v*` tag 或手动触发 | 单条 matrix workflow 同时在 windows / ubuntu(x64+ARM64) / macos(ARM64+x64 跨 arch) runner 上构建、跑测试、校验导出符号、上传 artifact、打 tag 时打包并发布五平台 GitHub Release |
| **Bump & Tag** | 手动触发 | 自动 bump 版本（patch/minor/major/prerelease）、改 `Cargo.toml`、打 tag（间接触发 Build） |
| **Docs** | 推送 `master` 或手动触发 | 在 Linux runner 上跑 `cargo doc` 并部署到 GitHub Pages |

> 推送到 `master` 分支只触发 Docs 流水线（部署 API 文档），**不会**触发构建。发版统一
> 通过打 `v*` tag 进行；一次 tag 同时触发四个平台的并行构建，各平台独立产物。

### 发新版本

GitHub 仓库 → Actions → "Bump & Tag" → Run workflow → 选择版本类型
（patch / minor / major / prerelease）。流水线会自动完成版本号修改、提交、打 tag，
并触发 Build 流水线在三平台并行发布新版本。

## 项目结构

```
.
├── Cargo.toml                      # 包含 libz-sys static 特性与平台分叉依赖
├── .cargo/config.toml              # 设置 CFLAGS=-fvisibility=default（覆盖 libz-sys 的 hidden）
├── build.rs                        # 按 target_os 分叉链接器参数：/DEF: / --version-script / -exported_symbols_list
├── biturbo.def                     # 跨平台符号清单（Windows 走 /DEF:，Linux/macOS 作真源）
├── biturbo.exports.map             # Linux ld 版本脚本（导出 93 个符号，其余全部 local）
├── biturbo.exports.list            # macOS -exported_symbols_list 输入
└── src/
    ├── lib.rs                      # 库入口，声明 ffi 模块
    └── ffi/
        ├── mod.rs                  # 模块声明
        ├── types.rs                # 公用 C ABI 类型定义
        ├── error.rs / bt_error.rs  # 错误类型 + 线程安全错误缓冲区
        ├── bt_commits.rs           # 提交图遍历
        ├── bt_committer_times.rs   # 提交者统计
        ├── bt_references.rs        # 引用（分支/标签）枚举
        ├── bt_get_tree.rs          # 树对象递归读取
        ├── bt_head.rs              # HEAD 查询
        ├── bt_tags.rs              # Tag 详情
        ├── bt_stashes.rs           # Stash 管理
        ├── bt_revision_headers.rs  # Revision 头信息
        ├── bt_repository_manager.rs# 仓库管理器（保存/加载）
        ├── bt_git_config.rs        # Git 配置读取
        ├── bt_parse_patch.rs       # Patch/diff 解析
        ├── bt_markdown.rs          # Markdown → HTML
        ├── bt_highlight_syntax.rs # 语法高亮
        ├── bt_layout_treemap.rs    # 矩形 Treemap 布局
        ├── bt_decode_image.rs      # 图像解码
        ├── bt_process.rs           # 子进程 spawn
        ├── bt_cancellation.rs      # 取消令牌
        ├── bt_oid.rs               # OID 字符串解析
        ├── bt_release_vec.rs       # 通用 Vec 释放
        ├── winheap.rs              # 跨平台 FFI 内存分配器（Win kernel32 / Unix libc）
        └── zlib_touch.rs           # zlib 符号链接 anchor（防止静态库对象被链接器丢弃）
```

## 许可证

[MIT](./LICENSE)
