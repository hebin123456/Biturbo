//! 端到端测试：`bt_md_to_html` 的 Markdown 转 HTML 渲染。
//!
//! 覆盖标题、段落、代码块、列表等 Markdown 语法，以及 null 输入、
//! 内存释放等场景。

use biturbo::ffi::bt_markdown::{bt_md_to_html, bt_release_md_to_html};
use std::ffi::CString;

#[test]
fn render_heading_to_html() {
    let md = CString::new("# 标题\n").unwrap();
    let mut out: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc = unsafe { bt_md_to_html(md.as_ptr(), &mut out) };
    assert_eq!(rc, 0, "渲染标题应返回 0");
    assert!(!out.is_null(), "输出指针不应为 null");

    // 读取 HTML 字符串
    let html = unsafe { std::ffi::CStr::from_ptr(out) }
        .to_str()
        .expect("HTML 应为 UTF-8");
    assert!(
        html.contains("<h1>") || html.contains("<h1 "),
        "HTML 应包含 <h1> 标签，实际='{html}'"
    );

    // 释放
    unsafe { bt_release_md_to_html(&mut out) };
    assert!(out.is_null(), "释放后指针应被置 null");
}

#[test]
fn render_paragraph_to_html() {
    let md = CString::new("这是一段普通文本。\n").unwrap();
    let mut out: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc = unsafe { bt_md_to_html(md.as_ptr(), &mut out) };
    assert_eq!(rc, 0);
    assert!(!out.is_null());

    let html = unsafe { std::ffi::CStr::from_ptr(out) }
        .to_str()
        .expect("UTF-8");
    assert!(
        html.contains("这是一段普通文本"),
        "HTML 应包含原文文本，实际='{html}'"
    );

    unsafe { bt_release_md_to_html(&mut out) };
}

#[test]
fn render_code_block_to_html() {
    let md = CString::new("```\nfn main() {}\n```\n").unwrap();
    let mut out: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc = unsafe { bt_md_to_html(md.as_ptr(), &mut out) };
    assert_eq!(rc, 0);

    let html = unsafe { std::ffi::CStr::from_ptr(out) }
        .to_str()
        .expect("UTF-8");
    assert!(
        html.contains("<pre") || html.contains("<code"),
        "代码块应渲染为 <pre>/<code>，实际='{html}'"
    );

    unsafe { bt_release_md_to_html(&mut out) };
}

#[test]
fn render_unordered_list_to_html() {
    let md = CString::new("- 项目一\n- 项目二\n- 项目三\n").unwrap();
    let mut out: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc = unsafe { bt_md_to_html(md.as_ptr(), &mut out) };
    assert_eq!(rc, 0);

    let html = unsafe { std::ffi::CStr::from_ptr(out) }
        .to_str()
        .expect("UTF-8");
    assert!(
        html.contains("<ul") || html.contains("<li"),
        "列表应渲染为 <ul>/<li>，实际='{html}'"
    );

    unsafe { bt_release_md_to_html(&mut out) };
}

#[test]
fn render_empty_markdown() {
    let md = CString::new("").unwrap();
    let mut out: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc = unsafe { bt_md_to_html(md.as_ptr(), &mut out) };
    assert_eq!(rc, 0, "空 Markdown 应返回 0");
    assert!(!out.is_null(), "空 Markdown 也应返回有效指针");

    unsafe { bt_release_md_to_html(&mut out) };
    assert!(out.is_null());
}

#[test]
fn null_input_returns_error() {
    let mut out: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc = unsafe { bt_md_to_html(std::ptr::null(), &mut out) };
    assert_eq!(rc, 1, "null 输入应返回 1");
    assert!(out.is_null(), "失败时输出应保持 null");
}

#[test]
fn null_output_pointer_returns_error() {
    let md = CString::new("# test\n").unwrap();
    let rc = unsafe { bt_md_to_html(md.as_ptr(), std::ptr::null_mut()) };
    assert_eq!(rc, 1, "null 输出指针应返回 1");
}

#[test]
fn release_null_pointer_is_safe() {
    // 释放 null 指针不应崩溃
    unsafe { bt_release_md_to_html(std::ptr::null_mut()) };
}

#[test]
fn double_release_after_first_sets_null() {
    let md = CString::new("# 双重释放测试\n").unwrap();
    let mut out: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc = unsafe { bt_md_to_html(md.as_ptr(), &mut out) };
    assert_eq!(rc, 0);
    assert!(!out.is_null());

    // 第一次释放——指针被置 null
    unsafe { bt_release_md_to_html(&mut out) };
    assert!(out.is_null());

    // 第二次释放——out 已为 null，应安全无操作
    unsafe { bt_release_md_to_html(&mut out) };
    assert!(out.is_null());
}

#[test]
fn render_bold_and_italic() {
    let md = CString::new("**粗体**和*斜体*\n").unwrap();
    let mut out: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc = unsafe { bt_md_to_html(md.as_ptr(), &mut out) };
    assert_eq!(rc, 0);

    let html = unsafe { std::ffi::CStr::from_ptr(out) }
        .to_str()
        .expect("UTF-8");
    // 粗体应包含 <strong> 或 <b>
    assert!(
        html.contains("<strong") || html.contains("<b>"),
        "粗体应渲染为 <strong>/<b>，实际='{html}'"
    );

    unsafe { bt_release_md_to_html(&mut out) };
}

#[test]
fn render_link_to_html() {
    let md = CString::new("[链接文本](https://example.com)\n").unwrap();
    let mut out: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc = unsafe { bt_md_to_html(md.as_ptr(), &mut out) };
    assert_eq!(rc, 0);

    let html = unsafe { std::ffi::CStr::from_ptr(out) }
        .to_str()
        .expect("UTF-8");
    assert!(
        html.contains("<a ") && html.contains("href"),
        "链接应渲染为 <a href>，实际='{html}'"
    );

    unsafe { bt_release_md_to_html(&mut out) };
}
