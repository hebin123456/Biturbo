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

// Callback signature for bt_spawn_with_callback
pub type ReadLineCallback = unsafe extern "C" fn(
    cb_target_ptr: *mut c_void,
    kind: u8,
    data_ptr: *const u8,
    data_len: i64,
);

#[repr(C)]
pub struct BtProcessCancellationToken {
    pub inner: *mut c_void,
}

#[repr(C)]
pub struct BtSpawnWithOutputResult {
    pub status: i32,
    _pad: u32,
    pub stdout: BtBuf,
    pub stderr: BtBuf,
}

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

    // Copy stdout to process heap
    let stdout_len = output.stdout.len();
    let stdout_ptr = unsafe { heap_alloc(stdout_len) };
    if !stdout_ptr.is_null() && stdout_len > 0 {
        core::ptr::copy_nonoverlapping(output.stdout.as_ptr(), stdout_ptr, stdout_len);
    }

    // Copy stderr to process heap
    let stderr_len = output.stderr.len();
    let stderr_ptr = unsafe { heap_alloc(stderr_len) };
    if !stderr_ptr.is_null() && stderr_len > 0 {
        core::ptr::copy_nonoverlapping(output.stderr.as_ptr(), stderr_ptr, stderr_len);
    }

    unsafe {
        (*out_result).status = output.status.code().unwrap_or(0);
        (*out_result).stdout = BtBuf { ptr: stdout_ptr as _, len: stdout_len, cap: stdout_len };
        (*out_result).stderr = BtBuf { ptr: stderr_ptr as _, len: stderr_len, cap: stderr_len };
    }

    0
}

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
