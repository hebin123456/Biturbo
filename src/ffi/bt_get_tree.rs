use crate::ffi::error::set_last_error_str;
use crate::ffi::types::BtOid;
use crate::ffi::winheap::{heap_alloc, heap_alloc_c_string, heap_free};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::Path;

#[repr(C)]
pub struct BtTreeItem {
    pub kind: u16,
    _pad: u16,
    pub filename: *mut c_char,
    pub treeish: BtOid,
}

#[repr(C)]
pub struct BtTree {
    pub entries: *mut BtTreeItem,
    pub entries_len: i64,
    pub entries_cap: i64,
}

#[no_mangle]
pub unsafe extern "C" fn bt_get_tree(
    git_dir_path: *const c_char,
    oid_ptr: *const BtOid,
    out_result: *mut BtTree,
) -> c_int {
    if git_dir_path.is_null() || oid_ptr.is_null() || out_result.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_result).entries = core::ptr::null_mut();
        (*out_result).entries_len = 0;
        (*out_result).entries_cap = 0;
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

    let raw_oid = unsafe { (*oid_ptr).to_bytes() };
    let git2_oid = match git2::Oid::from_bytes(&raw_oid) {
        Ok(o) => o,
        Err(e) => {
            set_last_error_str(&format!("failed to parse OID: {e}"));
            return 1;
        }
    };

    let tree = match repo.find_tree(git2_oid) {
        Ok(t) => t,
        Err(e) => {
            set_last_error_str(&format!("failed to find tree: {e}"));
            return 1;
        }
    };

    let mut entries: Vec<BtTreeItem> = Vec::new();
    for entry in tree.iter() {
        let name = entry.name().unwrap_or("");
        let filename_ptr = unsafe { heap_alloc_c_string(name) };
        if filename_ptr.is_null() {
            for ent in &entries {
                unsafe { heap_free(ent.filename as _) };
            }
            set_last_error_str("insufficient memory");
            return 1;
        }

        let child_id = entry.id();
        let bytes = child_id.as_bytes();
        let bt_oid = BtOid::from_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
            bytes[16], bytes[17], bytes[18], bytes[19]
        ]);

        entries.push(BtTreeItem {
            kind: entry.filemode() as u16,
            _pad: 0,
            filename: filename_ptr,
            treeish: bt_oid,
        });
    }

    if entries.is_empty() {
        return 0;
    }

    let alloc_bytes = entries.len() * std::mem::size_of::<BtTreeItem>();
    let entries_ptr = unsafe { heap_alloc(alloc_bytes) } as *mut BtTreeItem;
    if entries_ptr.is_null() {
        for ent in entries {
            unsafe { heap_free(ent.filename as _) };
        }
        set_last_error_str("insufficient memory");
        return 1;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(entries.as_ptr(), entries_ptr, entries.len());
        (*out_result).entries = entries_ptr;
        (*out_result).entries_len = entries.len() as i64;
        (*out_result).entries_cap = entries.len() as i64;
    }

    0
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_tree(p: *mut BtTree) {
    if p.is_null() {
        return;
    }
    let entries_ptr = std::ptr::replace(&mut (*p).entries, core::ptr::null_mut());
    let len = (*p).entries_len;
    let cap = (*p).entries_cap;
    (*p).entries_len = 0;
    (*p).entries_cap = 0;

    if !entries_ptr.is_null() {
        for i in 0..len {
            let ent = &mut *entries_ptr.add(i as usize);
            let filename = std::ptr::replace(&mut ent.filename, core::ptr::null_mut());
            if !filename.is_null() {
                heap_free(filename as _);
            }
        }
        if cap != 0 {
            heap_free(entries_ptr as _);
        }
    }
}
