//! # zlib 符号链接保障
//!
//! 确保 `libz-sys` 被实际链接进 `cdylib`。
//!
//! `libz-sys` 定义了 zlib 的 C API 符号（如 `crc32`、`deflate`、`inflate` 等）。
//! 由于本 crate 通过 `biturbo.def` 导出这些符号，必须保证链接器从 rlib
//! 中真正拉入对应的对象代码，否则最终 DLL 会缺失 zlib 实现。本模块通过
//! 一个 `#[used] static` 引用 [`libz_sys::zlibVersion`]，强制保留符号依赖。

use libz_sys as z;
use std::os::raw::c_char;

#[used]
static ZLIB_VERSION_REF: unsafe extern "C" fn() -> *const c_char = z::zlibVersion;

