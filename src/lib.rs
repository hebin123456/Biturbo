//! `biturbo.dll` reimplementation.
//!
//! This crate is built as a `cdylib`, exporting symbols listed in `biturbo.def`.
//! The implementation is split into modules under `src/ffi/`.

#![allow(non_snake_case)]

mod ffi;

