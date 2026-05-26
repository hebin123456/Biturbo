use crate::ffi::error::set_last_error_str;
use crate::ffi::types::BtOid;
use crate::ffi::winheap::{heap_alloc, heap_alloc_c_string, heap_free};
use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::Path;

#[repr(C)]
pub struct BtIdentity {
    pub name: *mut c_char,
    pub email: *mut c_char,
}

#[repr(C)]
pub struct BtStash {
    pub reflog_id: i32,
    pub oid: BtOid,
    pub first_parent: BtOid,
    pub author_index: i64,
    pub author_time: i64,
    pub subject: *mut c_char,
}

#[repr(C)]
pub struct BtRepositoryStashes {
    pub stashes: *mut BtStash,
    pub stashes_len: i64,
    pub stashes_cap: i64,
    pub identities: *mut BtIdentity,
    pub identities_len: i64,
    pub identities_cap: i64,
}

#[no_mangle]
pub unsafe extern "C" fn bt_get_repository_stashes(
    _working_dir_path: *const c_char,
    git_dir_path: *const c_char,
    out_result: *mut BtRepositoryStashes,
) -> c_int {
    if git_dir_path.is_null() || out_result.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_result).stashes = core::ptr::null_mut();
        (*out_result).stashes_len = 0;
        (*out_result).stashes_cap = 0;
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

    let reflog = match repo.reflog("refs/stash") {
        Ok(rl) => rl,
        Err(_) => {
            // No stashes is not an error
            return 0;
        }
    };

    let mut stashes_list = Vec::new();
    let mut identity_map = HashMap::new();
    let mut identity_list = Vec::new();

    for (index, entry) in reflog.iter().enumerate() {
        let commit_id = entry.id_new();
        let commit = match repo.find_commit(commit_id) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let bytes = commit_id.as_bytes();
        let oid = BtOid::from_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
            bytes[16], bytes[17], bytes[18], bytes[19]
        ]);

        let first_parent_oid = if commit.parent_count() > 0 {
            if let Ok(pid) = commit.parent_id(0) {
                let p_bytes = pid.as_bytes();
                BtOid::from_bytes([
                    p_bytes[0], p_bytes[1], p_bytes[2], p_bytes[3], p_bytes[4], p_bytes[5], p_bytes[6], p_bytes[7],
                    p_bytes[8], p_bytes[9], p_bytes[10], p_bytes[11], p_bytes[12], p_bytes[13], p_bytes[14], p_bytes[15],
                    p_bytes[16], p_bytes[17], p_bytes[18], p_bytes[19]
                ])
            } else {
                BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 }
            }
        } else {
            BtOid { s0: 0, s1: 0, s2: 0, s3: 0, s4: 0 }
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

        stashes_list.push(BtStash {
            reflog_id: index as i32,
            oid,
            first_parent: first_parent_oid,
            author_index,
            author_time,
            subject: subject_ptr,
        });
    }

    if stashes_list.is_empty() {
        return 0;
    }

    // Allocate stashes on heap
    let stashes_alloc_bytes = stashes_list.len() * std::mem::size_of::<BtStash>();
    let stashes_ptr = unsafe { heap_alloc(stashes_alloc_bytes) } as *mut BtStash;
    if stashes_ptr.is_null() {
        for s in stashes_list {
            unsafe { heap_free(s.subject as _) };
        }
        set_last_error_str("insufficient memory");
        return 1;
    }

    // Allocate identities on heap
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
        for s in stashes_list {
            unsafe { heap_free(s.subject as _) };
        }
        unsafe { heap_free(stashes_ptr as _) };
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
        core::ptr::copy_nonoverlapping(stashes_list.as_ptr(), stashes_ptr, stashes_list.len());
        core::ptr::copy_nonoverlapping(identities_list.as_ptr(), identities_ptr, identities_list.len());

        (*out_result).stashes = stashes_ptr;
        (*out_result).stashes_len = stashes_list.len() as i64;
        (*out_result).stashes_cap = stashes_list.len() as i64;
        (*out_result).identities = identities_ptr;
        (*out_result).identities_len = identities_list.len() as i64;
        (*out_result).identities_cap = identities_list.len() as i64;
    }

    0
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_repository_stashes(p: *mut BtRepositoryStashes) {
    if p.is_null() {
        return;
    }
    let stashes_ptr = std::ptr::replace(&mut (*p).stashes, core::ptr::null_mut());
    let stashes_len = (*p).stashes_len;
    let stashes_cap = (*p).stashes_cap;
    (*p).stashes_len = 0;
    (*p).stashes_cap = 0;

    let identities_ptr = std::ptr::replace(&mut (*p).identities, core::ptr::null_mut());
    let identities_len = (*p).identities_len;
    let identities_cap = (*p).identities_cap;
    (*p).identities_len = 0;
    (*p).identities_cap = 0;

    if !stashes_ptr.is_null() {
        for i in 0..stashes_len {
            let s = &mut *stashes_ptr.add(i as usize);
            let s_subject = std::ptr::replace(&mut s.subject, core::ptr::null_mut());
            if !s_subject.is_null() {
                heap_free(s_subject as _);
            }
        }
        if stashes_cap != 0 {
            heap_free(stashes_ptr as _);
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

#[allow(dead_code)]
pub fn parse_sha_to_btoid(sha: &str) -> Option<BtOid> {
    let b = sha.as_bytes();
    if b.len() != 40 { return None; }
    let mut raw = [0u8; 20];
    for i in 0..20 {
        let hi = hex_nibble(b[i * 2])?;
        let lo = hex_nibble(b[i * 2 + 1])?;
        raw[i] = (hi << 4) | lo;
    }
    Some(BtOid::from_bytes(raw))
}

#[allow(dead_code)]
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
