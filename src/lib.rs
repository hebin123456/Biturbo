//! Biturbo — a Rust cdylib providing a zlib/libgit2-compatible DLL with an
//! extended high-performance `bt_*` Git repository API.
//!
//! This crate is built as a `cdylib`, exporting symbols listed in `biturbo.def`.
//! The implementation is split into modules under `src/ffi/`.

#![allow(non_snake_case)]

mod ffi;

