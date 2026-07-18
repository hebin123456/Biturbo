//! # 子进程派生与取消
//!
//! 提供 [`bt_spawn_with_output`]（一次性收集 stdout/stderr）与
//! [`bt_spawn_with_callback`]（按行回调 stdout/stderr）两种子进程派生方式，
//! 配套 [`bt_new_process_cancellation_token`] /
//! [`bt_kill_process_cancellation_token`] /
//! [`bt_release_process_cancellation_token`] 管理取消令牌。
//!
//! # 错误码
//! 与其他模块一致：`0` 成功、`1` 失败。

use crate::ffi::error::set_last_error_str;
use crate::ffi::types::BtBuf;
use crate::ffi::winheap::{heap_alloc, heap_free};
use std::ffi::CStr;
use std::io::Write;
use std::os::raw::{c_char, c_int, c_void};
use std::process::{Command, Stdio, Child};
use std::sync::{Mutex, OnceLock};
use std::collections::HashSet;
use std::thread;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

static ACTIVE_PROCESS_TOKENS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

fn get_active_process_tokens() -> &'static Mutex<HashSet<usize>> {
    ACTIVE_PROCESS_TOKENS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn legacy_vec_capacity(len: usize) -> usize {
    len.next_power_of_two().max(16)
}

unsafe fn heap_alloc_legacy_output(bytes: &[u8]) -> (*mut u8, usize) {
    let cap = legacy_vec_capacity(bytes.len());
    let ptr = unsafe { heap_alloc(cap) };
    if !ptr.is_null() && !bytes.is_empty() {
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        }
    }
    (ptr, cap)
}

/// 子进程输出回调函数签名。
///
/// 由 [`bt_spawn_with_callback`] 在每读到一行 stdout/stderr 时调用。
///
/// # 参数
/// - `cb_target_ptr`：调用方传入的不透明上下文指针，原样回传。
/// - `kind`：`0` 表示 stdout，`1` 表示 stderr。
/// - `data_ptr` / `data_len`：本行字节流（含行尾 `\n`，最后一次可能不含）。
pub type ReadLineCallback = unsafe extern "C" fn(
    cb_target_ptr: *mut c_void,
    kind: u8,
    data_ptr: *const u8,
    data_len: i64,
);

/// 子进程取消令牌的 C ABI 包装。
///
/// `inner` 指向堆分配的 [`ProcessCancellationToken`]，必须由
/// [`bt_release_process_cancellation_token`] 释放。
#[repr(C)]
pub struct BtProcessCancellationToken {
    pub inner: *mut c_void,
}

/// [`bt_spawn_with_output`] 的返回结果。
///
/// # 字段
/// - `status`：子进程退出码；spawn/wait 失败时为 `-1`。
/// - `_pad`：对齐填充。
/// - `stdout` / `stderr`：通过进程堆分配的字节缓冲区。
///
/// # 内存所有权
/// `stdout` 与 `stderr` 必须由 [`bt_release_spawn_with_output_result`] 释放。
#[repr(C)]
pub struct BtSpawnWithOutputResult {
    pub status: i32,
    _pad: u32,
    pub stdout: BtBuf,
    pub stderr: BtBuf,
}

/// [`bt_spawn_with_callback`] 的返回结果。
///
/// # 字段
/// - `status`：子进程退出码；spawn 失败或被取消时为 `-1`。
#[repr(C)]
pub struct BtSpawnWithCallbackResult {
    pub status: i32,
}

struct ProcessCancelState {
    child: Option<Child>,
    cancelled: bool,
}

pub struct ProcessCancellationToken {
    state: Mutex<ProcessCancelState>,
}

/// 创建一个子进程取消令牌。
///
/// 用于配合 [`bt_spawn_with_callback`] 实现对子进程的取消控制。
/// 返回的句柄必须用 [`bt_release_process_cancellation_token`] 释放。
///
/// # 内存所有权
/// `inner` 指向堆分配的内部状态，跨 FFI 边界由调用方持有所有权。
#[no_mangle]
pub unsafe extern "C" fn bt_new_process_cancellation_token() -> BtProcessCancellationToken {
    let token = Box::new(ProcessCancellationToken {
        state: Mutex::new(ProcessCancelState {
            child: None,
            cancelled: false,
        }),
    });
    let ptr = Box::into_raw(token);
    {
        let mut lock = get_active_process_tokens().lock().unwrap();
        lock.insert(ptr as usize);
    }
    BtProcessCancellationToken {
        inner: ptr as *mut c_void,
    }
}

