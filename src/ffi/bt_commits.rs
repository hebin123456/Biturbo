use crate::ffi::error::set_last_error_str;
use crate::ffi::types::BtOid;
use crate::ffi::winheap::{heap_alloc, heap_free};
use core::ffi::c_void;
use std::collections::{HashMap, HashSet};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::sync::Mutex;

const BT_OK: c_int = 0;
const BT_ERR: c_int = 1;
const BT_ERR_CANCELED: c_int = 2;

#[repr(C)]
pub struct BtCommitStorage {
    pub oids: *mut BtOid,
    pub oids_len: i64,
    pub oids_cap: i64,
    pub indexes: *mut u32,
    pub indexes_len: i64,
    pub indexes_cap: i64,
    pub has_more: u8,
}

#[repr(C)]
pub struct BtOidPair {
    pub left: BtOid,
    pub right: BtOid,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BtBehindAheadCount {
    pub left: u32,
    pub right: u32,
}

#[repr(C)]
pub struct BtBehindAheadCounts {
    pub items: *mut BtBehindAheadCount,
    pub items_len: i64,
    pub items_cap: i64,
}

#[repr(C)]
pub struct BtSearchCommitsResult {
    pub matches: *mut BtOid,
    pub matches_len: i64,
    pub matches_cap: i64,
}

struct CommitEdges {
    parents: Vec<BtOid>,
    author_time: i64,
}

struct Cache {
    commit_edges_cache: Mutex<HashMap<BtOid, CommitEdges>>,
}

fn cstr_to_utf8<'a>(p: *const c_char, field: &'static str) -> Result<&'a str, c_int> {
    if p.is_null() {
        set_last_error_str(&format!("{field} is null"));
        return Err(BT_ERR);
    }
    let bytes = unsafe { CStr::from_ptr(p) }.to_bytes();
    std::str::from_utf8(bytes).map_err(|_| {
        set_last_error_str(&format!("{field} is not valid UTF-8"));
        BT_ERR
    })
}

fn is_canceled(cancellation_token_ptr: *mut *mut u8) -> bool {
    crate::ffi::bt_cancellation::is_token_active_and_canceled(cancellation_token_ptr)
}

unsafe fn alloc_and_copy_slice<T: Copy>(values: &[T]) -> *mut T {
    unsafe { alloc_and_copy_slice_with_cap(values, values.len()).0 }
}

unsafe fn alloc_and_copy_slice_with_cap<T: Copy>(values: &[T], cap: usize) -> (*mut T, usize) {
    if values.is_empty() {
        return (core::ptr::null_mut(), 0);
    }
    let cap = cap.max(values.len());
    let bytes = cap * std::mem::size_of::<T>();
    let p = unsafe { heap_alloc(bytes) } as *mut T;
    if p.is_null() {
        return (core::ptr::null_mut(), 0);
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            values.as_ptr() as *const u8,
            p as *mut u8,
            values.len() * std::mem::size_of::<T>(),
        );
    }
    (p, cap)
}

fn next_legacy_capacity(len: usize) -> usize {
    if len == 0 {
        0
    } else {
        len.next_power_of_two()
    }
}

fn git_oid_to_btoid(oid: git2::Oid) -> BtOid {
    let bytes = oid.as_bytes();
    BtOid::from_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16], bytes[17], bytes[18], bytes[19],
    ])
}

fn btoid_to_hex(oid: &BtOid) -> String {
    format!("{:08x}{:08x}{:08x}{:08x}{:08x}", oid.s0, oid.s1, oid.s2, oid.s3, oid.s4)
}

