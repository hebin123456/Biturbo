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
    if values.is_empty() {
        return core::ptr::null_mut();
    }
    let bytes = values.len() * std::mem::size_of::<T>();
    let p = unsafe { heap_alloc(bytes) } as *mut T;
    if p.is_null() {
        return core::ptr::null_mut();
    }
    unsafe {
        core::ptr::copy_nonoverlapping(values.as_ptr() as *const u8, p as *mut u8, bytes);
    }
    p
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
        let oids_ptr = std::ptr::replace(&mut (*r).oids, core::ptr::null_mut());
        (*r).oids_len = 0;
        (*r).oids_cap = 0;
        if !oids_ptr.is_null() {
            heap_free(oids_ptr as *mut c_void);
        }

        let indexes_ptr = std::ptr::replace(&mut (*r).indexes, core::ptr::null_mut());
        (*r).indexes_len = 0;
        (*r).indexes_cap = 0;
        if !indexes_ptr.is_null() {
            heap_free(indexes_ptr as *mut c_void);
        }

        (*r).has_more = 0;
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
    time: i64,
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
            time: edges.author_time,
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

#[derive(PartialEq, Eq)]
struct ReadyItem {
    index: usize,
    priority: i64,
    time: i64,
    oid: BtOid,
}

impl Ord for ReadyItem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let cmp_prio = self.priority.cmp(&other.priority);
        if cmp_prio != std::cmp::Ordering::Equal {
            return cmp_prio;
        }
        let cmp_time = self.time.cmp(&other.time);
        if cmp_time != std::cmp::Ordering::Equal {
            return cmp_time;
        }
        if self.oid == other.oid {
            std::cmp::Ordering::Equal
        } else if self.oid < other.oid {
            std::cmp::Ordering::Greater // SMALLER OID gets HIGHER priority in BinaryHeap
        } else {
            std::cmp::Ordering::Less
        }
    }
}

impl PartialOrd for ReadyItem {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
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
    let mut record_indexes = HashMap::new();
    let mut records = Vec::new();
    let mut stack = tips.to_vec();
    *hit_limit = false;

    while let Some(oid) = stack.pop() {
        if is_canceled(cancellation_token_ptr) {
            set_last_error_str("Canceled");
            return false;
        }
        if record_indexes.contains_key(&oid) {
            continue;
        }
        let Some(edges) = get_commit_edges(repo, oid, cache) else {
            continue;
        };
        record_indexes.insert(oid, records.len());
        records.push(CommitRecord {
            oid,
            parents: edges.parents.clone(),
            time: edges.author_time,
        });

        // PUSH PARENTS IN REVERSE ORDER TO MATCH STACK LIFO (visiting first parent first!)
        for &parent in edges.parents.iter().rev() {
            if !record_indexes.contains_key(&parent) {
                stack.push(parent);
            }
        }
    }

    let mut remaining_children = vec![0u32; records.len()];
    for record in &records {
        for &parent in &record.parents {
            if let Some(&parent_idx) = record_indexes.get(&parent) {
                remaining_children[parent_idx] += 1;
            }
        }
    }

    let mut ready = std::collections::BinaryHeap::new();
    let mut emitted = vec![0u8; records.len()];
    let mut continuation_priority = vec![0i64; records.len()];

    for i in 0..records.len() {
        if remaining_children[i] == 0 {
            ready.push(ReadyItem {
                index: i,
                priority: 0,
                time: records[i].time,
                oid: records[i].oid,
            });
        }
    }

    let mut emitted_count = 0usize;
    let start = std::cmp::max(0, skip_count) as usize;
    let end_limit = if max_count > 0 {
        start + max_count as usize
    } else {
        usize::MAX
    };

    while let Some(item) = ready.pop() {
        if is_canceled(cancellation_token_ptr) {
            set_last_error_str("Canceled");
            return false;
        }
        let record_index = item.index;
        if emitted[record_index] != 0 {
            continue;
        }
        emitted[record_index] = 1;

        let record = &records[record_index];
        if emitted_count >= start && emitted_count < end_limit {
            indexes.push(flat.len() as u32);
            flat.push(record.oid);
            for &parent in &record.parents {
                flat.push(parent);
            }
        }
        emitted_count += 1;
        if emitted_count > end_limit {
            *hit_limit = true;
            break;
        }

        let mut parent_position = 0i64;
        for &parent in &record.parents {
            if let Some(&parent_index) = record_indexes.get(&parent) {
                if remaining_children[parent_index] > 0 {
                    remaining_children[parent_index] -= 1;
                    if remaining_children[parent_index] == 0 {
                        // Formula from C++ biturbo.cpp
                        continuation_priority[parent_index] = (records.len() - emitted_count) as i64 * 16 - parent_position;
                        ready.push(ReadyItem {
                            index: parent_index,
                            priority: continuation_priority[parent_index],
                            time: records[parent_index].time,
                            oid: records[parent_index].oid,
                        });
                    }
                }
            }
            parent_position += 1;
        }
    }

    *hit_limit = *hit_limit || emitted_count < records.len();
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

    let mut combined_tips = tips.to_vec();
    combined_tips.extend_from_slice(required);

    let mut max_count = page_size.saturating_mul(std::cmp::max(1, min_pages));
    if max_count <= 0 {
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

    let success = if combined_tips.len() <= 1 {
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

    let oids_ptr = unsafe { alloc_and_copy_slice(&flat) };
    if !flat.is_empty() && oids_ptr.is_null() {
        set_last_error_str("insufficient memory");
        return BT_ERR;
    }
    let indexes_ptr = unsafe { alloc_and_copy_slice(&indexes) };
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
        (*out_result).oids_cap = flat.len() as i64;
        (*out_result).indexes = indexes_ptr;
        (*out_result).indexes_len = indexes.len() as i64;
        (*out_result).indexes_cap = indexes.len() as i64;
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
    unsafe {
        bt_get_commits(
            git_dir_path,
            oid,
            1,
            0,
            10000,
            0,
            1,
            core::ptr::null(),
            0,
            cache,
            core::ptr::null_mut(),
            out_result,
        )
    }
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
    let tips = [unsafe { *dst }, unsafe { *src }];
    unsafe {
        bt_get_commits(
            git_dir_path,
            tips.as_ptr(),
            2,
            0,
            10000,
            0,
            1,
            core::ptr::null(),
            0,
            cache,
            core::ptr::null_mut(),
            out_result,
        )
    }
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
                if needle.is_empty() || msg.contains(&needle) {
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