/// 取消由 [`bt_new_process_cancellation_token`] 创建的令牌。
///
/// 设置令牌的“已取消”标志。若已有子进程与该令牌关联，
/// 会立即向子进程发送 kill。
///
/// # 参数
/// - `token`：令牌指针；为 `null` 或非活动句柄返回 `1`。
///
/// # 返回值
/// - `0`：成功。
/// - `1`：令牌无效。
#[no_mangle]
pub unsafe extern "C" fn bt_kill_process_cancellation_token(token: *mut BtProcessCancellationToken) -> c_int {
    if token.is_null() {
        set_last_error_str("invalid cancellation token");
        return 1;
    }
    let lock = get_active_process_tokens().lock().unwrap();
    let ptr = (*token).inner as *mut ProcessCancellationToken;
    if ptr.is_null() || !lock.contains(&(ptr as usize)) {
        set_last_error_str("invalid cancellation token");
        return 1;
    }

    let token_ref = &*(ptr);
    let mut state = token_ref.state.lock().unwrap();
    state.cancelled = true;

    if let Some(ref mut child) = state.child {
        // Kill the child process natively
        let _ = child.kill();
    }
    0
}

/// 释放由 [`bt_new_process_cancellation_token`] 创建的令牌。
///
/// 会从活动令牌集合中移除并回收 `Box`。`*token.inner` 会被置 `null`，
/// 重复释放安全。传入 `null` 安全。
///
/// # 内存所有权
/// 仅可释放由 [`bt_new_process_cancellation_token`] 返回的令牌。
#[no_mangle]
pub unsafe extern "C" fn bt_release_process_cancellation_token(token: *mut BtProcessCancellationToken) {
    if token.is_null() {
        return;
    }
    let mut lock = get_active_process_tokens().lock().unwrap();
    let ptr = std::ptr::replace(&mut (*token).inner, core::ptr::null_mut()) as *mut ProcessCancellationToken;
    if !ptr.is_null() {
        if lock.remove(&(ptr as usize)) {
            let _ = Box::from_raw(ptr);
        }
    }
}