#[no_mangle]
pub unsafe extern "C" fn bt_new_commit_graph_cache(identifier: *const c_char) -> *mut c_void {
    let _ident = match cstr_to_utf8(identifier, "bt_new_commit_graph_cache: identifier") {
        Ok(s) => s.to_string(),
        Err(_) => String::new(),
    };
    let boxed = Box::new(Cache {
        commit_edges_cache: Mutex::new(HashMap::new()),
    });
    Box::into_raw(boxed) as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_commit_graph_cache(cache: *mut *mut c_void) {
    if cache.is_null() {
        return;
    }
    unsafe {
        let inner = std::ptr::replace(cache, core::ptr::null_mut());
        if !inner.is_null() {
            drop(Box::from_raw(inner as *mut Cache));
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_commit_storage(r: *mut BtCommitStorage) {
    if r.is_null() {
        return;
    }
    unsafe {
        let oids_ptr = (*r).oids;
        if !oids_ptr.is_null() {
            heap_free(oids_ptr as *mut c_void);
        }

        let indexes_ptr = (*r).indexes;
        if !indexes_ptr.is_null() {
            heap_free(indexes_ptr as *mut c_void);
        }
    }
}

fn get_commit_edges(
    repo: &git2::Repository,
    oid: BtOid,
    cache: &Mutex<HashMap<BtOid, CommitEdges>>,
) -> Option<CommitEdges> {
    {
        let lock = cache.lock().unwrap();
        if let Some(edges) = lock.get(&oid) {
            return Some(CommitEdges {
                parents: edges.parents.clone(),
                author_time: edges.author_time,
            });
        }
    }

    let raw_oid = oid.to_bytes();
    let git2_oid = git2::Oid::from_bytes(&raw_oid).ok()?;
    let commit = repo.find_commit(git2_oid).ok()?;

    let mut parents = Vec::new();
    for i in 0..commit.parent_count() {
        if let Ok(pid) = commit.parent_id(i) {
            let p_bytes = pid.as_bytes();
            parents.push(BtOid::from_bytes([
                p_bytes[0], p_bytes[1], p_bytes[2], p_bytes[3], p_bytes[4], p_bytes[5], p_bytes[6], p_bytes[7],
                p_bytes[8], p_bytes[9], p_bytes[10], p_bytes[11], p_bytes[12], p_bytes[13], p_bytes[14], p_bytes[15],
                p_bytes[16], p_bytes[17], p_bytes[18], p_bytes[19]
            ]));
        }
    }

    let author_time = commit.author().when().seconds();
    let edges = CommitEdges { parents, author_time };
    
    let mut lock = cache.lock().unwrap();
    lock.insert(oid, CommitEdges {
        parents: edges.parents.clone(),
        author_time: edges.author_time,
    });
    Some(edges)
}

struct CommitRecord {
    oid: BtOid,
    parents: Vec<BtOid>,
}

fn build_commit_storage_native(
    repo: &git2::Repository,
    tips: &[BtOid],
    date_order: bool,
    skip_count: i64,
    max_count: i64,
    cache: &Mutex<HashMap<BtOid, CommitEdges>>,
    cancellation_token_ptr: *mut *mut u8,
    flat: &mut Vec<BtOid>,
    indexes: &mut Vec<u32>,
    hit_limit: &mut bool,
) -> bool {
    let mut stack = tips.to_vec();
    let mut seen = HashSet::new();
    let mut records = Vec::new();
    *hit_limit = false;

    while let Some(oid) = stack.pop() {
        if is_canceled(cancellation_token_ptr) {
            set_last_error_str("Canceled");
            return false;
        }
        if seen.contains(&oid) {
            continue;
        }
        let Some(edges) = get_commit_edges(repo, oid, cache) else {
            continue;
        };
        seen.insert(oid);
        records.push(CommitRecord {
            oid,
            parents: edges.parents.clone(),
        });

        // PUSH PARENTS IN REVERSE ORDER TO MATCH STACK LIFO (visiting first parent first!)
        for &parent in edges.parents.iter().rev() {
            if !seen.contains(&parent) {
                stack.push(parent);
            }
        }

        if !date_order && skip_count == 0 && max_count > 0 && (records.len() as i64) >= max_count {
            *hit_limit = true;
            break;
        }
    }

    let start = std::cmp::max(0, skip_count) as usize;
    let mut end = records.len();
    if max_count > 0 && start < end {
        end = std::cmp::min(end, start + max_count as usize);
    }
    *hit_limit = end < records.len() || (max_count > 0 && end >= start && (end - start) >= max_count as usize);

    for i in start..end {
        indexes.push(flat.len() as u32);
        flat.push(records[i].oid);
        for &parent in &records[i].parents {
            flat.push(parent);
        }
    }
    true
}

fn build_commit_storage_priority_native(
    repo: &git2::Repository,
    tips: &[BtOid],
    skip_count: i64,
    max_count: i64,
    cache: &Mutex<HashMap<BtOid, CommitEdges>>,
    cancellation_token_ptr: *mut *mut u8,
    flat: &mut Vec<BtOid>,
    indexes: &mut Vec<u32>,
    hit_limit: &mut bool,
) -> bool {
    *hit_limit = false;
    let mut walk = match repo.revwalk() {
        Ok(walk) => walk,
        Err(e) => {
            set_last_error_str(&format!("failed to create revwalk: {e}"));
            return false;
        }
    };
    let _ = walk.set_sorting(git2::Sort::TOPOLOGICAL);
    for &tip in tips {
        let raw_oid = tip.to_bytes();
        if let Ok(git_oid) = git2::Oid::from_bytes(&raw_oid) {
            let _ = walk.push(git_oid);
        }
    }

    let mut emitted_count = 0usize;
    let start = std::cmp::max(0, skip_count) as usize;
    let end_limit = if max_count > 0 {
        start + max_count as usize
    } else {
        usize::MAX
    };

    for oid_result in walk {
        if is_canceled(cancellation_token_ptr) {
            set_last_error_str("Canceled");
            return false;
        }
        if emitted_count >= end_limit {
            *hit_limit = true;
            break;
        }
        let oid = match oid_result {
            Ok(oid) => oid,
            Err(_) => continue,
        };
        let commit = match repo.find_commit(oid) {
            Ok(commit) => commit,
            Err(_) => continue,
        };
        if emitted_count >= start {
            let btoid = git_oid_to_btoid(oid);
            indexes.push(flat.len() as u32);
            flat.push(btoid);
            let mut parents = Vec::with_capacity(commit.parent_count());
            for i in 0..commit.parent_count() {
                if let Ok(parent_id) = commit.parent_id(i) {
                    let parent = git_oid_to_btoid(parent_id);
                    flat.push(parent);
                    parents.push(parent);
                }
            }
            if let Ok(mut lock) = cache.lock() {
                lock.entry(btoid).or_insert_with(|| CommitEdges {
                    parents,
                    author_time: commit.author().when().seconds(),
                });
            }
        }
        emitted_count += 1;
    }

    true
}

#[no_mangle]
pub unsafe extern "C" fn bt_get_commits(
    git_dir_path: *const c_char,
    tips_ptr: *const BtOid,
    tips_len: i64,
    date_order: u8,
    page_size: i64,
    skip_pages: i64,
    min_pages: i64,
    required_oids_ptr: *const BtOid,
    required_oids_len: i64,
    cache_handle: *mut *mut c_void,
    cancellation_token_ptr: *mut *mut u8,
    out_result: *mut BtCommitStorage,
) -> c_int {
    if out_result.is_null() {
        set_last_error_str("bt_get_commits: out_result is null");
        return BT_ERR;
    }

    unsafe {
        (*out_result).oids = core::ptr::null_mut();
        (*out_result).oids_len = 0;
        (*out_result).oids_cap = 0;
        (*out_result).indexes = core::ptr::null_mut();
        (*out_result).indexes_len = 0;
        (*out_result).indexes_cap = 0;
        (*out_result).has_more = 0;
    }

    if is_canceled(cancellation_token_ptr) {
        set_last_error_str("Canceled");
        return BT_ERR_CANCELED;
    }

    let git_dir_str = match cstr_to_utf8(git_dir_path, "bt_get_commits: git_dir_path") {
        Ok(s) => s,
        Err(rc) => return rc,
    };
    let git_dir = Path::new(git_dir_str);

    let repo = match git2::Repository::open(git_dir) {
        Ok(r) => r,
        Err(e) => {
            set_last_error_str(&format!("failed to open repository: {e}"));
            return BT_ERR;
        }
    };

    if (tips_ptr.is_null() || tips_len <= 0) && (required_oids_ptr.is_null() || required_oids_len <= 0) {
        set_last_error_str("bt_get_commits: tips are empty");
        return BT_ERR;
    }

    let tips = if tips_ptr.is_null() || tips_len <= 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(tips_ptr, tips_len as usize) }
    };
    let required = if required_oids_ptr.is_null() || required_oids_len <= 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(required_oids_ptr, required_oids_len as usize) }
    };

    let has_required = !required.is_empty();
    let combined_tips = if tips.is_empty() {
        required.to_vec()
    } else {
        tips.to_vec()
    };

    let mut max_count = page_size.saturating_mul(std::cmp::max(1, min_pages));
    if has_required {
        max_count = 0;
    } else if max_count <= 0 {
        max_count = 10000;
    }
    let skip_count = if page_size > 0 && skip_pages > 0 {
        page_size.saturating_mul(skip_pages)
    } else {
        0
    };

    // Grab or fall back to a local temporary cache
    let local_cache = Mutex::new(HashMap::new());
    let cache = if !cache_handle.is_null() && !unsafe { *cache_handle }.is_null() {
        unsafe { &(*(*cache_handle as *const Cache)).commit_edges_cache }
    } else {
        &local_cache
    };

    let mut flat = Vec::new();
    let mut indexes = Vec::new();
    let mut hit_limit = false;

    let success = if combined_tips.len() <= 1 && !has_required {
        build_commit_storage_native(
            &repo,
            &combined_tips,
            date_order != 0,
            skip_count,
            max_count,
            cache,
            cancellation_token_ptr,
            &mut flat,
            &mut indexes,
            &mut hit_limit,
        )
    } else {
        build_commit_storage_priority_native(
            &repo,
            &combined_tips,
            skip_count,
            max_count,
            cache,
            cancellation_token_ptr,
            &mut flat,
            &mut indexes,
            &mut hit_limit,
        )
    };

    if !success {
        return BT_ERR;
    }

    let (oids_cap_target, indexes_cap_target) = if has_required {
        (next_legacy_capacity(flat.len()), next_legacy_capacity(indexes.len()))
    } else if combined_tips.len() > 1 {
        let page_cap = usize::try_from(max_count).unwrap_or(0);
        (page_cap.max(flat.len()), page_cap.max(indexes.len()))
    } else {
        (flat.len(), indexes.len())
    };

    let (oids_ptr, oids_cap) = unsafe { alloc_and_copy_slice_with_cap(&flat, oids_cap_target) };
    if !flat.is_empty() && oids_ptr.is_null() {
        set_last_error_str("insufficient memory");
        return BT_ERR;
    }
    let (indexes_ptr, indexes_cap) = unsafe { alloc_and_copy_slice_with_cap(&indexes, indexes_cap_target) };
    if !indexes.is_empty() && indexes_ptr.is_null() {
        if !oids_ptr.is_null() {
            unsafe { heap_free(oids_ptr as *mut c_void) };
        }
        set_last_error_str("insufficient memory");
        return BT_ERR;
    }

    unsafe {
        (*out_result).oids = oids_ptr;
        (*out_result).oids_len = flat.len() as i64;
        (*out_result).oids_cap = oids_cap as i64;
        (*out_result).indexes = indexes_ptr;
        (*out_result).indexes_len = indexes.len() as i64;
        (*out_result).indexes_cap = indexes_cap as i64;
        (*out_result).has_more = if hit_limit { 1 } else { 0 };
    }

    BT_OK
}

