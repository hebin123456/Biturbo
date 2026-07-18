//! 端到端测试：`bt_highlight_syntax` 的轻量级语法高亮。
//!
//! 覆盖 C# / Rust / JS-TS 三种语言的注释、关键字、类型、修饰符、
//! 字符串、数字识别，以及 null 输入、空区间等边界场景。
//!
//! 注意：区间偏移以 UTF-16 code unit 为单位；ASCII 文本中 UTF-16 偏移 == 字节偏移。

use biturbo::ffi::bt_highlight_syntax::{bt_highlight_syntax, BtHighlightedDiff};
use biturbo::ffi::bt_release_vec::bt_release_highlight_syntax;
use biturbo::ffi::types::BtRange;
use std::ffi::CString;

#[test]
fn highlight_csharp_modifier_public() {
    // "public" 在 C# 中是修饰符（style 5）
    let diff = CString::new("public").unwrap();
    let file = CString::new("test.cs").unwrap();
    let ranges = [BtRange { start: 0, end: 6 }];
    let mut result = BtHighlightedDiff {
        items: std::ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_highlight_syntax(
            file.as_ptr(),
            diff.as_ptr(),
            ranges.as_ptr(),
            ranges.len() as i64,
            &mut result,
        )
    };
    assert_eq!(rc, 0, "应返回 0");
    assert_eq!(result.items_len, 1, "应识别出 1 个区间");
    assert!(!result.items.is_null());

    let item = unsafe { &*result.items };
    assert_eq!(item.style, 5, "public 应为 style 5（修饰符）");

    unsafe { bt_release_highlight_syntax(&mut result as *mut _ as *mut _) };
}

#[test]
fn highlight_rust_keyword_fn() {
    // "fn" 在 Rust 中是关键字（style 2）
    let diff = CString::new("fn").unwrap();
    let file = CString::new("main.rs").unwrap();
    let ranges = [BtRange { start: 0, end: 2 }];
    let mut result = BtHighlightedDiff {
        items: std::ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_highlight_syntax(
            file.as_ptr(),
            diff.as_ptr(),
            ranges.as_ptr(),
            ranges.len() as i64,
            &mut result,
        )
    };
    assert_eq!(rc, 0);
    assert_eq!(result.items_len, 1);
    let item = unsafe { &*result.items };
    assert_eq!(item.style, 2, "fn 应为 style 2（关键字）");

    unsafe { bt_release_highlight_syntax(&mut result as *mut _ as *mut _) };
}

#[test]
fn highlight_comment_style_zero() {
    // "// 注释" 在任何语言中都是注释（style 0）
    let diff = CString::new("// 这是一条注释").unwrap();
    let file = CString::new("code.rs").unwrap();
    let ranges = [BtRange { start: 0, end: 8 }];
    let mut result = BtHighlightedDiff {
        items: std::ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_highlight_syntax(
            file.as_ptr(),
            diff.as_ptr(),
            ranges.as_ptr(),
            ranges.len() as i64,
            &mut result,
        )
    };
    assert_eq!(rc, 0);
    assert_eq!(result.items_len, 1);
    let item = unsafe { &*result.items };
    assert_eq!(item.style, 0, "注释应为 style 0");

    unsafe { bt_release_highlight_syntax(&mut result as *mut _ as *mut _) };
}

#[test]
fn highlight_string_literal_style_one() {
    // 以双引号开头的区间识别为字符串（style 1）
    let diff = CString::new("\"hello world\"").unwrap();
    let file = CString::new("script.js").unwrap();
    let ranges = [BtRange { start: 0, end: 13 }];
    let mut result = BtHighlightedDiff {
        items: std::ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_highlight_syntax(
            file.as_ptr(),
            diff.as_ptr(),
            ranges.as_ptr(),
            ranges.len() as i64,
            &mut result,
        )
    };
    assert_eq!(rc, 0);
    assert_eq!(result.items_len, 1);
    let item = unsafe { &*result.items };
    assert_eq!(item.style, 1, "字符串应为 style 1");

    unsafe { bt_release_highlight_syntax(&mut result as *mut _ as *mut _) };
}

#[test]
fn highlight_csharp_type_class() {
    // "class" 在 C# 中是类型（style 3）
    let diff = CString::new("class").unwrap();
    let file = CString::new("Program.cs").unwrap();
    let ranges = [BtRange { start: 0, end: 5 }];
    let mut result = BtHighlightedDiff {
        items: std::ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_highlight_syntax(
            file.as_ptr(),
            diff.as_ptr(),
            ranges.as_ptr(),
            ranges.len() as i64,
            &mut result,
        )
    };
    assert_eq!(rc, 0);
    assert_eq!(result.items_len, 1);
    let item = unsafe { &*result.items };
    assert_eq!(item.style, 3, "class 在 C# 中应为 style 3（类型）");

    unsafe { bt_release_highlight_syntax(&mut result as *mut _ as *mut _) };
}

