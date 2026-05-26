//! Ensure `libz-sys` is linked into the cdylib.
//!
//! `libz-sys` defines the zlib C API symbols (e.g. `crc32`, `deflate`, ...).
//! Since our crate exports those symbols via `biturbo.def`, we must ensure the
//! linker actually pulls in the corresponding object code from the rlib.

use libz_sys as z;
use std::os::raw::c_char;

#[used]
static ZLIB_VERSION_REF: unsafe extern "C" fn() -> *const c_char = z::zlibVersion;

