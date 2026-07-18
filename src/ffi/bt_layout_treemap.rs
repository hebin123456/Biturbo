//! # 矩形 Treemap 布局
//!
//! 提供 [`bt_layout_treemap`]：把一组带“尺寸”的节点铺到给定矩形中，
//! 返回每个节点对应的子矩形。算法刻意复刻原版 `biturbo.dll` 的行为，
//! 包括把 `i64` 尺寸按无符号 `u64` 转 `f64`、特定边界情况下的占位输出等。
//!
//! # 错误码
//! 与其他模块一致：`0` 成功、`1` 失败（参数非法 / panic / 内存不足）。

use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::heap_alloc;
use std::os::raw::c_int;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// 二维矩形（左上角 + 宽高）。
///
/// # 字段
/// - `x` / `y`：左上角坐标。
/// - `w` / `h`：宽 / 高。
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BtRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// 单个 Treemap 输出条目。
///
/// # 字段
/// - `index`：原始输入数组中的下标（用于把矩形映射回原始节点）。
/// - `rect`：该节点被分配到的子矩形。
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BtTreemapItem {
    pub index: i64,
    pub rect: BtRect,
}

/// Treemap 布局结果。
///
/// # 内存所有权
/// `items` 通过进程堆分配，必须用
/// [`crate::ffi::bt_release_vec::bt_release_layout_treemap`] 释放。
#[repr(C)]
pub struct BtLayoutTreemapResult {
    pub items: *mut BtTreemapItem,
    pub items_len: i64,
    pub items_cap: i64,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    index: i64,
    // The reference DLL converts the incoming i64 bits through an unsigned
    // integer-to-f64 path. This preserves normal positive sizes and also
    // matches the legacy behavior for negative inputs.
    size: f64,
}

/// 把一组尺寸铺到给定矩形，返回每个节点对应的子矩形。
///
/// 内部会捕获 panic 并返回 `1`，避免 C 侧因 Rust panic 而崩溃。
/// 输入按原版 DLL 约定：负数尺寸按 `u64` 重解释为 `f64`。
///
/// # 参数
/// - `sizes_ptr` / `sizes_len`：每个节点的尺寸（`i64` 数组，按无符号解释）。
/// - `rect`：目标矩形。
/// - `out_result`：输出 [`BtLayoutTreemapResult`]，调用前可未初始化。
///
/// # 返回值
/// - `0`：成功。
/// - `1`：参数非法、内部 panic 或内存不足。
///
/// # 内存所有权
/// 输出的 `items` 数组通过进程堆分配，必须用
/// [`crate::ffi::bt_release_vec::bt_release_layout_treemap`] 释放。
#[no_mangle]
pub unsafe extern "C" fn bt_layout_treemap(
    sizes_ptr: *const i64,
    sizes_len: i64,
    rect: BtRect,
    out_result: *mut BtLayoutTreemapResult,
) -> c_int {
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        bt_layout_treemap_impl_ffi(sizes_ptr, sizes_len, rect, out_result)
    })) {
        Ok(code) => code,
        Err(_) => {
            set_last_error_str("bt_layout_treemap panicked");
            if !out_result.is_null() {
                unsafe {
                    (*out_result).items = core::ptr::null_mut();
                    (*out_result).items_len = 0;
                    (*out_result).items_cap = 0;
                }
            }
            1
        }
    }
}

unsafe fn bt_layout_treemap_impl_ffi(
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
    unsafe { assign_items_result(items, out_result) }
}

