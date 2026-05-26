use crate::ffi::error::set_last_error_str;
use crate::ffi::types::BtOid;
use crate::ffi::winheap::{heap_alloc, heap_alloc_c_string, heap_free};
use crate::ffi::bt_stashes::BtIdentity;
use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::Path;

#[repr(C)]
pub struct BtRevisionHeader {
    pub author_index: i64,
    pub author_time: i64,
    pub subject: *mut c_char,
    pub has_body: u8,
}

#[repr(C)]
pub struct BtRevisionHeaders {
    pub revisions: *mut BtRevisionHeader,
    pub revisions_len: i64,
    pub revisions_cap: i64,
    pub identities: *mut BtIdentity,
    pub identities_len: i64,
    pub identities_cap: i64,
}

#[no_mangle]
pub unsafe extern "C" fn bt_get_revision_headers(
    _working_dir_path: *const c_char,
    git_dir_path: *const c_char,
    oids_ptr: *const BtOid,
    oids_len: i64,
    out_result: *mut BtRevisionHeaders,
) -> c_int {
    if git_dir_path.is_null() || oids_ptr.is_null() || oids_len <= 0 || out_result.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_result).revisions = core::ptr::null_mut();
        (*out_result).revisions_len = 0;
        (*out_result).revisions_cap = 0;
        (*out_result).identities = core::ptr::null_mut();
        (*out_result).identities_len = 0;
        (*out_result).identities_cap = 0;
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

    let mut revisions = Vec::new();
    let mut identity_map = HashMap::new();
    let mut identity_list = Vec::new();

    for oid in oids {
        let raw_oid = oid.to_bytes();
        let git2_oid = match git2::Oid::from_bytes(&raw_oid) {
            Ok(o) => o,
            Err(_) => continue,
        };
        let commit = match repo.find_commit(git2_oid) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let author = commit.author();
        let author_name = author.name().unwrap_or("");
        let author_email = author.email().unwrap_or("");
        let author_time = author.when().seconds();

        let id_key = (author_name.to_string(), author_email.to_string());
        let author_index = *identity_map.entry(id_key.clone()).or_insert_with(|| {
            let idx = identity_list.len() as i64;
            identity_list.push(id_key);
            idx
        });

        let subject = commit.summary().ok().flatten().unwrap_or("");
        let subject_ptr = unsafe { heap_alloc_c_string(subject) };

        let body = commit.body().ok().flatten().unwrap_or("");
        let has_body = if body.trim().is_empty() { 0u8 } else { 1u8 };

        revisions.push(BtRevisionHeader {
            author_index,
            author_time,
            subject: subject_ptr,
            has_body,
        });
    }

    if revisions.is_empty() {
        return 0;
    }

    let revisions_alloc_bytes = revisions.len() * std::mem::size_of::<BtRevisionHeader>();
    let revisions_ptr = unsafe { heap_alloc(revisions_alloc_bytes) } as *mut BtRevisionHeader;
    if revisions_ptr.is_null() {
        for r in &revisions {
            unsafe { heap_free(r.subject as _) };
        }
        set_last_error_str("insufficient memory");
        return 1;
    }

    let mut identities_list = Vec::new();
    for (name, email) in identity_list {
        let name_ptr = unsafe { heap_alloc_c_string(&name) };
        let email_ptr = unsafe { heap_alloc_c_string(&email) };
        identities_list.push(BtIdentity {
            name: name_ptr,
            email: email_ptr,
        });
    }

    let identities_alloc_bytes = identities_list.len() * std::mem::size_of::<BtIdentity>();
    let identities_ptr = unsafe { heap_alloc(identities_alloc_bytes) } as *mut BtIdentity;
    if identities_ptr.is_null() {
        for r in &revisions {
            unsafe { heap_free(r.subject as _) };
        }
        unsafe { heap_free(revisions_ptr as _) };
        for id in identities_list {
            unsafe {
                heap_free(id.name as _);
                heap_free(id.email as _);
            }
        }
        set_last_error_str("insufficient memory");
        return 1;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(revisions.as_ptr(), revisions_ptr, revisions.len());
        core::ptr::copy_nonoverlapping(identities_list.as_ptr(), identities_ptr, identities_list.len());

        (*out_result).revisions = revisions_ptr;
        (*out_result).revisions_len = revisions.len() as i64;
        (*out_result).revisions_cap = revisions.len() as i64;
        (*out_result).identities = identities_ptr;
        (*out_result).identities_len = identities_list.len() as i64;
        (*out_result).identities_cap = identities_list.len() as i64;
    }

    0
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_revision_headers(p: *mut BtRevisionHeaders) {
    if p.is_null() {
        return;
    }
    let revisions_ptr = std::ptr::replace(&mut (*p).revisions, core::ptr::null_mut());
    let revisions_len = (*p).revisions_len;
    let revisions_cap = (*p).revisions_cap;
    (*p).revisions_len = 0;
    (*p).revisions_cap = 0;

    let identities_ptr = std::ptr::replace(&mut (*p).identities, core::ptr::null_mut());
    let identities_len = (*p).identities_len;
    let identities_cap = (*p).identities_cap;
    (*p).identities_len = 0;
    (*p).identities_cap = 0;

    if !revisions_ptr.is_null() {
        for i in 0..revisions_len {
            let r = &mut *revisions_ptr.add(i as usize);
            let r_subject = std::ptr::replace(&mut r.subject, core::ptr::null_mut());
            if !r_subject.is_null() {
                heap_free(r_subject as _);
            }
        }
        if revisions_cap != 0 {
            heap_free(revisions_ptr as _);
        }
    }

    if !identities_ptr.is_null() {
        for i in 0..identities_len {
            let id = &mut *identities_ptr.add(i as usize);
            let id_name = std::ptr::replace(&mut id.name, core::ptr::null_mut());
            if !id_name.is_null() {
                heap_free(id_name as _);
            }
            let id_email = std::ptr::replace(&mut id.email, core::ptr::null_mut());
            if !id_email.is_null() {
                heap_free(id_email as _);
            }
        }
        if identities_cap != 0 {
            heap_free(identities_ptr as _);
        }
    }
}
