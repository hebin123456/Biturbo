use crate::ffi::error::set_last_error_str;
use crate::ffi::types::{BtGitConfig, BtGitConfigEntry, BtGitConfigKv};
use crate::ffi::winheap::{heap_alloc, heap_alloc_c_string};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::path::PathBuf;

/// Parse a Git-style configuration file.
#[no_mangle]
pub unsafe extern "C" fn bt_get_git_config(config_path: *const c_char, out_cfg: *mut BtGitConfig) -> c_int {
    if config_path.is_null() || out_cfg.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_cfg).ptr = core::ptr::null_mut();
        (*out_cfg).len = 0;
        (*out_cfg).cap = 0;
    }

    let path_bytes = unsafe { CStr::from_ptr(config_path) }.to_bytes();
    let path_str = match std::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8 config path");
            return 1;
        }
    };

    let path = PathBuf::from(path_str);
    let content = match std::fs::read(&path) {
        Ok(c) => c,
        Err(e) => {
            set_last_error_str(&format!("open git config at '{}': {e}", path.display()));
            return 1;
        }
    };

    let entries = match parse_config_content(&content) {
        Ok(e) => e,
        Err(msg) => {
            set_last_error_str(&format!("parse config: {msg}"));
            return 1;
        }
    };

    if entries.is_empty() {
        return 0;
    }

    // Allocate memory using process heap
    let entry_bytes = entries.len() * std::mem::size_of::<BtGitConfigEntry>();
    let entry_ptr = unsafe { heap_alloc(entry_bytes) } as *mut BtGitConfigEntry;
    if entry_ptr.is_null() {
        set_last_error_str("insufficient memory");
        return 1;
    }

    for (i, entry) in entries.iter().enumerate() {
        let entry_dst = unsafe { &mut *entry_ptr.add(i) };
        entry_dst.a = unsafe { heap_alloc_c_string(&entry.section) };
        entry_dst.b = unsafe { heap_alloc_c_string(&entry.subsection) };

        if entry.kvs.is_empty() {
            entry_dst.kv_ptr = core::ptr::null_mut();
            entry_dst.kv_len = 0;
            entry_dst.kv_cap = 0;
        } else {
            let kv_bytes = entry.kvs.len() * std::mem::size_of::<BtGitConfigKv>();
            let kv_ptr = unsafe { heap_alloc(kv_bytes) } as *mut BtGitConfigKv;
            if kv_ptr.is_null() {
                // Free already allocated parts of this and previous entries
                // to avoid memory leaks
                for prev_idx in 0..=i {
                    let prev = unsafe { &mut *entry_ptr.add(prev_idx) };
                    if !prev.a.is_null() { crate::ffi::winheap::heap_free(prev.a as _); }
                    if !prev.b.is_null() { crate::ffi::winheap::heap_free(prev.b as _); }
                    if !prev.kv_ptr.is_null() {
                        for kv_idx in 0..prev.kv_len {
                            let kv = unsafe { &mut *prev.kv_ptr.add(kv_idx) };
                            if !kv.k.is_null() { crate::ffi::winheap::heap_free(kv.k as _); }
                            if !kv.v.is_null() { crate::ffi::winheap::heap_free(kv.v as _); }
                        }
                        if prev.kv_cap != 0 {
                            crate::ffi::winheap::heap_free(prev.kv_ptr as _);
                        }
                    }
                }
                crate::ffi::winheap::heap_free(entry_ptr as _);
                set_last_error_str("insufficient memory");
                return 1;
            }

            for (j, kv) in entry.kvs.iter().enumerate() {
                let kv_dst = unsafe { &mut *kv_ptr.add(j) };
                kv_dst.k = unsafe { heap_alloc_c_string(&kv.k) };
                kv_dst.v = unsafe { heap_alloc_c_string(&kv.v) };
            }

            entry_dst.kv_ptr = kv_ptr;
            entry_dst.kv_len = entry.kvs.len();
            entry_dst.kv_cap = entry.kvs.len();
        }
    }

    unsafe {
        (*out_cfg).ptr = entry_ptr;
        (*out_cfg).len = entries.len();
        (*out_cfg).cap = entries.len();
    }

    0
}

