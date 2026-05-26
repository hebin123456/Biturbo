use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::{heap_alloc, heap_alloc_c_string, heap_free};
use serde::{Deserialize, Serialize};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;

#[repr(C)]
pub struct BtRepositoryManagerRepository {
    pub path: *mut c_char,
    pub alias: *mut c_char,
    pub opened: u32,
    pub color: u8,
}

#[repr(C)]
pub struct BtRepositoryManager {
    pub source_dirs: *mut *mut c_char,
    pub source_dirs_len: i64,
    pub source_dirs_cap: i64,
    pub scan_depth: u8,
    pub ignore: *mut *mut c_char,
    pub ignore_len: i64,
    pub ignore_cap: i64,
    pub repositories: *mut BtRepositoryManagerRepository,
    pub repositories_len: i64,
    pub repositories_cap: i64,
}

#[derive(Serialize, Deserialize)]
struct TomlRepo {
    path: String,
    #[serde(default)]
    alias: String,
    #[serde(default)]
    opened: u32,
    #[serde(default)]
    color: u8,
}

#[derive(Serialize, Deserialize)]
struct TomlConfig {
    source_dirs: Vec<String>,
    scan_depth: u8,
    ignore: Vec<String>,
    repositories: Vec<TomlRepo>,
}

#[no_mangle]
pub unsafe extern "C" fn bt_get_repository_manager(
    path: *const c_char,
    out_result: *mut BtRepositoryManager,
) -> c_int {
    if path.is_null() || out_result.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_result).source_dirs = core::ptr::null_mut();
        (*out_result).source_dirs_len = 0;
        (*out_result).source_dirs_cap = 0;
        (*out_result).scan_depth = 5;
        (*out_result).ignore = core::ptr::null_mut();
        (*out_result).ignore_len = 0;
        (*out_result).ignore_cap = 0;
        (*out_result).repositories = core::ptr::null_mut();
        (*out_result).repositories_len = 0;
        (*out_result).repositories_cap = 0;
    }

    let path_bytes = unsafe { CStr::from_ptr(path) }.to_bytes();
    let path_str = match std::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8 repositories path");
            return 1;
        }
    };

    let filepath = PathBuf::from(path_str);
    if !filepath.exists() {
        // File does not exist yet: return empty but success
        return 0;
    }

    let content = match std::fs::read_to_string(&filepath) {
        Ok(c) => c,
        Err(e) => {
            set_last_error_str(&format!("failed to read '{}': {e}", filepath.display()));
            return 1;
        }
    };

    let config: TomlConfig = match toml::from_str(&content) {
        Ok(c) => c,
        Err(e) => {
            set_last_error_str(&format!("failed to parse TOML config: {e}"));
            return 1;
        }
    };

    // 1. Allocate source_dirs
    let mut source_dirs_ptrs = Vec::new();
    for s in &config.source_dirs {
        let p = unsafe { heap_alloc_c_string(s) };
        if p.is_null() {
            set_last_error_str("insufficient memory");
            // Clean up already allocated
            for ptr in source_dirs_ptrs {
                unsafe { heap_free(ptr as _) };
            }
            return 1;
        }
        source_dirs_ptrs.push(p);
    }

    let source_dirs_len = source_dirs_ptrs.len() as i64;
    let source_dirs_ptr = if source_dirs_len > 0 {
        let alloc_bytes = source_dirs_ptrs.len() * std::mem::size_of::<*mut c_char>();
        let p = unsafe { heap_alloc(alloc_bytes) } as *mut *mut c_char;
        if p.is_null() {
            set_last_error_str("insufficient memory");
            for ptr in source_dirs_ptrs {
                unsafe { heap_free(ptr as _) };
            }
            return 1;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(source_dirs_ptrs.as_ptr(), p, source_dirs_ptrs.len());
        }
        p
    } else {
        core::ptr::null_mut()
    };

    // 2. Allocate ignore
    let mut ignore_ptrs = Vec::new();
    for s in &config.ignore {
        let p = unsafe { heap_alloc_c_string(s) };
        if p.is_null() {
            set_last_error_str("insufficient memory");
            for ptr in ignore_ptrs {
                unsafe { heap_free(ptr as _) };
            }
            if !source_dirs_ptr.is_null() {
                for ptr in source_dirs_ptrs {
                    unsafe { heap_free(ptr as _) };
                }
                unsafe { heap_free(source_dirs_ptr as _) };
            }
            return 1;
        }
        ignore_ptrs.push(p);
    }

    let ignore_len = ignore_ptrs.len() as i64;
    let ignore_ptr = if ignore_len > 0 {
        let alloc_bytes = ignore_ptrs.len() * std::mem::size_of::<*mut c_char>();
        let p = unsafe { heap_alloc(alloc_bytes) } as *mut *mut c_char;
        if p.is_null() {
            set_last_error_str("insufficient memory");
            for ptr in ignore_ptrs {
                unsafe { heap_free(ptr as _) };
            }
            for ptr in source_dirs_ptrs {
                unsafe { heap_free(ptr as _) };
            }
            unsafe { heap_free(source_dirs_ptr as _) };
            return 1;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(ignore_ptrs.as_ptr(), p, ignore_ptrs.len());
        }
        p
    } else {
        core::ptr::null_mut()
    };

    // 3. Allocate repositories
    let mut repos: Vec<BtRepositoryManagerRepository> = Vec::new();
    for repo in &config.repositories {
        let path_p = unsafe { heap_alloc_c_string(&repo.path) };
        let alias_p = if repo.alias.is_empty() {
            core::ptr::null_mut()
        } else {
            unsafe { heap_alloc_c_string(&repo.alias) }
        };

        if path_p.is_null() {
            set_last_error_str("insufficient memory");
            // Free all allocations
            for r in repos {
                if !r.path.is_null() { unsafe { heap_free(r.path as _) }; }
                if !r.alias.is_null() { unsafe { heap_free(r.alias as _) }; }
            }
            // Free other lists...
            for ptr in ignore_ptrs { unsafe { heap_free(ptr as _) }; }
            if !ignore_ptr.is_null() { unsafe { heap_free(ignore_ptr as _) }; }
            for ptr in source_dirs_ptrs { unsafe { heap_free(ptr as _) }; }
            if !source_dirs_ptr.is_null() { unsafe { heap_free(source_dirs_ptr as _) }; }
            return 1;
        }

        repos.push(BtRepositoryManagerRepository {
            path: path_p,
            alias: alias_p,
            opened: repo.opened,
            color: repo.color,
        });
    }

    let repositories_len = repos.len() as i64;
    let repositories_ptr = if repositories_len > 0 {
        let alloc_bytes = repos.len() * std::mem::size_of::<BtRepositoryManagerRepository>();
        let p = unsafe { heap_alloc(alloc_bytes) } as *mut BtRepositoryManagerRepository;
        if p.is_null() {
            set_last_error_str("insufficient memory");
            for r in repos {
                if !r.path.is_null() { unsafe { heap_free(r.path as _) }; }
                if !r.alias.is_null() { unsafe { heap_free(r.alias as _) }; }
            }
            for ptr in ignore_ptrs { unsafe { heap_free(ptr as _) }; }
            if !ignore_ptr.is_null() { unsafe { heap_free(ignore_ptr as _) }; }
            for ptr in source_dirs_ptrs { unsafe { heap_free(ptr as _) }; }
            if !source_dirs_ptr.is_null() { unsafe { heap_free(source_dirs_ptr as _) }; }
            return 1;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(repos.as_ptr(), p, repos.len());
        }
        p
    } else {
        core::ptr::null_mut()
    };

    unsafe {
        (*out_result).source_dirs = source_dirs_ptr;
        (*out_result).source_dirs_len = source_dirs_len;
        (*out_result).source_dirs_cap = source_dirs_len;
        (*out_result).scan_depth = config.scan_depth;
        (*out_result).ignore = ignore_ptr;
        (*out_result).ignore_len = ignore_len;
        (*out_result).ignore_cap = ignore_len;
        (*out_result).repositories = repositories_ptr;
        (*out_result).repositories_len = repositories_len;
        (*out_result).repositories_cap = repositories_len;
    }

    0
}

