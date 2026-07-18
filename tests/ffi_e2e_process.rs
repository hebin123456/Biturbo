//! 端到端测试：`bt_process` 子进程派生与取消令牌。
//!
//! 覆盖取消令牌生命周期、`bt_spawn_with_output` 正常派生与错误路径、
//! 以及释放函数的安全性。子进程派生使用 `cmd.exe /c echo`，仅在 Windows CI 上运行。

use biturbo::ffi::bt_process::{
    bt_kill_process_cancellation_token, bt_new_process_cancellation_token,
    bt_release_process_cancellation_token, bt_release_spawn_with_output_result,
    bt_spawn_with_output, BtProcessCancellationToken, BtSpawnWithOutputResult,
};
use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr;

/// 创建一个零初始化的 `BtSpawnWithOutputResult`。
/// `_pad` 字段为私有，无法用结构体字面量构造，故用 `zeroed`。
fn zeroed_spawn_result() -> BtSpawnWithOutputResult {
    unsafe { std::mem::zeroed() }
}

// ---------- 取消令牌生命周期 ----------

#[test]
fn new_cancellation_token_returns_non_null_inner() {
    // 新建的令牌 inner 不应为 null
    let mut token = unsafe { bt_new_process_cancellation_token() };
    assert!(!token.inner.is_null(), "新建令牌的 inner 不应为 null");
    unsafe { bt_release_process_cancellation_token(&mut token as *mut BtProcessCancellationToken) };
    // 释放后 inner 应被置 null
    assert!(token.inner.is_null(), "释放后 inner 应为 null");
}

#[test]
fn release_cancellation_token_null_is_safe() {
    // 传入 null 指针应直接返回，不触发解引用
    unsafe { bt_release_process_cancellation_token(ptr::null_mut()) };
}

#[test]
fn release_cancellation_token_double_release_safe() {
    // 重复释放应安全（第二次 inner 已为 null）
    let mut token = unsafe { bt_new_process_cancellation_token() };
    unsafe { bt_release_process_cancellation_token(&mut token) };
    // 第二次释放：inner 已为 null，应安全返回
    unsafe { bt_release_process_cancellation_token(&mut token) };
    assert!(token.inner.is_null());
}

#[test]
fn kill_valid_token_returns_zero() {
    // 对有效令牌调用 kill 应返回 0
    let mut token = unsafe { bt_new_process_cancellation_token() };
    let rc = unsafe { bt_kill_process_cancellation_token(&mut token as *mut BtProcessCancellationToken) };
    assert_eq!(rc, 0, "kill 有效令牌应返回 0");
    unsafe { bt_release_process_cancellation_token(&mut token) };
}

#[test]
fn kill_null_token_returns_error() {
    // null 指针应返回 1
    let rc = unsafe { bt_kill_process_cancellation_token(ptr::null_mut()) };
    assert_eq!(rc, 1, "kill null 令牌应返回 1");
}

#[test]
fn kill_released_token_returns_error() {
    // 释放后再 kill 应返回 1（inner 已为 null，不在活动集合中）
    let mut token = unsafe { bt_new_process_cancellation_token() };
    unsafe { bt_release_process_cancellation_token(&mut token) };
    let rc = unsafe { bt_kill_process_cancellation_token(&mut token as *mut BtProcessCancellationToken) };
    assert_eq!(rc, 1, "kill 已释放令牌应返回 1");
}

// ---------- bt_spawn_with_output ----------

/// 构造 `cmd.exe /c echo <text>` 的参数，返回 (path_cstr, args_vec)。
fn make_echo_args(text: &str) -> (CString, Vec<CString>) {
    let path = CString::new("cmd.exe").unwrap();
    let args = vec![
        CString::new("/c").unwrap(),
        CString::new("echo").unwrap(),
        CString::new(text).unwrap(),
    ];
    (path, args)
}

#[test]
fn spawn_echo_collects_stdout() {
    // 派生 cmd.exe /c echo hello，验证 stdout 包含 "hello"
    let (path, args) = make_echo_args("hello");
    let arg_ptrs: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
    let mut result = zeroed_spawn_result();
    let rc = unsafe {
        bt_spawn_with_output(
            path.as_ptr(),
            ptr::null(),
            arg_ptrs.as_ptr(),
            arg_ptrs.len() as i64,
            ptr::null(),
            0,
            ptr::null(),
            0,
            &mut result,
        )
    };
    assert_eq!(rc, 0, "spawn echo 应返回 0");
    assert_eq!(result.status, 0, "cmd.exe /c echo 退出码应为 0");

    // stdout 应包含 "hello"（可能带 \r\n）
    let stdout = unsafe {
        std::slice::from_raw_parts(result.stdout.ptr as *const u8, result.stdout.len)
    };
    let stdout_str = String::from_utf8_lossy(stdout);
    assert!(
        stdout_str.trim().contains("hello"),
        "stdout 应包含 hello，实际: {:?}",
        stdout_str
    );

    unsafe { bt_release_spawn_with_output_result(&mut result) };
}