#[no_mangle]
pub unsafe extern "C" fn bt_get_commit_subgraph(
    git_dir_path: *const c_char,
    oid: *const BtOid,
    cache: *mut *mut c_void,
    out_result: *mut BtCommitStorage,
) -> c_int {
    if oid.is_null() {
        set_last_error_str("bt_get_commit_subgraph: oid is null");
        return BT_ERR;
    }
    let tip = unsafe { *oid };
    get_commit_subgraph_date_order(git_dir_path, &[tip], None, cache, out_result)
}

#[no_mangle]
pub unsafe extern "C" fn bt_get_commit_subgraph_2(
    git_dir_path: *const c_char,
    src: *const BtOid,
    dst: *const BtOid,
    cache: *mut *mut c_void,
    out_result: *mut BtCommitStorage,
) -> c_int {
    if src.is_null() || dst.is_null() {
        set_last_error_str("bt_get_commit_subgraph_2: invalid arguments");
        return BT_ERR;
    }
    let src_oid = unsafe { *src };
    let dst_oid = unsafe { *dst };
    get_commit_subgraph_date_order(git_dir_path, &[dst_oid], Some(src_oid), cache, out_result)
}

fn get_commit_subgraph_date_order(
    git_dir_path: *const c_char,
    tips: &[BtOid],
    stop_after: Option<BtOid>,
    cache_handle: *mut *mut c_void,
    out_result: *mut BtCommitStorage,
) -> c_int {
    if out_result.is_null() {
        set_last_error_str("bt_get_commit_subgraph: out_result is null");
        return BT_ERR;
    }
    unsafe {
        (*out_result).oids = core::ptr::null_mut();
        (*out_result).oids_len = 0;
        (*out_result).oids_cap = 0;
        (*out_result).indexes = core::ptr::null_mut();
        (*out_result).indexes_len = 0;
        (*out_result).indexes_cap = 0;
        (*out_result).has_more = 0;
    }

    let git_dir_str = match cstr_to_utf8(git_dir_path, "bt_get_commit_subgraph: git_dir_path") {
        Ok(s) => s,
        Err(rc) => return rc,
    };
    let repo = match git2::Repository::open(Path::new(git_dir_str)) {
        Ok(r) => r,
        Err(e) => {
            set_last_error_str(&format!("failed to open repository: {e}"));
            return BT_ERR;
        }
    };
    let local_cache = Mutex::new(HashMap::new());
    let cache = if !cache_handle.is_null() && !unsafe { *cache_handle }.is_null() {
        unsafe { &(*(*cache_handle as *const Cache)).commit_edges_cache }
    } else {
        &local_cache
    };

    let mut walk = match repo.revwalk() {
        Ok(walk) => walk,
        Err(e) => {
            set_last_error_str(&format!("failed to create revwalk: {e}"));
            return BT_ERR;
        }
    };
    let _ = walk.set_sorting(git2::Sort::TIME);
    for tip in tips {
        if let Ok(git_oid) = git2::Oid::from_bytes(&tip.to_bytes()) {
            let _ = walk.push(git_oid);
        }
    }

    let mut flat = Vec::new();
    let mut indexes = Vec::new();
    for oid_result in walk {
        let Ok(oid) = oid_result else {
            continue;
        };
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        let btoid = git_oid_to_btoid(oid);
        let mut parents = Vec::with_capacity(commit.parent_count());
        for i in 0..commit.parent_count() {
            if let Ok(parent_id) = commit.parent_id(i) {
                parents.push(git_oid_to_btoid(parent_id));
            }
        }
        if let Ok(mut lock) = cache.lock() {
            lock.entry(btoid).or_insert_with(|| CommitEdges {
                parents: parents.clone(),
                author_time: commit.author().when().seconds(),
            });
        }
        if !parents.is_empty() {
            indexes.push(flat.len() as u32);
            flat.push(btoid);
            flat.extend_from_slice(&parents);
        }
        if stop_after == Some(btoid) {
            break;
        }
    }

    let (oids_ptr, oids_cap) = unsafe {
        alloc_and_copy_slice_with_cap(&flat, next_legacy_capacity(flat.len()))
    };
    if !flat.is_empty() && oids_ptr.is_null() {
        set_last_error_str("insufficient memory");
        return BT_ERR;
    }
    let (indexes_ptr, indexes_cap) = unsafe {
        alloc_and_copy_slice_with_cap(&indexes, next_legacy_capacity(indexes.len()))
    };
    if !indexes.is_empty() && indexes_ptr.is_null() {
        if !oids_ptr.is_null() {
            unsafe { heap_free(oids_ptr as *mut c_void) };
        }
        set_last_error_str("insufficient memory");
        return BT_ERR;
    }
    unsafe {
        (*out_result).oids = oids_ptr;
        (*out_result).oids_len = flat.len() as i64;
        (*out_result).oids_cap = oids_cap as i64;
        (*out_result).indexes = indexes_ptr;
        (*out_result).indexes_len = indexes.len() as i64;
        (*out_result).indexes_cap = indexes_cap as i64;
        (*out_result).has_more = 0;
    }
    BT_OK
}

