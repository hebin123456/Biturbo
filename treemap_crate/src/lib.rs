// Standalone treemap crate for algorithm verification under wine.
// Exports bt_layout_treemap + bt_release_layout_treemap with the SAME ABI as
// the real biturbo.dll, so the probe.exe harness can load either DLL and diff.
use core::ffi::c_void;
use std::os::raw::c_int;
use std::vec::Vec;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BtRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BtTreemapItem {
    pub index: i64,
    pub rect: BtRect,
}

#[repr(C)]
pub struct BtLayoutTreemapResult {
    pub items: *mut BtTreemapItem,
    pub items_len: i64,
    pub items_cap: i64,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    index: i64,
    size: f64,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetProcessHeap() -> *mut c_void;
    fn HeapAlloc(h: *mut c_void, flags: u32, bytes: usize) -> *mut c_void;
    fn HeapFree(h: *mut c_void, flags: u32, p: *mut c_void) -> i32;
}

unsafe fn heap_alloc(bytes: usize) -> *mut u8 {
    if bytes == 0 {
        return core::ptr::null_mut();
    }
    let h = unsafe { GetProcessHeap() };
    if h.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { HeapAlloc(h, 0, bytes) as *mut u8 }
}

unsafe fn heap_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let h = unsafe { GetProcessHeap() };
    if h.is_null() {
        return;
    }
    unsafe {
        let _ = HeapFree(h, 0, ptr);
    }
}

