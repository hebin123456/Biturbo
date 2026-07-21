# Release Notes

## v1.1.1

本次发布扩展构建矩阵，新增 Linux ARM64 与 macOS ARM64 两个原生平台产物。

### 新增

- **Linux ARM64 构建**（`aarch64-unknown-linux-gnu`）
  - CI 使用 GitHub 官方 `ubuntu-24.04-arm` standard runner（native ARM64，无需 QEMU 仿真）
  - 产物 `libbiturbo.so` 归档为 `biturbo-linux-arm64-1.1.1.zip`
- **macOS ARM64 构建**（`aarch64-apple-darwin`）
  - CI 使用 `macos-14`（Apple Silicon M1）standard runner
  - 产物 `libbiturbo.dylib` 归档为 `biturbo-macos-arm64-1.1.1.zip`
  - 仅构建 ARM64（Apple Silicon，2020 年后 Mac 主流架构），不再构建 x64

### 修复

- **Linux ARM64 链接失败**：`biturbo.exports.map` 此前使用了带版本标签的
  `BITURBO_1.1.0 { ... };` 格式，与 rustc 自动生成的 anonymous 版本脚本同时
  传给 `ld` 时会触发 `anonymous version tag cannot be combined with other
  version tags` 错误（aarch64 binutils 较严格，x86_64 较宽松所以未暴露）。
  改为 anonymous 版本脚本（直接 `{ global: ... local: *; };`，无版本标签）。

### 平台覆盖