#[no_mangle]
pub unsafe extern "C" fn bt_get_behind_ahead_counts(
    git_dir_path: *const c_char,
    oid_pairs_ptr: *const BtOidPair,
    oid_pairs_len: i64,
    _cache_handle: *mut *mut c_void,
    out_result: *mut BtBehindAheadCounts,
) -> c_int {
    if out_result.is_null() {
        set_last_error_str("bt_get_behind_ahead_counts: out_result is null");
        return BT_ERR;
    }
    unsafe {
        (*out_result).items = core::ptr::null_mut();
        (*out_result).items_len = 0;
        (*out_result).items_cap = 0;
    }
    let git_dir_str = match cstr_to_utf8(git_dir_path, "bt_get_behind_ahead_counts: git_dir_path") {
        Ok(s) => s,
        Err(rc) => return rc,
    };
    let git_dir = Path::new(git_dir_str);
    if oid_pairs_ptr.is_null() || oid_pairs_len <= 0 {
        return BT_OK;
    }

    let repo = match git2::Repository::open(git_dir) {
        Ok(r) => r,
        Err(e) => {
            set_last_error_str(&format!("failed to open repository: {e}"));
            return BT_ERR;
        }
    };

    let pairs = unsafe { std::slice::from_raw_parts(oid_pairs_ptr, oid_pairs_len as usize) };
    let mut items = Vec::with_capacity(pairs.len());

    for pair in pairs {
        let left_raw = pair.left.to_bytes();
        let right_raw = pair.right.to_bytes();
        let mut left_count = 0;
        let mut right_count = 0;
        if let (Ok(left_oid), Ok(right_oid)) = (git2::Oid::from_bytes(&left_raw), git2::Oid::from_bytes(&right_raw)) {
            if let Ok((ahead, behind)) = repo.graph_ahead_behind(left_oid, right_oid) {
                left_count = ahead as u32;
                right_count = behind as u32;
            }
        }
        items.push(BtBehindAheadCount { left: left_count, right: right_count });
    }

    let p = unsafe { alloc_and_copy_slice(&items) };
    if !items.is_empty() && p.is_null() {
        set_last_error_str("insufficient memory");
        return BT_ERR;
    }
    unsafe {
        (*out_result).items = p;
        (*out_result).items_len = items.len() as i64;
        (*out_result).items_cap = items.len() as i64;
    }
    BT_OK
}