#[no_mangle]
pub unsafe extern "C" fn bt_layout_treemap(
    sizes_ptr: *const i64,
    sizes_len: i64,
    rect: BtRect,
    out_result: *mut BtLayoutTreemapResult,
) -> c_int {
    unsafe {
        (*out_result).items = core::ptr::null_mut();
        (*out_result).items_len = 0;
        (*out_result).items_cap = 0;
    }
    if out_result.is_null() || sizes_len < 0 {
        return 1;
    }
    if sizes_ptr.is_null() || sizes_len == 0 {
        return 0;
    }
    let sizes = unsafe { core::slice::from_raw_parts(sizes_ptr, sizes_len as usize) };
    let items = layout_treemap_impl(sizes, rect);
    let bytes_len = items.len() * core::mem::size_of::<BtTreemapItem>();
    let ptr = unsafe { heap_alloc(bytes_len) } as *mut BtTreemapItem;
    if !ptr.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(items.as_ptr(), ptr, items.len());
            (*out_result).items = ptr;
            (*out_result).items_len = items.len() as i64;
            (*out_result).items_cap = items.len() as i64;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn bt_release_layout_treemap(buf: *mut BtLayoutTreemapResult) {
    unsafe {
        if buf.is_null() || (*buf).items_cap == 0 {
            return;
        }
        let ptr = core::ptr::replace(&mut (*buf).items, core::ptr::null_mut());
        (*buf).items_len = 0;
        (*buf).items_cap = 0;
        heap_free(ptr as *mut c_void);
    }
}

// ===================== layout algorithm (copied from src/ffi/bt_layout_treemap.rs) =====================
fn layout_treemap_impl(sizes: &[i64], rect: BtRect) -> Vec<BtTreemapItem> {
    let mut nodes: Vec<Node> = sizes
        .iter()
        .enumerate()
        .map(|(i, &s)| Node {
            index: i as i64,
            size: if s > 0 { s as f64 } else { 0.0 },
        })
        .collect();
    nodes.sort_by(|a, b| b.size.partial_cmp(&a.size).unwrap_or(core::cmp::Ordering::Equal));

    let mut items = Vec::new();
    if nodes.is_empty() {
        return items;
    }
    let total: f64 = nodes.iter().map(|n| n.size).sum();
    if total <= 0.0 {
        for n in &nodes {
            items.push(BtTreemapItem {
                index: n.index,
                rect: BtRect { x: rect.x, y: rect.y, w: 1.0, h: 1.0 },
            });
        }
        return items;
    }
    if nodes.len() < 3 {
        layout_simple(&nodes, rect, &mut items);
        return items;
    }
    let canvas_area = f64::max(0.0, rect.w * rect.h);
    let mut remaining = rect;
    let mut row: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < nodes.len() {
        let candidate_worst = worst_aspect(&nodes, &row, i, canvas_area, total, &remaining);
        let current_worst = if row.is_empty() {
            f64::INFINITY
        } else {
            worst_aspect(&nodes, &row, usize::MAX, canvas_area, total, &remaining)
        };
        if !row.is_empty() && candidate_worst > current_worst {
            layout_row(&nodes, &row, canvas_area, total, &mut remaining, &mut items);
            row.clear();
        }
        row.push(i);
        i += 1;
    }
    if !row.is_empty() {
        layout_row(&nodes, &row, canvas_area, total, &mut remaining, &mut items);
    }
    items
}

fn worst_aspect(
    nodes: &[Node],
    row: &[usize],
    candidate: usize,
    canvas_area: f64,
    total: f64,
    remaining: &BtRect,
) -> f64 {
    let fixed = |idx: usize| canvas_area * nodes[idx].size / total;
    let mut sum_area = 0.0f64;
    let mut min_area = f64::INFINITY;
    let mut max_area = 0.0f64;
    for &idx in row {
        let a = fixed(idx);
        sum_area += a;
        if a < min_area { min_area = a; }
        if a > max_area { max_area = a; }
    }
    if candidate != usize::MAX {
        let a = fixed(candidate);
        sum_area += a;
        if a < min_area { min_area = a; }
        if a > max_area { max_area = a; }
    }
    if sum_area <= 0.0 || min_area <= 0.0 {
        return f64::INFINITY;
    }
    let side = remaining.w.min(remaining.h);
    if side <= 0.0 {
        return f64::INFINITY;
    }
    let s2 = side * side;
    (s2 * max_area / (sum_area * sum_area)).max((sum_area * sum_area) / (s2 * min_area))
}

fn layout_row(
    nodes: &[Node],
    row: &[usize],
    canvas_area: f64,
    total: f64,
    remaining: &mut BtRect,
    items: &mut Vec<BtTreemapItem>,
) {
    let fixed = |idx: usize| canvas_area * nodes[idx].size / total;
    let mut sum_area = 0.0f64;
    for &idx in row {
        sum_area += fixed(idx);
    }
    if sum_area <= 0.0 || row.is_empty() {
        return;
    }
    if remaining.w <= remaining.h {
        let t = if remaining.w > 0.0 { sum_area / remaining.w } else { 0.0 };
        let mut x = remaining.x;
        for &idx in row {
            let a = fixed(idx);
            let w = if t > 0.0 { a / t } else { 0.0 };
            items.push(BtTreemapItem {
                index: nodes[idx].index,
                rect: BtRect { x, y: remaining.y, w, h: t },
            });
            x += w;
        }
        remaining.y += t;
        remaining.h -= t;
    } else {
        let t = if remaining.h > 0.0 { sum_area / remaining.h } else { 0.0 };
        let mut y = remaining.y;
        for &idx in row {
            let a = fixed(idx);
            let h = if t > 0.0 { a / t } else { 0.0 };
            items.push(BtTreemapItem {
                index: nodes[idx].index,
                rect: BtRect { x: remaining.x, y, w: t, h },
            });
            y += h;
        }
        remaining.x += t;
        remaining.w -= t;
    }
}

fn layout_simple(nodes: &[Node], rect: BtRect, items: &mut Vec<BtTreemapItem>) {
    if nodes.is_empty() {
        return;
    }
    if nodes.len() == 1 {
        items.push(BtTreemapItem { index: nodes[0].index, rect });
        return;
    }
    let denom = nodes[0].size + nodes[1].size;
    let frac = if denom > 0.0 { nodes[0].size / denom } else { 0.5 };
    if rect.w >= rect.h {
        let w0 = rect.w * frac;
        items.push(BtTreemapItem {
            index: nodes[0].index,
            rect: BtRect { x: rect.x, y: rect.y, w: w0, h: rect.h },
        });
        items.push(BtTreemapItem {
            index: nodes[1].index,
            rect: BtRect { x: rect.x + w0, y: rect.y, w: rect.w - w0, h: rect.h },
        });
    } else {
        let h0 = rect.h * frac;
        items.push(BtTreemapItem {
            index: nodes[0].index,
            rect: BtRect { x: rect.x, y: rect.y, w: rect.w, h: h0 },
        });
        items.push(BtTreemapItem {
            index: nodes[1].index,
            rect: BtRect { x: rect.x, y: rect.y + h0, w: rect.w, h: rect.h - h0 },
        });
    }
}
