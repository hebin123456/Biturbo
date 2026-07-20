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
        // Linux：libz-sys 编译 zlib 时启用了 `-fvisibility=hidden`，导致所有 zlib
        // 符号在 .so 中是 local 而非 global。链接器版本脚本可以显式列出需要导出
        // 的符号并把它们提升为 dynamic symbol table 中的 global 项。
        // `src/ffi/zlib_touch.rs` 中的 `#[used] static ZLIB_ANCHORS` 负责把 zlib
        // 静态库中的对象代码拉入链接输入，本脚本再把这些符号从 local 提升为 global。
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
