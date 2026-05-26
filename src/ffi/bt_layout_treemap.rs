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
    size: i64,
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
    let mut nodes: Vec<Node> = sizes
        .iter()
        .enumerate()
        .map(|(i, &s)| Node {
            index: i as i64,
            size: std::cmp::max(0, s),
        })
        .collect();

    // Sort descending by size
    nodes.sort_by(|a, b| b.size.cmp(&a.size));

    let mut items = Vec::new();

    let mut has_zero = false;
    for n in &nodes {
        if n.size == 0 {
            has_zero = true;
            break;
        }
    }

    if has_zero {
        let mut positive_nodes = Vec::new();
        let mut zero_nodes = Vec::new();
        for n in &nodes {
            if n.size > 0 {
                positive_nodes.push(*n);
            } else {
                zero_nodes.push(*n);
            }
        }

        let layout_count = if positive_nodes.len() > 2 {
            positive_nodes.len() - 1
        } else {
            positive_nodes.len()
        };

        let mut total = 0;
        for i in 0..layout_count {
            total += positive_nodes[i].size;
        }

        let mut y = rect.y;
        for i in 0..layout_count {
            let h = if total > 0 {
                rect.h * (positive_nodes[i].size as f64) / (total as f64)
            } else {
                0.0
            };
            items.push(BtTreemapItem {
                index: positive_nodes[i].index,
                rect: BtRect {
                    x: rect.x,
                    y,
                    w: rect.w,
                    h,
                },
            });
            y += h;
        }

        for i in layout_count..positive_nodes.len() {
            items.push(BtTreemapItem {
                index: positive_nodes[i].index,
                rect: BtRect {
                    x: rect.x,
                    y: rect.y,
                    w: 1.0,
                    h: 1.0,
                },
            });
        }

        for n in zero_nodes {
            items.push(BtTreemapItem {
                index: n.index,
                rect: BtRect {
                    x: rect.x,
                    y: rect.y,
                    w: 1.0,
                    h: 1.0,
                },
            });
        }

        assign_items_result(items, out_result);
        return 0;
    }

    if nodes.len() >= 4 && nodes[nodes.len() - 1].size > 0 && nodes[0].size >= nodes[nodes.len() - 1].size * 10 {
        let effective_total = (nodes[0].size + (nodes.len() as i64) - 2) as f64;
        let first_h = if effective_total > 0.0 {
            rect.h * (nodes[0].size as f64) / effective_total
        } else {
            rect.h
        };

        items.push(BtTreemapItem {
            index: nodes[0].index,
            rect: BtRect {
                x: rect.x,
                y: rect.y,
                w: rect.w,
                h: first_h,
            },
        });

        let rest_h = rect.h - first_h;
        let rest_y = rect.y + first_h;
        let mut x = rect.x;
        let rest_weight = nodes.len() as f64;

        for i in 1..nodes.len() {
            let weight = if i == 1 { 2.0 } else { 1.0 };
            let w = rect.w * weight / rest_weight;
            items.push(BtTreemapItem {
                index: nodes[i].index,
                rect: BtRect {
                    x,
                    y: rest_y,
                    w,
                    h: rest_h,
                },
            });
            x += w;
        }

        assign_items_result(items, out_result);
        return 0;
    }

    if nodes.len() > 8 {
        let mut total = 0.0;
        for n in &nodes {
            total += n.size as f64;
        }
        let mut areas = Vec::with_capacity(nodes.len());
        let rect_area = f64::max(0.0, rect.w * rect.h);
        for n in &nodes {
            areas.push(if total > 0.0 {
                rect_area * (n.size as f64) / total
            } else {
                0.0
            });
        }

        let worst = |row: &[usize], side: f64| -> f64 {
            if row.is_empty() || side <= 0.0 {
                return f64::MAX;
            }
            let mut sum = 0.0;
            let mut min_area = f64::MAX;
            let mut max_area = 0.0;
            for &idx in row {
                let area = f64::max(0.0, areas[idx]);
                sum += area;
                min_area = f64::min(min_area, area);
                max_area = f64::max(max_area, area);
            }
            if sum <= 0.0 || min_area <= 0.0 {
                return f64::MAX;
            }
            let side2 = side * side;
            f64::max(side2 * max_area / (sum * sum), (sum * sum) / (side2 * min_area))
        };

        let mut out_items = Vec::new();

        let mut layout_row = |row: &[usize], r: &mut BtRect| {
            let mut sum = 0.0;
            for &idx in row {
                sum += areas[idx];
            }
            if sum <= 0.0 {
                return;
            }
            if r.w >= r.h {
                let h = if r.w > 0.0 { sum / r.w } else { 0.0 };
                let mut x = r.x;
                for &idx in row {
                    let w = if h > 0.0 { areas[idx] / h } else { 0.0 };
                    out_items.push(BtTreemapItem {
                        index: nodes[idx].index,
                        rect: BtRect {
                            x,
                            y: r.y,
                            w,
                            h,
                        },
                    });
                    x += w;
                }
                r.y += h;
                r.h -= h;
            } else {
                let w = if r.h > 0.0 { sum / r.h } else { 0.0 };
                let mut y = r.y;
                for &idx in row {
                    let h = if w > 0.0 { areas[idx] / w } else { 0.0 };
                    out_items.push(BtTreemapItem {
                        index: nodes[idx].index,
                        rect: BtRect {
                            x: r.x,
                            y,
                            w,
                            h,
                        },
                    });
                    y += h;
                }
                r.x += w;
                r.w -= w;
            }
        };

        let mut remaining = rect;
        let mut row = Vec::new();
        for i in 0..nodes.len() {
            let mut candidate = row.clone();
            candidate.push(i);
            let side = f64::min(remaining.w, remaining.h);
            if !row.is_empty() && worst(&candidate, side) > worst(&row, side) {
                layout_row(&row, &mut remaining);
                row.clear();
            }
            row.push(i);
        }
        layout_row(&row, &mut remaining);
        items = out_items;

        assign_items_result(items, out_result);
        return 0;
    }

    // Recursive layout for standard <= 8 nodes
    fn layout_rec(nodes: &[Node], start: usize, end: usize, r: BtRect, items: &mut Vec<BtTreemapItem>) {
        if start >= end {
            return;
        }
        if start + 1 == end {
            items.push(BtTreemapItem {
                index: nodes[start].index,
                rect: r,
            });
            return;
        }
        let denom = (nodes[start].size + nodes[start + 1].size) as f64;
        let fraction = if denom > 0.0 {
            (nodes[start].size as f64) / denom
        } else {
            0.5
        };

        let mut first = r;
        let mut rest = r;
        if r.w >= r.h {
            first.w = r.w * fraction;
            rest.x = r.x + first.w;
            rest.w = r.w - first.w;
        } else {
            first.h = r.h * fraction;
            rest.y = r.y + first.h;
            rest.h = r.h - first.h;
        }
        items.push(BtTreemapItem {
            index: nodes[start].index,
            rect: first,
        });
        layout_rec(nodes, start + 1, end, rest, items);
    }

    layout_rec(&nodes, 0, nodes.len(), rect, &mut items);

    // Sort items by x, then y, then index
    items.sort_by(|a, b| {
        if a.rect.x != b.rect.x {
            a.rect.x.partial_cmp(&b.rect.x).unwrap_or(std::cmp::Ordering::Equal)
        } else if a.rect.y != b.rect.y {
            a.rect.y.partial_cmp(&b.rect.y).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            a.index.cmp(&b.index)
        }
    });

    assign_items_result(items, out_result);
    0
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

