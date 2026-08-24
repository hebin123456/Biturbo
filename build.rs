use std::{env, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let def_path = manifest_dir.join("biturbo.def");
    let exports_map = manifest_dir.join("biturbo.exports.map");

    println!("cargo:rerun-if-changed={}", def_path.display());
    println!("cargo:rerun-if-changed={}", exports_map.display());

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_os == "windows" {
        // Windows：通过 MSVC 的 /DEF: 链接参数导出 .def 声明的符号（带序号）。
        // /DEF: 会强制导出列表中的所有符号，即使本 crate 代码没有引用它们。
        println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def_path.display());
    } else if target_os == "linux" {
        // Linux：本脚本把 biturbo.def 声明的符号（bt_* + zlib ABI）精确列入
        // 版本脚本 global 段，其余一律 local 隐藏（libgit2 的 git_* 不外泄）。
        // `src/ffi/zlib_touch.rs` 中的 `#[used] static ZLIB_ANCHORS` 负责把
        // zlib 静态库对象拉入链接输入，脚本再把这些符号导出。
        //
        // 链接器要求：必须使用 LLVM lld（工具链自带的 rust-lld）。rustc 链接
        // cdylib 时会注入自己的 anonymous 版本脚本（列出 #[no_mangle] 符号 +
        // local: *），GNU ld 不允许两个 anonymous 脚本共存（anonymous version
        // tag cannot be combined with other version tags）；lld 按命令行合并
        // 两份脚本，且精确符号匹配优先于 `*` 通配，因此本脚本列出的符号全部
        // 如期导出。x86_64 上 rustc 默认即 rust-lld；aarch64 需在 CI 中显式
        // 指定（见 .github/workflows/build-windows.yml 的 "Use bundled
        // rust-lld" 步骤）。
        println!(
            "cargo:rustc-cdylib-link-arg=-Wl,--version-script={}",
            exports_map.display()
        );
    } else if target_os == "macos" {
        // macOS：使用 -exported_symbols_list 指定导出符号清单（同样可越过
        // -fvisibility=hidden 把符号提升为 dynamic exports）。
        // 注意：macOS 的符号清单格式与 Linux 版本脚本不同，只有符号名列表。
        let exports_list = manifest_dir.join("biturbo.exports.list");
        println!(
            "cargo:rustc-cdylib-link-arg=-Wl,-exported_symbols_list,{}",
            exports_list.display()
        );
    }
    // biturbo.def 作为跨平台符号清单文档与 check_exports.py 的真源。
}