struct ParsedEntry {
    section: String,
    subsection: String,
    kvs: Vec<ParsedKv>,
}

struct ParsedKv {
    k: String,
    v: String,
}

fn parse_config_content(content: &[u8]) -> Result<Vec<ParsedEntry>, String> {
    let mut entries: Vec<ParsedEntry> = Vec::new();
    let text = std::str::from_utf8(content).map_err(|e| format!("non-utf8 config: {e}"))?;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section_content = &trimmed[1..trimmed.len() - 1].trim();
            // Handle subsection e.g. [remote "origin"]
            let mut parts = section_content.splitn(2, |c: char| c.is_whitespace());
            let section = parts.next().unwrap_or("").trim().to_string();
            let mut subsection = String::new();
            if let Some(sub) = parts.next() {
                let sub_trimmed = sub.trim();
                if sub_trimmed.starts_with('"') && sub_trimmed.ends_with('"') {
                    subsection = sub_trimmed[1..sub_trimmed.len() - 1].to_string();
                } else {
                    subsection = sub_trimmed.to_string();
                }
            }

            entries.push(ParsedEntry {
                section,
                subsection,
                kvs: Vec::new(),
            });
        } else if let Some(idx) = trimmed.find('=') {
            let k = trimmed[..idx].trim().to_string();
            let v = trimmed[idx + 1..].trim().to_string();

            if let Some(current) = entries.last_mut() {
                current.kvs.push(ParsedKv { k, v });
            }
        }
    }

    Ok(entries)
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_git_config(cfg: *mut BtGitConfig) {
    if cfg.is_null() {
        return;
    }
    let ptr = std::ptr::replace(&mut (*cfg).ptr, core::ptr::null_mut());
    let len = (*cfg).len;
    let cap = (*cfg).cap;
    (*cfg).len = 0;
    (*cfg).cap = 0;

    if !ptr.is_null() {
        for i in 0..len {
            let entry = &mut *ptr.add(i);
            let entry_a = std::ptr::replace(&mut entry.a, core::ptr::null_mut());
            if !entry_a.is_null() {
                *entry_a = 0; // poison
                crate::ffi::winheap::heap_free(entry_a as *mut c_void);
            }
            let entry_b = std::ptr::replace(&mut entry.b, core::ptr::null_mut());
            if !entry_b.is_null() {
                *entry_b = 0; // poison
                crate::ffi::winheap::heap_free(entry_b as *mut c_void);
            }
            let kv_ptr = std::ptr::replace(&mut entry.kv_ptr, core::ptr::null_mut());
            let kv_len = entry.kv_len;
            let kv_cap = entry.kv_cap;
            entry.kv_len = 0;
            entry.kv_cap = 0;
            if !kv_ptr.is_null() {
                for j in 0..kv_len {
                    let kv = &mut *kv_ptr.add(j);
                    let kv_k = std::ptr::replace(&mut kv.k, core::ptr::null_mut());
                    if !kv_k.is_null() {
                        *kv_k = 0; // poison
                        crate::ffi::winheap::heap_free(kv_k as *mut c_void);
                    }
                    let kv_v = std::ptr::replace(&mut kv.v, core::ptr::null_mut());
                    if !kv_v.is_null() {
                        *kv_v = 0; // poison
                        crate::ffi::winheap::heap_free(kv_v as *mut c_void);
                    }
                }
                if kv_cap != 0 {
                    crate::ffi::winheap::heap_free(kv_ptr as *mut c_void);
                }
            }
        }
        if cap != 0 {
            crate::ffi::winheap::heap_free(ptr as *mut c_void);
        }
    }
}
