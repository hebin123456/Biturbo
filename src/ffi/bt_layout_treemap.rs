use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::heap_alloc;
use std::os::raw::c_int;

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
    // Size normalized to f64 at construction time, matching the reference
    // implementation. All layout arithmetic is done in f64 to avoid i64
    // overflow (which previously panicked at the FFI boundary and aborted
    // the host process).
    size: f64,
}

#[no_mangle]
pub unsafe extern "C" fn bt_layout_treemap(
    sizes_ptr: *const i64,
    sizes_len: i64,
    rect: BtRect,
    out_result: *mut BtLayoutTreemapResult,
) -> c_int {
    if out_result.is_null() || sizes_len < 0 {
        set_last_error_str("invalid arguments");
        return 1;
    }

    unsafe {
        (*out_result).items = core::ptr::null_mut();
        (*out_result).items_len = 0;
        (*out_result).items_cap = 0;
    }

    if sizes_ptr.is_null() || sizes_len == 0 {
        return 0;
    }

    let sizes = unsafe { std::slice::from_raw_parts(sizes_ptr, sizes_len as usize) };
    let items = layout_treemap_impl(sizes, rect);
    assign_items_result(items, out_result);
    0
}

/// Pure layout logic, separated from the FFI allocation so it can be unit
/// tested on any platform.
///
/// Matches the reference `biturbo.dll`: every size is converted to f64 up
/// front, and layout uses the squarify algorithm. For fewer than 3 nodes a
/// simple proportional split is used (also matching the reference).
fn layout_treemap_impl(sizes: &[i64], rect: BtRect) -> Vec<BtTreemapItem> {
    let mut nodes: Vec<Node> = sizes
        .iter()
        .enumerate()
        .map(|(i, &s)| Node {
            index: i as i64,
            size: if s > 0 { s as f64 } else { 0.0 },
        })
        .collect();

    // Sort descending by size.
    nodes.sort_by(|a, b| b.size.partial_cmp(&a.size).unwrap_or(std::cmp::Ordering::Equal));

    let mut items = Vec::new();

    if nodes.is_empty() {
        return items;
    }

    // Total of all positive sizes, accumulated in f64 (no overflow risk).
    let total: f64 = nodes.iter().map(|n| n.size).sum();

    if total <= 0.0 {
        // All sizes are zero/negative: emit 1x1 placeholders anchored at rect.
        for n in &nodes {
            items.push(BtTreemapItem {
                index: n.index,
                rect: BtRect { x: rect.x, y: rect.y, w: 1.0, h: 1.0 },
            });
        }
        return items;
    }

    // Fewer than 3 nodes: simple proportional split (matches reference's
    // small-count path).
    if nodes.len() < 3 {
        layout_simple(&nodes, rect, &mut items);
        return items;
    }

    // 3+ nodes: squarify.
    let mut remaining = rect;
    let mut row: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < nodes.len() {
        let candidate_worst = worst_aspect(&nodes, &row, i, total, &remaining);
        let current_worst = if row.is_empty() {
            f64::INFINITY
        } else {
            worst_aspect(&nodes, &row, usize::MAX, total, &remaining)
        };

        if !row.is_empty() && candidate_worst > current_worst {
            // Adding this node makes the row worse; lay out the current row
            // and start a new one.
            layout_row(&nodes, &row, total, &mut remaining, &mut items);
            row.clear();
        }
        row.push(i);
        i += 1;
    }
    // Flush the final row.
    if !row.is_empty() {
        layout_row(&nodes, &row, total, &mut remaining, &mut items);
    }

    items
}

/// Worst aspect ratio of a row of nodes within `remaining`, optionally
/// including the candidate node `candidate` (usize::MAX means "current row
/// only"). `total` is the sum of all node sizes (used to convert sizes to
/// areas).
fn worst_aspect(
    nodes: &[Node],
    row: &[usize],
    candidate: usize,
    total: f64,
    remaining: &BtRect,
) -> f64 {
    let area_total = f64::max(0.0, remaining.w * remaining.h);
    let mut sum_area = 0.0f64;
    let mut min_area = f64::INFINITY;
    let mut max_area = 0.0f64;

    for &idx in row {
        let a = area_total * nodes[idx].size / total;
        sum_area += a;
        if a < min_area { min_area = a; }
        if a > max_area { max_area = a; }
    }
    if candidate != usize::MAX {
        let a = area_total * nodes[candidate].size / total;
        sum_area += a;
        if a < min_area { min_area = a; }
        if a > max_area { max_area = a; }
    }

    if sum_area <= 0.0 || min_area <= 0.0 {
        return f64::INFINITY;
    }
    // The row is laid out along the shorter side of `remaining`.
    let side = remaining.w.min(remaining.h);
    if side <= 0.0 {
        return f64::INFINITY;
    }
    let s2 = side * side;
    // max(w^2 * maxArea / sum^2, sum^2 / (w^2 * minArea))
    (s2 * max_area / (sum_area * sum_area)).max((sum_area * sum_area) / (s2 * min_area))
}

