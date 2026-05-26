use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::heap_free_u8;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};

#[repr(C)]
pub struct BtHead {
    pub oid20: [u8; 20],
    _pad: [u8; 4], // keep `ref_name` at offset 0x18
    pub ref_name: *mut c_char,
}

#[no_mangle]
pub unsafe extern "C" fn bt_get_head(git_dir_path: *const c_char, out_head: *mut BtHead) -> c_int {
    if git_dir_path.is_null() || out_head.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_head).oid20 = [0u8; 20];
        (*out_head).ref_name = core::ptr::null_mut();
    }

    let git_dir_bytes = unsafe { CStr::from_ptr(git_dir_path) }.to_bytes();
    let git_dir_str = match std::str::from_utf8(git_dir_bytes) {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8 git_dir_path");
            return 1;
        }
    };
    let git_dir = PathBuf::from(git_dir_str);

    match get_head_impl(&git_dir) {
        Ok((oid20, ref_name_opt)) => {
            unsafe { (*out_head).oid20 = oid20 };
            if let Some(ref_name) = ref_name_opt {
                let p = unsafe { crate::ffi::winheap::heap_alloc_c_string(&ref_name) };
                if p.is_null() {
                    set_last_error_str("insufficient memory");
                    return 1;
                }
                unsafe { (*out_head).ref_name = p };
            }
            0
        }
        Err(msg) => {
            set_last_error_str(&format!("read head in '{}': {msg}", git_dir.display()));
            1
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_head(head: *mut BtHead) {
    if head.is_null() {
        return;
    }
    let p = std::ptr::replace(&mut (*head).ref_name, core::ptr::null_mut()) as *mut u8;
    if p.is_null() {
        return;
    }
    unsafe { *p = 0 };
    unsafe { heap_free_u8(p) };
}

fn get_head_impl(git_dir: &Path) -> Result<([u8; 20], Option<String>), String> {
    let head_path = git_dir.join("HEAD");
    let head_bytes = std::fs::read(&head_path).map_err(|e| format!("open HEAD: {e}"))?;
    let head_trimmed = trim_ascii_whitespace(&head_bytes);
    if head_trimmed.is_empty() {
        return Err("empty HEAD".to_string());
    }

    if let Some(rest) = head_trimmed.strip_prefix(b"ref: ") {
        let ref_bytes = trim_ascii_whitespace(rest);
        let ref_name = std::str::from_utf8(ref_bytes)
            .map_err(|_| "non-utf8 ref name".to_string())?
            .to_string();
        Ok(([0u8; 20], Some(ref_name)))
    } else {
        let oid_hex = std::str::from_utf8(head_trimmed)
            .map_err(|_| "non-utf8 detached oid".to_string())?;
        let oid20 = parse_oid_swapped(oid_hex)?;
        Ok((oid20, None))
    }
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let mut s = 0;
    let mut e = bytes.len();
    while s < e && bytes[s].is_ascii_whitespace() {
        s += 1;
    }
    while e > s && bytes[e - 1].is_ascii_whitespace() {
        e -= 1;
    }
    &bytes[s..e]
}

fn parse_oid_swapped(hex40: &str) -> Result<[u8; 20], String> {
    let b = hex40.as_bytes();
    if b.len() != 40 {
        return Err("OID length must be 40".to_string());
    }
    let mut raw = [0u8; 20];
    for i in 0..20 {
        let hi = hex_nibble(b[i * 2]).ok_or_else(|| "invalid hash id".to_string())?;
        let lo = hex_nibble(b[i * 2 + 1]).ok_or_else(|| "invalid hash id".to_string())?;
        raw[i] = (hi << 4) | lo;
    }
    let mut out = [0u8; 20];
    for word in 0..5 {
        let base = word * 4;
        out[base + 0] = raw[base + 3];
        out[base + 1] = raw[base + 2];
        out[base + 2] = raw[base + 1];
        out[base + 3] = raw[base + 0];
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