/// Pure layout logic, separated from the FFI allocation so it can be unit
/// tested on any platform.
///
/// Matches the reference `biturbo.dll`, including its legacy treemap quirks:
/// sizes are converted as unsigned 64-bit values, and the recursive squarify
/// routine intentionally uses the same off-by-one row accumulation behavior.
fn layout_treemap_impl(sizes: &[i64], rect: BtRect) -> Vec<BtTreemapItem> {
    let mut nodes: Vec<Node> = sizes
        .iter()
        .enumerate()
        .map(|(i, &s)| Node {
            index: i as i64,
            size: (s as u64) as f64,
        })
        .collect();

    // Sort descending by size.
    nodes.sort_by(|a, b| b.size.partial_cmp(&a.size).unwrap_or(std::cmp::Ordering::Equal));

    let mut items = Vec::new();

    if nodes.is_empty() {
        return items;
    }

    let total: f64 = nodes.iter().map(|n| n.size).sum();

    if total == 0.0 && nodes.len() >= 3 {
        // The original returns 1x1 placeholders for an all-zero input.
        for n in &nodes {
            items.push(BtTreemapItem {
                index: n.index,
                rect: BtRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            });
        }
        return items;
    }

    layout_legacy_recursive(&nodes, rect, &mut items);

    items
}

fn layout_legacy_recursive(nodes: &[Node], rect: BtRect, items: &mut Vec<BtTreemapItem>) {
    if nodes.is_empty() {
        return;
    }

    if nodes.len() < 3 {
        layout_legacy_simple(nodes, rect, items);
        return;
    }

    let total_without_last: f64 = nodes[..nodes.len() - 1].iter().map(|n| n.size).sum();
    if total_without_last == 0.0 {
        for n in nodes {
            items.push(BtTreemapItem {
                index: n.index,
                rect: BtRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            });
        }
        return;
    }
    if !total_without_last.is_finite() {
        layout_legacy_simple(nodes, rect, items);
        return;
    }

    let first_ratio = nodes[0].size / total_without_last;
    if first_ratio <= 0.0 || !first_ratio.is_finite() {
        layout_legacy_simple(nodes, rect, items);
        return;
    }

    let mut row_ratio = first_ratio;
    let mut row_len = 1usize;
    while row_len < nodes.len() {
        let candidate_ratio = row_ratio + nodes[row_len - 1].size / total_without_last;
        if legacy_aspect(candidate_ratio, first_ratio, rect) > legacy_aspect(row_ratio, first_ratio, rect) {
            break;
        }
        row_ratio = candidate_ratio;
        row_len += 1;
    }

    let mut remaining = rect;
    layout_legacy_row(&nodes[..row_len], row_ratio, &mut remaining, items);
    if row_len < nodes.len() {
        layout_legacy_recursive(&nodes[row_len..], remaining, items);
    }
}

fn legacy_aspect(sum_ratio: f64, first_ratio: f64, rect: BtRect) -> f64 {
    if rect.w <= 0.0 || rect.h <= 0.0 || sum_ratio <= 0.0 || first_ratio <= 0.0 {
        return f64::INFINITY;
    }
    let long_over_short = if rect.h > rect.w { rect.h / rect.w } else { rect.w / rect.h };
    let aspect = long_over_short * sum_ratio * sum_ratio / first_ratio;
    if aspect < 1.0 { 1.0 / aspect } else { aspect }
}

fn layout_legacy_row(row: &[Node], row_ratio: f64, remaining: &mut BtRect, items: &mut Vec<BtTreemapItem>) {
    if row.is_empty() {
        return;
    }
    let row_total: f64 = row.iter().map(|n| n.size).sum();
    if row_total == 0.0 || !row_total.is_finite() {
        return;
    }

    if remaining.w >= remaining.h {
        let strip_w = remaining.w * row_ratio;
        let mut y = remaining.y;
        for n in row {
            let h = remaining.h * n.size / row_total;
            items.push(BtTreemapItem {
                index: n.index,
                rect: BtRect { x: remaining.x, y, w: strip_w, h },
            });
            y += h;
        }
        remaining.x += strip_w;
        remaining.w -= strip_w;
    } else {
        let strip_h = remaining.h * row_ratio;
        let mut x = remaining.x;
        for n in row {
            let w = remaining.w * n.size / row_total;
            items.push(BtTreemapItem {
                index: n.index,
                rect: BtRect { x, y: remaining.y, w, h: strip_h },
            });
            x += w;
        }
        remaining.y += strip_h;
        remaining.h -= strip_h;
    }
}