#[no_mangle]
pub unsafe extern "C" fn bt_save_repository_manager(
    path: *const c_char,
    source_dirs_ptr: *const *const c_char,
    source_dirs_len: i64,
    scan_depth: u8,
    ignore_ptr: *const *const c_char,
    ignore_len: i64,
    paths_ptr: *const *const c_char,
    paths_len: i64,
    aliases_ptr: *const *const c_char,
    aliases_len: i64,
    opened_ptr: *const u32,
    opened_len: i64,
    colors_ptr: *const u8,
    colors_len: i64,
) -> c_int {
    if path.is_null() {
        set_last_error_str("invalid input path");
        return 1;
    }

    let path_bytes = unsafe { CStr::from_ptr(path) }.to_bytes();
    let path_str = match std::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8 path");
            return 1;
        }
    };
    let filepath = PathBuf::from(path_str);

    // Build source_dirs list
    let mut source_dirs = Vec::new();
    if !source_dirs_ptr.is_null() && source_dirs_len > 0 {
        for i in 0..source_dirs_len {
            let p = *source_dirs_ptr.add(i as usize);
            if !p.is_null() {
                if let Ok(s) = CStr::from_ptr(p).to_str() {
                    source_dirs.push(s.to_string());
                }
            }
        }
    }

    // Build ignore list
    let mut ignore = Vec::new();
    if !ignore_ptr.is_null() && ignore_len > 0 {
        for i in 0..ignore_len {
            let p = *ignore_ptr.add(i as usize);
            if !p.is_null() {
                if let Ok(s) = CStr::from_ptr(p).to_str() {
                    ignore.push(s.to_string());
                }
            }
        }
    }

    // Build repositories list
    let mut repositories = Vec::new();
    if !paths_ptr.is_null() && paths_len > 0 {
        for i in 0..paths_len {
            let p_ptr = *paths_ptr.add(i as usize);
            if p_ptr.is_null() { continue; }
            let p_str = match CStr::from_ptr(p_ptr).to_str() {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };

            let alias_str = if !aliases_ptr.is_null() && i < aliases_len {
                let a_ptr = *aliases_ptr.add(i as usize);
                if a_ptr.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(a_ptr).to_str().unwrap_or("").to_string()
                }
            } else {
                String::new()
            };

            let opened = if !opened_ptr.is_null() && i < opened_len {
                *opened_ptr.add(i as usize)
            } else {
                0
            };

            let color = if !colors_ptr.is_null() && i < colors_len {
                *colors_ptr.add(i as usize)
            } else {
                0
            };

            repositories.push(TomlRepo {
                path: p_str,
                alias: alias_str,
                opened,
                color,
            });
        }
    }

    let config = TomlConfig {
        source_dirs,
        scan_depth,
        ignore,
        repositories,
    };

    let serialized = match toml::to_string(&config) {
        Ok(s) => s,
        Err(e) => {
            set_last_error_str(&format!("failed to serialize TOML: {e}"));
            return 1;
        }
    };

    if let Err(e) = std::fs::write(&filepath, serialized) {
        set_last_error_str(&format!("failed to write TOML to '{}': {e}", filepath.display()));
        return 1;
    }

    0
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_repository_manager(p: *mut BtRepositoryManager) {
    if p.is_null() {
        return;
    }

    let source_dirs_ptr = std::ptr::replace(&mut (*p).source_dirs, core::ptr::null_mut());
    let source_dirs_len = (*p).source_dirs_len;
    (*p).source_dirs_len = 0;
    (*p).source_dirs_cap = 0;
    if !source_dirs_ptr.is_null() {
        for i in 0..source_dirs_len {
            let ptr = std::ptr::replace(&mut *source_dirs_ptr.add(i as usize), core::ptr::null_mut());
            if !ptr.is_null() {
                heap_free(ptr as _);
            }
        }
        heap_free(source_dirs_ptr as _);
    }

    let ignore_ptr = std::ptr::replace(&mut (*p).ignore, core::ptr::null_mut());
    let ignore_len = (*p).ignore_len;
    (*p).ignore_len = 0;
    (*p).ignore_cap = 0;
    if !ignore_ptr.is_null() {
        for i in 0..ignore_len {
            let ptr = std::ptr::replace(&mut *ignore_ptr.add(i as usize), core::ptr::null_mut());
            if !ptr.is_null() {
                heap_free(ptr as _);
            }
        }
        heap_free(ignore_ptr as _);
    }

    let repositories_ptr = std::ptr::replace(&mut (*p).repositories, core::ptr::null_mut());
    let repositories_len = (*p).repositories_len;
    (*p).repositories_len = 0;
    (*p).repositories_cap = 0;
    if !repositories_ptr.is_null() {
        for i in 0..repositories_len {
            let r = &mut *repositories_ptr.add(i as usize);
            let r_path = std::ptr::replace(&mut r.path, core::ptr::null_mut());
            if !r_path.is_null() {
                heap_free(r_path as _);
            }
            let r_alias = std::ptr::replace(&mut r.alias, core::ptr::null_mut());
            if !r_alias.is_null() {
                heap_free(r_alias as _);
            }
        }
        heap_free(repositories_ptr as _);
    }
}