#[test]
fn highlight_multiple_ranges() {
    // 多个区间同时高亮
    let diff = CString::new("public fn class").unwrap();
    let file = CString::new("multi.rs").unwrap();
    // public 在 Rust 中不识别，fn 是关键字，class 不识别
    let ranges = [
        BtRange { start: 0, end: 6 },   // "public"
        BtRange { start: 7, end: 9 },   // "fn"
        BtRange { start: 10, end: 15 }, // "class"
    ];
    let mut result = BtHighlightedDiff {
        items: std::ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_highlight_syntax(
            file.as_ptr(),
            diff.as_ptr(),
            ranges.as_ptr(),
            ranges.len() as i64,
            &mut result,
        )
    };
    assert_eq!(rc, 0);
    // 只有 "fn" 被识别为关键字；"public" 和 "class" 在 Rust 中不匹配
    assert_eq!(result.items_len, 1, "Rust 中只有 fn 被识别");
    let item = unsafe { &*result.items };
    assert_eq!(item.style, 2);

    unsafe { bt_release_highlight_syntax(&mut result as *mut _ as *mut _) };
}

#[test]
fn highlight_null_file_path_returns_empty() {
    // null file_path 应返回 0 但结果为空（跳过语言规则）
    let diff = CString::new("public").unwrap();
    let ranges = [BtRange { start: 0, end: 6 }];
    let mut result = BtHighlightedDiff {
        items: std::ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_highlight_syntax(
            std::ptr::null(),
            diff.as_ptr(),
            ranges.as_ptr(),
            ranges.len() as i64,
            &mut result,
        )
    };
    assert_eq!(rc, 0, "null file_path 应返回 0");
    assert_eq!(result.items_len, 0, "无语言规则时不应产生高亮");
}

#[test]
fn highlight_null_diff_returns_empty() {
    let file = CString::new("test.cs").unwrap();
    let ranges = [BtRange { start: 0, end: 6 }];
    let mut result = BtHighlightedDiff {
        items: std::ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_highlight_syntax(
            file.as_ptr(),
            std::ptr::null(),
            ranges.as_ptr(),
            ranges.len() as i64,
            &mut result,
        )
    };
    assert_eq!(rc, 0, "null diff 应返回 0");
    assert_eq!(result.items_len, 0);
}

#[test]
fn highlight_zero_ranges_returns_empty() {
    let diff = CString::new("public").unwrap();
    let file = CString::new("test.cs").unwrap();
    let mut result = BtHighlightedDiff {
        items: std::ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_highlight_syntax(
            file.as_ptr(),
            diff.as_ptr(),
            std::ptr::null(),
            0,
            &mut result,
        )
    };
    assert_eq!(rc, 0, "0 个区间应返回 0");
    assert_eq!(result.items_len, 0);
}

#[test]
fn highlight_number_style_eight() {
    // 以数字开头的区间识别为数字（style 8）
    let diff = CString::new("42").unwrap();
    let file = CString::new("calc.rs").unwrap();
    let ranges = [BtRange { start: 0, end: 2 }];
    let mut result = BtHighlightedDiff {
        items: std::ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_highlight_syntax(
            file.as_ptr(),
            diff.as_ptr(),
            ranges.as_ptr(),
            ranges.len() as i64,
            &mut result,
        )
    };
    assert_eq!(rc, 0);
    assert_eq!(result.items_len, 1, "数字 42 应被识别");
    let item = unsafe { &*result.items };
    assert_eq!(item.style, 8, "数字应为 style 8");

    unsafe { bt_release_highlight_syntax(&mut result as *mut _ as *mut _) };
}

#[test]
fn highlight_unknown_extension_no_match() {
    // 未知扩展名不匹配任何语言规则，"fn" 不会被识别
    let diff = CString::new("fn").unwrap();
    let file = CString::new("readme.txt").unwrap();
    let ranges = [BtRange { start: 0, end: 2 }];
    let mut result = BtHighlightedDiff {
        items: std::ptr::null_mut(),
        items_len: 0,
        items_cap: 0,
    };
    let rc = unsafe {
        bt_highlight_syntax(
            file.as_ptr(),
            diff.as_ptr(),
            ranges.as_ptr(),
            ranges.len() as i64,
            &mut result,
        )
    };
    assert_eq!(rc, 0);
    assert_eq!(result.items_len, 0, "未知扩展名不应产生高亮");
}
