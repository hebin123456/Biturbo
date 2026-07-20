//! # zlib 符号链接保障
//!
//! 确保 `libz-sys` 被实际链接进 `cdylib`，且所有 zlib 符号都被保留下来。
//!
//! 在 Windows 上，`biturbo.def` 通过 `/DEF:` 强制链接器导出列表中的全部符号，
//! 即使本 crate 没有引用。在 Linux/macOS 上，链接器默认会丢弃静态库中
//! 未被引用的对象文件，因此需要通过 `#[used]` 静态变量引用每个 zlib 符号，
//! 让对应的对象代码被拉入最终的 cdylib，从而被 Rust cdylib 的默认导出
//! 机制重导出。

use libz_sys as z;
use std::os::raw::{c_char, c_int, c_uint, c_void};

// libz_sys 未导出但 zlib 实际提供的符号（仅作链接 anchor 用）。
extern "C" {
    fn deflatePending(strm: *mut c_void, pending: *mut c_uint, bits: *mut c_int) -> c_int;
    fn inflateCodesUsed() -> c_uint;
    fn inflateResetKeep(strm: *mut c_void) -> c_int;
    fn inflateSyncPoint(strm: *mut c_void) -> c_int;
    fn inflateUndermine(strm: *mut c_void, subvert: c_int) -> c_int;
    fn zError(err: c_int) -> *const c_char;
}

/// 指向所有需要被重导出的 zlib 符号的函数指针表包装类型。
///
/// 原始指针类型 `*const ()` 不实现 `Sync`，无法直接放入 `static`。
/// 此包装通过 `unsafe impl Sync` 声明可以跨线程共享：实际上该静态
/// 只在编译期用于锚定符号链接，运行时从不读写。
#[allow(dead_code)]
struct ZlibAnchors([*const (); 42]);

// SAFETY: `ZLIB_ANCHORS` 仅作为编译期符号 anchor，运行时从不读写，
// 因此跨线程共享是安全的。
unsafe impl Sync for ZlibAnchors {}

/// 指向所有需要被重导出的 zlib 符号的函数指针表。
///
/// `#[used]` 强制编译器保留该静态变量，让链接器在解析重定位时把对应的
/// zlib 对象代码从静态库拉入最终的 cdylib，从而被 Rust cdylib 的默认
/// 导出机制重导出。仅 Linux/macOS 需要此 anchor；Windows 通过 `/DEF:`
/// 直接控制导出列表，但保留本变量也无副作用。
#[used]
static ZLIB_ANCHORS: ZlibAnchors = ZlibAnchors([
    z::adler32 as *const (),
    z::adler32_combine as *const (),
    z::compress as *const (),
    z::compress2 as *const (),
    z::compressBound as *const (),
    z::crc32 as *const (),
    z::crc32_combine as *const (),
    z::deflate as *const (),
    z::deflateBound as *const (),
    z::deflateCopy as *const (),
    z::deflateEnd as *const (),
    z::deflateInit2_ as *const (),
    z::deflateInit_ as *const (),
    z::deflateParams as *const (),
    deflatePending as *const (),
    z::deflatePrime as *const (),
    z::deflateReset as *const (),
    z::deflateSetDictionary as *const (),
    z::deflateSetHeader as *const (),
    z::deflateTune as *const (),
    z::inflate as *const (),
    z::inflateBack as *const (),
    z::inflateBackEnd as *const (),
    z::inflateBackInit_ as *const (),
    inflateCodesUsed as *const (),
    z::inflateCopy as *const (),
    z::inflateEnd as *const (),
    z::inflateGetHeader as *const (),
    z::inflateInit2_ as *const (),
    z::inflateInit_ as *const (),
    z::inflateMark as *const (),
    z::inflatePrime as *const (),
    z::inflateReset as *const (),
    z::inflateReset2 as *const (),
    inflateResetKeep as *const (),
    z::inflateSetDictionary as *const (),
    z::inflateSync as *const (),
    inflateSyncPoint as *const (),
    inflateUndermine as *const (),
    z::uncompress as *const (),
    zError as *const (),
    z::zlibVersion as *const (),
]);