#[no_mangle]
pub unsafe extern "C" fn bt_find_fartherest_tip(
    git_dir_path: *const c_char,
    _head_oid: *const BtOid,
    tips_ptr: *const BtOid,
    tips_len: i64,
    base_oid: *const BtOid,
    _cache_handle: *mut *mut c_void,
    out_result: *mut BtOid,
) -> c_int {
    if out_result.is_null() || base_oid.is_null() {
        set_last_error_str("bt_find_fartherest_tip: invalid arguments");
        return BT_ERR;
    }
    let git_dir_str = match cstr_to_utf8(git_dir_path, "bt_find_fartherest_tip: git_dir_path") {
        Ok(s) => s,
        Err(rc) => return rc,
    };
    let git_dir = Path::new(git_dir_str);

    let repo = match git2::Repository::open(git_dir) {
        Ok(r) => r,
        Err(e) => {
            set_last_error_str(&format!("failed to open repository: {e}"));
            return BT_ERR;
        }
    };

    let base_raw = unsafe { (*base_oid).to_bytes() };
    let base_git2_oid = match git2::Oid::from_bytes(&base_raw) {
        Ok(o) => o,
        Err(_) => {
            unsafe { *out_result = *base_oid };
            return BT_OK;
        }
    };

    if tips_ptr.is_null() || tips_len <= 0 {
        unsafe {
            *out_result = *base_oid;
        }
        return BT_OK;
    }

    let tips = unsafe { std::slice::from_raw_parts(tips_ptr, tips_len as usize) };
    let mut best = tips[0];
    let mut best_count: i64 = -1;

    for &tip in tips {
        let tip_raw = tip.to_bytes();
        if let Ok(tip_git2_oid) = git2::Oid::from_bytes(&tip_raw) {
            if let Ok((ahead, _)) = repo.graph_ahead_behind(tip_git2_oid, base_git2_oid) {
                let count = ahead as i64;
                if count > best_count {
                    best_count = count;
                    best = tip;
                }
            }
        }
    }

    unsafe {
        *out_result = best;
    }
    BT_OK
}