/// Simple proportional split for fewer than 3 nodes. With one node it fills
/// the whole rect; with two it splits along the longer dimension by size
/// ratio.
fn layout_legacy_simple(nodes: &[Node], rect: BtRect, items: &mut Vec<BtTreemapItem>) {
    if nodes.is_empty() {
        return;
    }
    if nodes.len() == 1 {
        let frac = nodes[0].size / nodes[0].size;
        let rect = if rect.w >= rect.h {
            BtRect { x: rect.x, y: rect.y, w: rect.w * frac, h: rect.h }
        } else {
            BtRect { x: rect.x, y: rect.y, w: rect.w, h: rect.h * frac }
        };
        items.push(BtTreemapItem {
            index: nodes[0].index,
            rect,
        });
        return;
    }
    // Two nodes: split along the longer side.
    let denom = nodes[0].size + nodes[1].size;
    let frac = nodes[0].size / denom;
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

unsafe fn assign_items_result(items: Vec<BtTreemapItem>, out_result: *mut BtLayoutTreemapResult) -> c_int {
    let Some(bytes_len) = items.len().checked_mul(std::mem::size_of::<BtTreemapItem>()) else {
        set_last_error_str("allocation size overflow");
        return 1;
    };
    let ptr = unsafe { heap_alloc(bytes_len) } as *mut BtTreemapItem;
    if ptr.is_null() {
        set_last_error_str("insufficient memory");
        return 1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(items.as_ptr(), ptr, items.len());
        (*out_result).items = ptr;
        (*out_result).items_len = items.len() as i64;
        (*out_result).items_cap = items.len() as i64;
    }
    0
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

    fn assert_item_close(item: &BtTreemapItem, expected: (i64, f64, f64, f64, f64)) {
        let (index, x, y, w, h) = expected;
        assert_eq!(item.index, index);
        assert!((item.rect.x - x).abs() < 1e-6, "x mismatch: {:?}", item);
        assert!((item.rect.y - y).abs() < 1e-6, "y mismatch: {:?}", item);
        assert!((item.rect.w - w).abs() < 1e-6, "w mismatch: {:?}", item);
        assert!((item.rect.h - h).abs() < 1e-6, "h mismatch: {:?}", item);
    }

    #[test]
    fn legacy_layout_matches_reference_landscape() {
        let sizes = vec![60, 30, 20, 15, 10, 5];
        let rect = BtRect { x: 0.0, y: 0.0, w: 1000.0, h: 600.0 };
        let items = call(&sizes, rect);
        let expected = [
            (0, 0.0, 0.0, 444.44444444444446, 600.0),
            (1, 444.44444444444446, 0.0, 333.3333333333333, 480.0),
            (2, 777.7777777777778, 0.0, 222.22222222222223, 480.0),
            (3, 444.44444444444446, 480.0, 333.3333333333333, 120.0),
            (4, 777.7777777777778, 480.0, 148.14814814814815, 120.0),
            (5, 925.9259259259259, 480.0, 74.07407407407408, 120.0),
        ];
        assert_eq!(items.len(), expected.len());
        for (item, expected) in items.iter().zip(expected) {
            assert_item_close(item, expected);
        }
    }

    #[test]
    fn legacy_layout_matches_reference_many() {
        let sizes = vec![100, 80, 60, 45, 35, 25, 20, 15, 10, 5, 3, 2];
        let rect = BtRect { x: 0.0, y: 0.0, w: 100.0, h: 60.0 };
        let items = call(&sizes, rect);
        let expected = [
            (0, 0.0, 0.0, 50.251256281407031, 33.333333333333336),
            (1, 0.0, 33.333333333333336, 50.251256281407031, 26.666666666666664),
            (2, 50.251256281407031, 0.0, 28.427853553481697, 33.027522935779821),
            (3, 78.67910983488872, 0.0, 21.320890165111273, 33.027522935779821),
            (4, 50.251256281407031, 33.027522935779821, 15.408902921688087, 26.972477064220183),
            (5, 65.66015920309512, 33.027522935779821, 22.012718459554414, 14.984709480122325),
            (6, 65.66015920309512, 48.012232415902147, 22.012718459554414, 11.987767584097858),
            (7, 87.67287766264954, 33.027522935779821, 12.327122337350469, 12.26021684737281),
            (8, 87.67287766264954, 45.287739783152631, 12.327122337350469, 8.17347789824854),
            (9, 87.67287766264954, 53.461217681401173, 7.7044514608440426, 6.5387823185988312),
            (10, 95.377329123493581, 53.461217681401173, 4.6226708765064259, 3.9232693911592986),
            (11, 95.377329123493581, 57.384487072560475, 4.6226708765064259, 2.6155129274395326),
        ];
        assert_eq!(items.len(), expected.len());
        for (item, expected) in items.iter().zip(expected) {
            assert_item_close(item, expected);
        }
    }

    #[test]
    fn legacy_negative_input_matches_unsigned_conversion() {
        let sizes = vec![-1, 5, 2];
        let rect = BtRect { x: 0.0, y: 0.0, w: 100.0, h: 60.0 };
        let items = call(&sizes, rect);
        assert_eq!(items.len(), sizes.len());
        assert_eq!(items[0].index, 0);
        assert!((items[0].rect.w - 100.0).abs() < 1e-12);
    }

    #[test]
    fn legacy_zero_values_match_reference() {
        let rect = BtRect { x: 0.0, y: 0.0, w: 100.0, h: 60.0 };

        let one_zero = call(&[0], rect);
        assert_eq!(one_zero.len(), 1);
        assert_eq!(one_zero[0].index, 0);
        assert!(one_zero[0].rect.w.is_nan());
        assert_eq!(one_zero[0].rect.h, 60.0);

        let three_zeros = call(&[0, 0, 0], BtRect { x: 10.0, y: 20.0, w: 100.0, h: 60.0 });
        assert_eq!(three_zeros.len(), 3);
        for item in three_zeros {
            assert_eq!(item.rect, BtRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        }

        let mixed = call(&[0, 1, 0, 1, 0, 1], rect);
        let expected = [
            (1, 0.0, 0.0, 33.333333333333329, 60.0),
            (3, 33.333333333333329, 0.0, 33.333333333333336, 60.0),
            (5, 66.666666666666657, 0.0, 33.333333333333336, 60.0),
            (0, 0.0, 0.0, 1.0, 1.0),
            (2, 0.0, 0.0, 1.0, 1.0),
            (4, 0.0, 0.0, 1.0, 1.0),
        ];
        assert_eq!(mixed.len(), expected.len());
        for (item, expected) in mixed.iter().zip(expected) {
            assert_item_close(item, expected);
        }
    }

    #[test]
    fn empty_input_returns_empty() {
        // 空输入应返回空 Vec
        let items = call(&[], BtRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 });
        assert!(items.is_empty());
    }

    #[test]
    fn single_node_fills_rect() {
        // 单节点应填满整个矩形（宽度方向）
        let items = call(&[42], BtRect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 });
        assert_eq!(items.len(), 1);
        // 单节点 fill 整个 rect，w=100, h=50（沿较长边）
        assert_item_close(&items[0], (0, 0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn single_node_tall_rect_fills_height() {
        // 高大于宽时，单节点应沿高度方向填充
        let items = call(&[42], BtRect { x: 0.0, y: 0.0, w: 50.0, h: 100.0 });
        assert_eq!(items.len(), 1);
        assert_item_close(&items[0], (0, 0.0, 0.0, 50.0, 100.0));
    }

    #[test]
    fn two_nodes_split_along_longer_dimension() {
        // 两个节点沿较长边按比例分割。squarify 会先按 size 降序排序，
        // 因此 items[0] 对应 size=70（最大），items[1] 对应 size=30。
        let items = call(&[30, 70], BtRect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 });
        assert_eq!(items.len(), 2);
        // 总和 100，比例为 70/100 和 30/100
        // 第一节点（size=70）占 70% 宽度
        assert!((items[0].rect.w - 70.0).abs() < 1e-6, "w0 应为 70: {:?}", items[0].rect);
        // 第二节点（size=30）占 30% 宽度
        assert!((items[1].rect.w - 30.0).abs() < 1e-6, "w1 应为 30: {:?}", items[1].rect);
    }

    #[test]
    fn two_nodes_tall_rect_split_along_height() {
        // 高度大于宽度时，两节点沿高度方向分割。
        // squarify 按 size 降序：items[0] 对应 size=70，items[1] 对应 size=30。
        let items = call(&[30, 70], BtRect { x: 0.0, y: 0.0, w: 50.0, h: 100.0 });
        assert_eq!(items.len(), 2);
        assert!((items[0].rect.h - 70.0).abs() < 1e-6, "h0 应为 70: {:?}", items[0].rect);
        assert!((items[1].rect.h - 30.0).abs() < 1e-6, "h1 应为 30: {:?}", items[1].rect);
    }

    #[test]
    fn sorted_descending_by_size() {
        // 输入乱序，输出 items 应按尺寸降序排列
        let items = call(&[5, 100, 50, 1, 25], BtRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 });
        // 第一项应是最大的 size=100，对应原 index=1
        assert_eq!(items[0].index, 1);
    }

    #[test]
    fn negative_size_treated_as_huge_unsigned() {
        // 负数 i64 按 u64 解释为巨大正数，应排在最前
        let items = call(&[-1, 100, 50], BtRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 });
        // -1 as u64 是 u64::MAX，是最大值，对应原 index=0
        assert_eq!(items[0].index, 0);
    }

    #[test]
    fn aspect_helper_returns_infinity_for_zero_rect() {
        // 零尺寸矩形应返回 INFINITY
        let r = BtRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };
        assert!(legacy_aspect(1.0, 1.0, r).is_infinite());
    }

    #[test]
    fn aspect_helper_returns_infinity_for_zero_ratio() {
        let r = BtRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        assert!(legacy_aspect(0.0, 1.0, r).is_infinite());
        assert!(legacy_aspect(1.0, 0.0, r).is_infinite());
    }

    #[test]
    fn aspect_helper_returns_finite_for_valid_input() {
        let r = BtRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let a = legacy_aspect(1.0, 1.0, r);
        assert!(a.is_finite(), "应有限: {a}");
        assert!(a >= 1.0, "返回值应 >= 1.0: {a}");
    }

    #[test]
    fn all_zero_with_two_nodes_returns_nan_for_one() {
        // [0, 0] 两节点：denom=0 会产生 NaN
        let items = call(&[0, 0], BtRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 });
        assert_eq!(items.len(), 2);
        // 不应 panic，结果可能是 NaN 但仍返回
    }

    #[test]
    fn large_input_count_returns_one_per_node() {
        // 大量节点应正确返回每个一个 item
        let sizes: Vec<i64> = (1..=20).collect();
        let items = call(&sizes, BtRect { x: 0.0, y: 0.0, w: 1000.0, h: 1000.0 });
        assert_eq!(items.len(), 20);
    }

    #[test]
    fn all_indices_present_in_output() {
        // 所有输入索引都应在输出中恰好出现一次
        let sizes = vec![10, 20, 30, 40, 50];
        let items = call(&sizes, BtRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 });
        let mut indices: Vec<i64> = items.iter().map(|i| i.index).collect();
        indices.sort();
        assert_eq!(indices, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn square_rect_layout_completes() {
        // 正方形矩形布局应完成且每个 item 矩形非负
        let items = call(&[15, 25, 35, 45, 10, 5], BtRect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 });
        for item in &items {
            assert!(item.rect.w >= 0.0 || item.rect.w.is_nan(), "w 不应为负: {:?}", item);
            assert!(item.rect.h >= 0.0 || item.rect.h.is_nan(), "h 不应为负: {:?}", item);
        }
    }

    #[test]
    fn non_zero_origin_rect_preserved() {
        // 非零起点矩形的 items 应落在矩形内或附近
        let items = call(&[100, 50], BtRect { x: 10.0, y: 20.0, w: 100.0, h: 50.0 });
        assert_eq!(items.len(), 2);
        // 至少一个 item 的 x 应 >= 10（原点偏移生效）
        assert!(items.iter().all(|i| i.rect.x >= 10.0 - 1e-6));
    }
}