/// 派生子进程并一次性收集 stdout / stderr。
///
/// # 参数
/// - `path`：可执行文件路径（NUL 终止 UTF-8）。
/// - `current_dir`：可选工作目录；为 `null` 表示继承。
/// - `args_ptr` / `args_len`：参数字符串数组（每个为 NUL 终止 UTF-8）。
/// - `env_ptr` / `env_len`：环境变量扁平数组 `[key, val, key, val, ...]`。
/// - `stdin_ptr` / `stdin_len`：可选 stdin 字节流；为 `null` 时把 stdin 接到 `Stdio::null()`。
/// - `out_result`：输出 [`BtSpawnWithOutputResult`]。
///
/// # 返回值
/// - `0`：成功（无论退出码是多少）。
/// - `1`：参数非法、spawn 失败或内存不足。
///
/// # 内存所有权
/// 输出的 `stdout` / `stderr` 通过进程堆分配，必须用
/// [`bt_release_spawn_with_output_result`] 释放。
#[no_mangle]
pub unsafe extern "C" fn bt_spawn_with_output(
    path: *const c_char,
    current_dir: *const c_char,
    args_ptr: *const *const c_char,
    args_len: i64,
    env_ptr: *const *const c_char,
    env_len: i64,
    stdin_ptr: *const u8,
    stdin_len: i64,
    out_result: *mut BtSpawnWithOutputResult,
) -> c_int {
    if path.is_null() || out_result.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    // Initialize output
    unsafe {
        (*out_result).status = -1;
        (*out_result).stdout = BtBuf { ptr: core::ptr::null_mut(), len: 0, cap: 0 };
        (*out_result).stderr = BtBuf { ptr: core::ptr::null_mut(), len: 0, cap: 0 };
    }

    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8 executable path");
            return 1;
        }
    };

    let mut cmd = Command::new(path_str);

    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW - Prevent popping cmd.exe/git.exe console window

    if !current_dir.is_null() {
        if let Ok(dir) = CStr::from_ptr(current_dir).to_str() {
            cmd.current_dir(dir);
        }
    }

    // Parse args
    if !args_ptr.is_null() && args_len > 0 {
        for i in 0..args_len {
            let arg_ptr = *args_ptr.add(i as usize);
            if !arg_ptr.is_null() {
                if let Ok(arg) = CStr::from_ptr(arg_ptr).to_str() {
                    cmd.arg(arg);
                }
            }
        }
    }

    // Parse env (flat key-value pairs)
    if !env_ptr.is_null() && env_len > 0 {
        let mut i = 0;
        while i + 1 < env_len {
            let key_ptr = *env_ptr.add(i as usize);
            let val_ptr = *env_ptr.add((i + 1) as usize);
            if !key_ptr.is_null() && !val_ptr.is_null() {
                if let (Ok(key), Ok(val)) = (CStr::from_ptr(key_ptr).to_str(), CStr::from_ptr(val_ptr).to_str()) {
                    cmd.env(key, val);
                }
            }
            i += 2;
        }
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    if !stdin_ptr.is_null() && stdin_len > 0 {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            set_last_error_str(&format!("failed to spawn process '{}': {e}", path_str));
            return 1;
        }
    };

    // Write stdin if provided
    if !stdin_ptr.is_null() && stdin_len > 0 {
        if let Some(mut stdin) = child.stdin.take() {
            let data = std::slice::from_raw_parts(stdin_ptr, stdin_len as usize);
            let _ = stdin.write_all(data);
        }
    }

    // Collect output
    let output = match child.wait_with_output() {
        Ok(out) => out,
        Err(e) => {
            set_last_error_str(&format!("wait failed: {e}"));
            return 1;
        }
    };

    let stdout_len = output.stdout.len();
    let (stdout_ptr, stdout_cap) = unsafe { heap_alloc_legacy_output(&output.stdout) };
    if stdout_ptr.is_null() {
        set_last_error_str("insufficient memory");
        return 1;
    }

    let stderr_len = output.stderr.len();
    let (stderr_ptr, stderr_cap) = unsafe { heap_alloc_legacy_output(&output.stderr) };
    if stderr_ptr.is_null() {
        unsafe { heap_free(stdout_ptr as _) };
        set_last_error_str("insufficient memory");
        return 1;
    }

    unsafe {
        (*out_result).status = output.status.code().unwrap_or(0);
        (*out_result).stdout = BtBuf { ptr: stdout_ptr as _, len: stdout_len, cap: stdout_cap };
        (*out_result).stderr = BtBuf { ptr: stderr_ptr as _, len: stderr_len, cap: stderr_cap };
    }

    0
}

/// 释放 [`bt_spawn_with_output`] 返回的 [`BtSpawnWithOutputResult`] 中的
/// `stdout` 与 `stderr` 缓冲区。
///
/// 仅当对应 `BtBuf.cap != 0` 时才会释放，并把 `cap` / `len` 清零，
/// 重复释放安全。传入 `null` 安全。
///
/// # 内存所有权
/// 仅可释放由 [`bt_spawn_with_output`] 填充的字段。
#[no_mangle]
pub unsafe extern "C" fn bt_release_spawn_with_output_result(p: *mut BtSpawnWithOutputResult) {
    if p.is_null() {
        return;
    }
    let stdout = &mut (*p).stdout;
    if (*stdout).cap != 0 {
        let ptr = std::ptr::replace(&mut (*stdout).ptr, core::ptr::null_mut());
        (*stdout).cap = 0;
        (*stdout).len = 0;
        if !ptr.is_null() {
            heap_free(ptr);
        }
    }
    let stderr = &mut (*p).stderr;
    if (*stderr).cap != 0 {
        let ptr = std::ptr::replace(&mut (*stderr).ptr, core::ptr::null_mut());
        (*stderr).cap = 0;
        (*stderr).len = 0;
        if !ptr.is_null() {
            heap_free(ptr);
        }
    }
}