/// Lay out a row of nodes along the shorter side of `remaining`, pushing the
/// produced items into `items` and shrinking `remaining` accordingly.
fn layout_row(
    nodes: &[Node],
    row: &[usize],
    total: f64,
    remaining: &mut BtRect,
    items: &mut Vec<BtTreemapItem>,
) {
    let area_total = f64::max(0.0, remaining.w * remaining.h);
    let mut sum_area = 0.0f64;
    for &idx in row {
        sum_area += area_total * nodes[idx].size / total;
    }
    if sum_area <= 0.0 || row.is_empty() {
        return;
    }

    if remaining.w >= remaining.h {
        // Row laid out horizontally: height = sum_area / width, each node
        // gets width = area / height.
        let h = if remaining.w > 0.0 { sum_area / remaining.w } else { 0.0 };
        let mut x = remaining.x;
        for &idx in row {
            let w = if h > 0.0 { (area_total * nodes[idx].size / total) / h } else { 0.0 };
            items.push(BtTreemapItem {
                index: nodes[idx].index,
                rect: BtRect { x, y: remaining.y, w, h },
            });
            x += w;
        }
        remaining.y += h;
        remaining.h -= h;
    } else {
        // Row laid out vertically: width = sum_area / height.
        let w = if remaining.h > 0.0 { sum_area / remaining.h } else { 0.0 };
        let mut y = remaining.y;
        for &idx in row {
            let h = if w > 0.0 { (area_total * nodes[idx].size / total) / w } else { 0.0 };
            items.push(BtTreemapItem {
                index: nodes[idx].index,
                rect: BtRect { x: remaining.x, y, w, h },
            });
            y += h;
        }
        remaining.x += w;
        remaining.w -= w;
    }
}

/// Simple proportional split for fewer than 3 nodes. With one node it fills
/// the whole rect; with two it splits along the longer dimension by size
/// ratio.
fn layout_simple(nodes: &[Node], rect: BtRect, items: &mut Vec<BtTreemapItem>) {
    if nodes.is_empty() {
        return;
    }
    if nodes.len() == 1 {
        items.push(BtTreemapItem {
            index: nodes[0].index,
            rect,
        });
        return;
    }
    // Two nodes: split along the longer side.
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

unsafe fn assign_items_result(items: Vec<BtTreemapItem>, out_result: *mut BtLayoutTreemapResult) {
    let bytes_len = items.len() * std::mem::size_of::<BtTreemapItem>();
    let ptr = unsafe { heap_alloc(bytes_len) } as *mut BtTreemapItem;
    if !ptr.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(items.as_ptr(), ptr, items.len());
            (*out_result).items = ptr;
            (*out_result).items_len = items.len() as i64;
            (*out_result).items_cap = items.len() as i64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(sizes: &[i64], rect: BtRect) -> Vec<BtTreemapItem> {
        layout_treemap_impl(sizes, rect)
    }

    // Reproduces the suspected integer-overflow panic in the `has_zero`
    // branch: `total += positive_nodes[i].size` overflows i64 in debug
    // builds when two large sizes are summed.
    #[test]
    fn repro_overflow_has_zero_branch() {
        // Two near-i64::MAX positives + one zero forces the has_zero branch
        // and the i64 accumulation `total += size`.
        let sizes = vec![i64::MAX / 2 + 1, i64::MAX / 2 + 1, 0];
        let rect = BtRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let _ = call(&sizes, rect);
    }

    // Reproduces the suspected overflow in the "one huge, many tiny" branch:
    // `nodes[0].size + (len as i64) - 2` and `last.size * 10`.
    #[test]
    fn repro_overflow_huge_tiny_branch() {
        // >=4 nodes, largest >= 10x smallest (both positive), largest huge.
        let sizes = vec![i64::MAX - 5, 1, 1, 1];
        let rect = BtRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let _ = call(&sizes, rect);
    }

    // Reproduces the suspected overflow in the recursive <=8 branch:
    // `nodes[start].size + nodes[start+1].size`.
    #[test]
    fn repro_overflow_recursive_branch() {
        // 3 positive nodes (no zero, < 4 so no huge-tiny, <= 8 -> recursive).
        // First two are near i64::MAX so their sum overflows in debug.
        let sizes = vec![i64::MAX / 2 + 1, i64::MAX / 2 + 1, 1];
        let rect = BtRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let _ = call(&sizes, rect);
    }

    // Sanity: a normal layout produces one item per input.
    #[test]
    fn normal_layout_works() {
        let sizes = vec![10, 20, 30, 40];
        let rect = BtRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let items = call(&sizes, rect);
        assert_eq!(items.len(), sizes.len());
    }
}