| 平台 | target | runner | 产物 |
|------|--------|--------|------|
| Windows x64 | `x86_64-pc-windows-msvc` | `windows-latest` | `biturbo.dll` |
| Linux x64 | `x86_64-unknown-linux-gnu` | `ubuntu-latest` | `libbiturbo.so` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` | `libbiturbo.so` |
| macOS ARM64 | `aarch64-apple-darwin` | `macos-14` | `libbiturbo.dylib` |

CI matrix `fail-fast: false`，单平台失败不影响其他平台构建。

## v1.0.2

本次发布围绕"完善 FFI 内存容量语义与行为一致性"展开，重点修复了提交图遍历、引用哈希、语法高亮及配置序列化等问题。

### 修复与改进

- **提交图遍历（`bt_commits.rs`）**
  - 多 tip 提交图遍历从手写的 `BinaryHeap` 拓扑排序改为直接使用 `git2` 的 `revwalk` + `TOPOLOGICAL` 排序，保证遍历顺序的确定性。
  - `bt_get_commit_subgraph` / `bt_get_commit_subgraph_2` 重写为统一的 `get_commit_subgraph_date_order`，采用 `revwalk` + `TIME`（日期序）遍历，并支持 `stop_after` 提前终止。
  - 新增 `alloc_and_copy_slice_with_cap` 与 `next_legacy_capacity`，输出缓冲区的 `oids_cap` / `indexes_cap` 现按 `next_power_of_two` 计算，采用 2 的幂次容量语义。
  - `bt_search_commits` 现支持按 commit 的 SHA-1 十六进制串进行匹配，原先仅匹配提交信息。
  - `bt_get_commits` 在传入 `required_oids` 时不再限制 `max_count`，确保指定提交必被包含。
  - `bt_release_commit_storage` 释放后不再清零调用方字段，避免悬空访问。

- **引用列表（`bt_references.rs`）**
  - 引用哈希算法从 FNV-1a 改为 `DefaultHasher`（SipHash），实现确定性的引用哈希：遍历时过滤 `FETCH_HEAD` / `MERGE_HEAD`，tag 引用自动 peel 到目标对象，symref 用 `0`/`1` 标记。
  - `assign_bytes` / `assign_vector` 现保留 `Vec` 的真实 `capacity`，使输出 `BtBuf.cap` 反映真实容量。
  - `bt_release_references` 释放后不再清零字段。

- **仓库管理配置（`bt_repository_manager.rs`）**
  - `color` 字段支持字符串名称（`Red` / `Orange` / `Yellow` / `Green` / `Blue` / `Violet`）与整数互转，序列化时输出可读名称。
  - `scan_depth` 默认值改为 `5`。
  - 兼容旧版 TOML 中 `repository`（单数）字段名，读取时自动回退。
  - 通过 `skip_serializing_if` 清理默认值输出，使配置文件更简洁。

- **语法高亮（`bt_highlight_syntax.rs`）**
  - 重写语法样式判定逻辑，按语言（C# / Rust / JS-TS）分别返回不同样式：关键字（2）、类型（3）、修饰符（5）、字面量（7），替代原先统一的 keyword/type 判断，实现按语言区分的着色规则。

- **进程输出（`bt_process.rs`）**
  - `bt_spawn_with_output` 的 stdout / stderr 输出缓冲区改用 `next_power_of_two`（最小 16）的容量分配，采用 2 的幂次容量语义；并补齐 stderr 分配失败时回滚 stdout 的内存释放。

- **释放函数统一（`bt_stashes.rs` / `bt_tags.rs`）**
  - 多个 `bt_release_*` 函数不再使用 `ptr::replace` 清零调用方字段，与 v1.0.1 的 `bt_release_vec` 修复保持一致，规避释放后被改写导致的悬空访问。

- **图像解码（`bt_decode_image.rs`）**
  - TGA 解码失败的错误信息改为 `"failed to fill whole buffer"`，统一错误信息格式。

### 测试与文档自动化

- **单元测试覆盖**：新增对核心纯函数的单元测试，从原先仅 `bt_layout_treemap` 一个模块扩展到 9 个模块，覆盖：
  - `BtOid` 字节序/往返（`types.rs`）
  - `hex_nibble` 十六进制解析边界（`bt_oid.rs`）
  - `next_legacy_capacity` / `btoid_to_hex` 容量与格式（`bt_commits.rs`）
  - `legacy_vec_capacity` 进位语义（`bt_process.rs`）
  - `color_to_name` / `color_from_name` 互转与 TOML 序列化往返、旧字段兼容（`bt_repository_manager.rs`）
  - `syntax_style` 多语言样式判定（`bt_highlight_syntax.rs`）
  - `decode_tga_to_bmp` 各类 TGA 编码与异常输入（`bt_decode_image.rs`）
  - `tokenize_chunk_header` / `add_diff_header_path_tokens` diff 分词（`bt_parse_patch.rs`）
  - 所有新增测试为纯函数测试，不依赖外部动态库，随 `cargo test --lib` 在 Build Windows 流水线运行。
- **API 文档自动化**：新增 [`docs.yml`](./.github/workflows/docs.yml) 工作流，推送 `master` 时在 Windows runner 构建 `cargo doc` 并部署到 GitHub Pages，提供在线 API 文档。
- **CI 工作流对齐 ForkPlus**：将原 `release.yml` 重命名为 [`build-windows.yml`](./.github/workflows/build-windows.yml)，统一 `name: Build Windows`，新增 `workflow_dispatch` 手动触发与 `upload-artifact` 步骤；README 头部新增 Build 徽章，便于一眼看到 Windows 构建状态。

## v1.0.1

首个公开发布版本的基础修复，主要解决 FFI 边界的崩溃与内存安全问题。

### 修复

- **Treemap 布局崩溃（`bt_layout_treemap.rs`）**
  - 用 `catch_unwind` 包裹 FFI 入口，防止内部 panic 中止宿主进程。
  - 重写为 `layout_legacy_recursive`，实现标准的 squarify 算法（`total_without_last` / `first_ratio` / `legacy_aspect`），修正了行沿错误边铺开以及节点面积随 `remaining` 缩放导致留缝的两个缺陷。

- **进程回调按行读取（`bt_process.rs`）**
  - `bt_spawn_with_callback` 的 stdout / stderr 读取线程从 4KB 定长 chunk 改为 `read_until(b'\n')` 按行回调，与 `ReadLineCallback` 的语义一致。

- **释放后改写字段（`bt_release_vec.rs`）**
  - 释放 `BtBuf` 后不再改写调用方的 `ptr` / `len` / `cap` 字段，避免在 use-after-free 场景下产生悬空访问。