#[test]
fn spawn_null_path_returns_error() {
    // path 为 null 应返回 1（FFI 不触碰 out_result）
    let mut result = zeroed_spawn_result();
    let rc = unsafe {
        bt_spawn_with_output(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            &mut result,
        )
    };
    assert_eq!(rc, 1, "null path 应返回 1");
}

#[test]
fn spawn_null_out_result_returns_error() {
    // out_result 为 null 应返回 1
    let (path, _args) = make_echo_args("x");
    let rc = unsafe {
        bt_spawn_with_output(
            path.as_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null_mut(),
        )
    };
    assert_eq!(rc, 1, "null out_result 应返回 1");
}

#[test]
fn spawn_nonexistent_executable_returns_error() {
    // 不存在的可执行文件应 spawn 失败，返回 1
    // FFI 会先把 status 置 -1，再尝试 spawn
    let path = CString::new("this_executable_does_not_exist_12345.exe").unwrap();
    let mut result = zeroed_spawn_result();
    let rc = unsafe {
        bt_spawn_with_output(
            path.as_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            &mut result,
        )
    };
    assert_eq!(rc, 1, "不存在的可执行文件应返回 1");
    assert_eq!(result.status, -1, "spawn 失败时 status 应为 -1");
}

#[test]
fn spawn_with_env_vars() {
    // 通过环境变量传递值，用 cmd.exe /c echo %BT_TEST_E2E_VAR% 验证
    let path = CString::new("cmd.exe").unwrap();
    let arg_c = CString::new("/c").unwrap();
    let arg_echo = CString::new("echo").unwrap();
    let arg_val = CString::new("%BT_TEST_E2E_VAR%").unwrap();
    let args = vec![arg_c, arg_echo, arg_val];
    let arg_ptrs: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();

    let env_key = CString::new("BT_TEST_E2E_VAR").unwrap();
    let env_val = CString::new("env_value_42").unwrap();
    let env_ptrs: Vec<*const c_char> = vec![env_key.as_ptr(), env_val.as_ptr()];

    let mut result = zeroed_spawn_result();
    let rc = unsafe {
        bt_spawn_with_output(
            path.as_ptr(),
            ptr::null(),
            arg_ptrs.as_ptr(),
            arg_ptrs.len() as i64,
            env_ptrs.as_ptr(),
            env_ptrs.len() as i64,
            ptr::null(),
            0,
            &mut result,
        )
    };
    assert_eq!(rc, 0, "带环境变量的 spawn 应返回 0");
    let stdout = unsafe {
        std::slice::from_raw_parts(result.stdout.ptr as *const u8, result.stdout.len)
    };
    let stdout_str = String::from_utf8_lossy(stdout);
    assert!(
        stdout_str.trim().contains("env_value_42"),
        "stdout 应包含环境变量值，实际: {:?}",
        stdout_str
    );
    unsafe { bt_release_spawn_with_output_result(&mut result) };
}

#[test]
fn spawn_exit_code_propagated() {
    // cmd.exe /c exit 7 应返回退出码 7
    let path = CString::new("cmd.exe").unwrap();
    let arg_c = CString::new("/c").unwrap();
    let arg_exit = CString::new("exit").unwrap();
    let arg_code = CString::new("7").unwrap();
    let args = vec![arg_c, arg_exit, arg_code];
    let arg_ptrs: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();

    let mut result = zeroed_spawn_result();
    let rc = unsafe {
        bt_spawn_with_output(
            path.as_ptr(),
            ptr::null(),
            arg_ptrs.as_ptr(),
            arg_ptrs.len() as i64,
            ptr::null(),
            0,
            ptr::null(),
            0,
            &mut result,
        )
    };
    assert_eq!(rc, 0, "spawn 应返回 0");
    assert_eq!(result.status, 7, "退出码应为 7");
    unsafe { bt_release_spawn_with_output_result(&mut result) };
}

// ---------- bt_release_spawn_with_output_result ----------

#[test]
fn release_spawn_result_null_is_safe() {
    // 传入 null 应直接返回
    unsafe { bt_release_spawn_with_output_result(ptr::null_mut()) };
}

#[test]
fn release_spawn_result_double_release_safe() {
    // 重复释放应安全（第二次 cap 已为 0）
    let (path, args) = make_echo_args("double");
    let arg_ptrs: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
    let mut result = zeroed_spawn_result();
    unsafe {
        bt_spawn_with_output(
            path.as_ptr(),
            ptr::null(),
            arg_ptrs.as_ptr(),
            arg_ptrs.len() as i64,
            ptr::null(),
            0,
            ptr::null(),
            0,
            &mut result,
        );
        bt_release_spawn_with_output_result(&mut result);
        // 第二次释放：cap 已被清零，应安全返回
        bt_release_spawn_with_output_result(&mut result);
    }
    // 验证释放后字段已清零
    assert_eq!(result.stdout.cap, 0);
    assert_eq!(result.stderr.cap, 0);
}