/// 派生子进程并通过回调按行读取 stdout / stderr。
///
/// 与 [`bt_spawn_with_output`] 不同：本函数不会把整个输出缓存到内存，
/// 而是每读到一行就调用 `read_line_cb`。可通过 `process_cancellation_token_ptr`
/// 关联取消令牌，外部调用 [`bt_kill_process_cancellation_token`] 即可终止子进程。
///
/// # 参数
/// - `path` / `current_dir` / `args_ptr` / `args_len` / `env_ptr` / `env_len` /
///   `stdin_ptr` / `stdin_len`：同 [`bt_spawn_with_output`]。
/// - `cb_target_ptr`：原样回传给回调的不透明上下文指针。
/// - `read_line_cb`：每行回调，签名见 [`ReadLineCallback`]。
/// - `process_cancellation_token_ptr`：可选取消令牌；为 `null` 表示不参与取消。
/// - `out_result`：输出 [`BtSpawnWithCallbackResult`]，仅含退出码。
///
/// # 返回值
/// - `0`：成功（无论退出码是多少，包括被取消的情况）。
/// - `1`：参数非法或 spawn 失败。
///
/// # 内存所有权
/// 本函数不返回任何堆分配缓冲区；调用方在回调中接收的 `data_ptr` 仅在回调期间有效，
/// 回调返回后即被复用，调用方需要自行复制。
#[no_mangle]
pub unsafe extern "C" fn bt_spawn_with_callback(
    path: *const c_char,
    current_dir: *const c_char,
    args_ptr: *const *const c_char,
    args_len: i64,
    env_ptr: *const *const c_char,
    env_len: i64,
    stdin_ptr: *const u8,
    stdin_len: i64,
    cb_target_ptr: *mut c_void,
    read_line_cb: ReadLineCallback,
    process_cancellation_token_ptr: *mut BtProcessCancellationToken,
    out_result: *mut BtSpawnWithCallbackResult,
) -> c_int {
    if path.is_null() || out_result.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_result).status = -1;
    }

    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8 executable path");
            return 1;
        }
    };

    let mut cmd = Command::new(path_str);

    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW - Prevent popping cmd.exe/git.exe console window

    if !current_dir.is_null() {
        if let Ok(dir) = CStr::from_ptr(current_dir).to_str() {
            cmd.current_dir(dir);
        }
    }

    // Parse args
    if !args_ptr.is_null() && args_len > 0 {
        for i in 0..args_len {
            let arg_ptr = *args_ptr.add(i as usize);
            if !arg_ptr.is_null() {
                if let Ok(arg) = CStr::from_ptr(arg_ptr).to_str() {
                    cmd.arg(arg);
                }
            }
        }
    }

    // Parse env
    if !env_ptr.is_null() && env_len > 0 {
        let mut i = 0;
        while i + 1 < env_len {
            let key_ptr = *env_ptr.add(i as usize);
            let val_ptr = *env_ptr.add((i + 1) as usize);
            if !key_ptr.is_null() && !val_ptr.is_null() {
                if let (Ok(key), Ok(val)) = (CStr::from_ptr(key_ptr).to_str(), CStr::from_ptr(val_ptr).to_str()) {
                    cmd.env(key, val);
                }
            }
            i += 2;
        }
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    if !stdin_ptr.is_null() && stdin_len > 0 {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            set_last_error_str(&format!("failed to spawn process '{}': {e}", path_str));
            return 1;
        }
    };

    let mut child_opt = Some(child);

    // Register child with cancellation token if provided
    let mut was_cancelled = false;
    let mut has_token = false;
    let mut token_inner = core::ptr::null_mut();
    
    if !process_cancellation_token_ptr.is_null() {
        let lock = get_active_process_tokens().lock().unwrap();
        let ptr = (*process_cancellation_token_ptr).inner as *mut ProcessCancellationToken;
        if !ptr.is_null() && lock.contains(&(ptr as usize)) {
            has_token = true;
            token_inner = ptr;
            let token_ref = &*ptr;
            let state = token_ref.state.lock().unwrap();
            if state.cancelled {
                was_cancelled = true;
                if let Some(mut c) = child_opt.take() {
                    let _ = c.kill();
                }
            }
        }
    }

    if was_cancelled {
        unsafe { (*out_result).status = -1 };
        return 0;
    }

    // Take stdout/stderr pipes
    let stdout_pipe = child_opt.as_mut().unwrap().stdout.take().unwrap();
    let stderr_pipe = child_opt.as_mut().unwrap().stderr.take().unwrap();

    // Write stdin if provided
    if !stdin_ptr.is_null() && stdin_len > 0 {
        if let Some(mut stdin) = child_opt.as_mut().unwrap().stdin.take() {
            let data = std::slice::from_raw_parts(stdin_ptr, stdin_len as usize);
            let _ = stdin.write_all(data);
        }
    }

    // We can spawn stdout and stderr reading threads
    // In order to invoke callbacks, we pass target pointer, kind, data, and length.
    // Read line-by-line or chunk-by-chunk. Let's read chunk-by-chunk (or line-by-chunk) to be compatible.
    // Standard callbacks expect standard outputs. Let's read into buffers and invoke callback.
    let target = cb_target_ptr as usize;
    let cb = read_line_cb;

    let stdout_thread = thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        let mut reader = BufReader::new(stdout_pipe);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => unsafe {
                    cb(target as *mut c_void, 0, line.as_ptr(), line.len() as i64);
                },
                Err(_) => break,
            }
        }
    });

    let stderr_thread = thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        let mut reader = BufReader::new(stderr_pipe);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => unsafe {
                    cb(target as *mut c_void, 1, line.as_ptr(), line.len() as i64);
                },
                Err(_) => break,
            }
        }
    });

    // Store child in cancellation token for concurrent kill
    if has_token {
        let lock = get_active_process_tokens().lock().unwrap();
        if lock.contains(&(token_inner as usize)) {
            let token_ref = &*(token_inner);
            let mut state = token_ref.state.lock().unwrap();
            state.child = child_opt.take();
        }
    }

    stdout_thread.join().unwrap();
    stderr_thread.join().unwrap();

    let mut child_to_wait = child_opt;
    if has_token {
        let lock = get_active_process_tokens().lock().unwrap();
        if lock.contains(&(token_inner as usize)) {
            let token_ref = &*(token_inner);
            let mut state = token_ref.state.lock().unwrap();
            if let Some(c) = state.child.take() {
                child_to_wait = Some(c);
            }
        }
    }

    let status = if let Some(mut c) = child_to_wait {
        c.wait().map(|s| s.code().unwrap_or(0)).unwrap_or(-1)
    } else {
        -1
    };

    unsafe {
        (*out_result).status = status;
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_vec_capacity_minimum_floor_is_16() {
        // Matches original DLL: spawn output buffers are never smaller than 16 bytes.
        assert_eq!(legacy_vec_capacity(0), 16);
        assert_eq!(legacy_vec_capacity(1), 16);
        assert_eq!(legacy_vec_capacity(15), 16);
    }

    #[test]
    fn legacy_vec_capacity_exact_powers_of_two() {
        assert_eq!(legacy_vec_capacity(16), 16);
        assert_eq!(legacy_vec_capacity(32), 32);
        assert_eq!(legacy_vec_capacity(64), 64);
        assert_eq!(legacy_vec_capacity(256), 256);
    }

    #[test]
    fn legacy_vec_capacity_rounds_up_to_next_power_of_two() {
        assert_eq!(legacy_vec_capacity(17), 32);
        assert_eq!(legacy_vec_capacity(33), 64);
        assert_eq!(legacy_vec_capacity(100), 128);
        assert_eq!(legacy_vec_capacity(1000), 1024);
    }

    #[test]
    fn legacy_vec_capacity_always_ge_input() {
        for n in [0usize, 1, 7, 16, 17, 100, 1023, 1024, 1025] {
            assert!(legacy_vec_capacity(n) >= n, "capacity < len for n={n}");
        }
    }
}