#[no_mangle]
pub unsafe extern "C" fn bt_search_commits(
    git_dir_path: *const c_char,
    oids_ptr: *const BtOid,
    oids_len: i64,
    query: *const c_char,
    _ref_matches_ptr: *const BtOid,
    _ref_matches_len: i64,
    cancellation_token_ptr: *mut *mut u8,
    out_result: *mut BtSearchCommitsResult,
) -> c_int {
    if out_result.is_null() {
        set_last_error_str("bt_search_commits: out_result is null");
        return BT_ERR;
    }
    unsafe {
        (*out_result).matches = core::ptr::null_mut();
        (*out_result).matches_len = 0;
        (*out_result).matches_cap = 0;
    }

    if is_canceled(cancellation_token_ptr) {
        set_last_error_str("Canceled");
        return BT_ERR_CANCELED;
    }

    let git_dir_str = match cstr_to_utf8(git_dir_path, "bt_search_commits: git_dir_path") {
        Ok(s) => s,
        Err(rc) => return rc,
    };
    let git_dir = Path::new(git_dir_str);

    let repo = match git2::Repository::open(git_dir) {
        Ok(r) => r,
        Err(e) => {
            set_last_error_str(&format!("failed to open repository: {e}"));
            return BT_ERR;
        }
    };

    let query_str = if query.is_null() {
        ""
    } else {
        match cstr_to_utf8(query, "bt_search_commits: query") {
            Ok(s) => s,
            Err(_) => "",
        }
    };
    let needle = query_str.to_ascii_lowercase();

    if oids_ptr.is_null() || oids_len <= 0 {
        return BT_OK;
    }
    let oids = unsafe { std::slice::from_raw_parts(oids_ptr, oids_len as usize) };

    let mut matches = Vec::new();
    for oid in oids {
        if is_canceled(cancellation_token_ptr) {
            set_last_error_str("Canceled");
            return BT_ERR_CANCELED;
        }
        let raw_oid = oid.to_bytes();
        if let Ok(git2_oid) = git2::Oid::from_bytes(&raw_oid) {
            if let Ok(commit) = repo.find_commit(git2_oid) {
                let msg = commit.message().unwrap_or("").to_ascii_lowercase();
                let oid_hex = btoid_to_hex(oid);
                if needle.is_empty() || msg.contains(&needle) || oid_hex.contains(&needle) {
                    matches.push(*oid);
                }
            }
        }
    }

    let p = unsafe { alloc_and_copy_slice(&matches) };
    if !matches.is_empty() && p.is_null() {
        set_last_error_str("insufficient memory");
        return BT_ERR;
    }
    unsafe {
        (*out_result).matches = p;
        (*out_result).matches_len = matches.len() as i64;
        (*out_result).matches_cap = matches.len() as i64;
    }
    BT_OK
}
