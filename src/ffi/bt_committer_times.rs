use crate::ffi::error::set_last_error_str;
use crate::ffi::types::BtOid;
use crate::ffi::winheap::heap_alloc;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;

#[repr(C)]
pub struct BtCommitterTimes {
    pub times: *mut i64,
    pub times_len: i64,
    pub times_cap: i64,
}

#[no_mangle]
pub unsafe extern "C" fn bt_get_committer_times(
    git_dir_path: *const c_char,
    oids_ptr: *const BtOid,
    oids_len: i64,
    _commit_graph_cache_ptr: *const c_void,
    out_result: *mut BtCommitterTimes,
) -> c_int {
    if git_dir_path.is_null() || oids_ptr.is_null() || oids_len <= 0 || out_result.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_result).times = core::ptr::null_mut();
        (*out_result).times_len = 0;
        (*out_result).times_cap = 0;
    }

    let git_dir_bytes = unsafe { CStr::from_ptr(git_dir_path) }.to_bytes();
    let git_dir_str = match std::str::from_utf8(git_dir_bytes) {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8 git_dir_path");
            return 1;
        }
    };
    let git_dir = Path::new(git_dir_str);

    let repo = match git2::Repository::open(git_dir) {
        Ok(r) => r,
        Err(e) => {
            set_last_error_str(&format!("failed to open repository: {e}"));
            return 1;
        }
    };

    let oids = unsafe { std::slice::from_raw_parts(oids_ptr, oids_len as usize) };
    let mut times = Vec::with_capacity(oids_len as usize);

    for oid in oids {
        let raw_oid = oid.to_bytes();
        if let Ok(git2_oid) = git2::Oid::from_bytes(&raw_oid) {
            if let Ok(commit) = repo.find_commit(git2_oid) {
                let committer = commit.committer();
                times.push(committer.when().seconds());
                continue;
            }
        }
        times.push(0);
    }

    let alloc_bytes = times.len() * std::mem::size_of::<i64>();
    let times_ptr = unsafe { heap_alloc(alloc_bytes) } as *mut i64;
    if times_ptr.is_null() {
        set_last_error_str("insufficient memory");
        return 1;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(times.as_ptr(), times_ptr, times.len());
        (*out_result).times = times_ptr;
        (*out_result).times_len = times.len() as i64;
        (*out_result).times_cap = times.len() as i64;
    }

    0
}
