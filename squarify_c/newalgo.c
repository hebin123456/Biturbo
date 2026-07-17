// New algorithm matching reference biturbo.dll, derived from disassembly +
// controlled wine sampling. Same ABI as bt_layout_treemap for direct diff.
//
// Key findings (vs standard squarify):
//   - sum excludes the smallest (last) node: sum_excl = total - size[last]
//   - frac0 = size[0] / sum_excl  (FIXED, uses only the largest node)
//   - accumulated fraction acc = n * frac0  (n = row count), NOT sum(sizes)/sum
//   - aspect(n) = n^2 * long * frac0 / short
//   - worst(n) = max(aspect, 1/aspect); break row when worst(n+1) > worst(n)
//   - thickness = n * frac0 * long  (along long side)
//   - items tile along short side, proportional to actual sizes
//   - recurse on remaining nodes + remaining rect
//   - count < 3 base case: 1 fills, 2 splits along longer side by size ratio
#include <windows.h>
#include <stdio.h>
#include <stdint.h>

typedef struct { double x, y, w, h; } BtRect;
typedef struct { int64_t index; BtRect rect; } BtTreemapItem;
typedef struct { BtTreemapItem *items; int64_t items_len; int64_t items_cap; } BtLayoutTreemapResult;

typedef struct { int64_t index; double size; } Node;

// Stable descending sort: equal sizes keep original index order (ascending).
// The reference DLL preserves input order for equal-sized nodes.
static int cmp_node_desc(const void *a, const void *b) {
    const Node *na = a, *nb = b;
    if (na->size < nb->size) return 1;
    if (na->size > nb->size) return -1;
    // equal size -> stable by original index ascending
    if (na->index < nb->index) return -1;
    if (na->index > nb->index) return 1;
    return 0;
}

typedef struct {
    BtTreemapItem *arr;
    int64_t len, cap;
} OutBuf;

static void emit(OutBuf *o, int64_t index, BtRect r) {
    if (o->len >= o->cap) {
        int64_t nc = o->cap * 2 + 8;
        BtTreemapItem *na = (BtTreemapItem*)(o->arr ? HeapReAlloc(GetProcessHeap(), 0, o->arr, nc * sizeof(BtTreemapItem))
                                                     : HeapAlloc(GetProcessHeap(), 0, nc * sizeof(BtTreemapItem)));
        if (!na) return;
        o->arr = na; o->cap = nc;
    }
    o->arr[o->len].index = index;
    o->arr[o->len].rect = r;
    o->len++;
}

static double worst_ratio(int n, double a1) {
    double aspect = (double)n * (double)n * a1;
    if (aspect <= 0.0) return 1e300;
    double inv = 1.0 / aspect;
    return aspect > inv ? aspect : inv;
}

// Recursive layout. nodes sorted desc. [start, n)
static void layout_rec(Node *nodes, int64_t start, int64_t n, BtRect rect, OutBuf *out) {
    if (n <= 0) return;
    if (n == 1) {
        emit(out, nodes[start].index, rect);
        return;
    }
    if (n == 2) {
        double s0 = nodes[start].size, s1 = nodes[start+1].size;
        double denom = s0 + s1;
        double frac = denom > 0.0 ? s0 / denom : 0.5;
        // Reference uses strict w>h for the 2-node split direction: when
        // w==h it splits along h (horizontal bands), not w.
        if (rect.w > rect.h) {
            double w0 = rect.w * frac;
            emit(out, nodes[start].index,   (BtRect){rect.x, rect.y, w0, rect.h});
            emit(out, nodes[start+1].index, (BtRect){rect.x + w0, rect.y, rect.w - w0, rect.h});
        } else {
            double h0 = rect.h * frac;
            emit(out, nodes[start].index,   (BtRect){rect.x, rect.y, rect.w, h0});
            emit(out, nodes[start+1].index, (BtRect){rect.x, rect.y + h0, rect.w, rect.h - h0});
        }
        return;
    }
    // n >= 3
    double total = 0.0;
    for (int64_t i = start; i < start + n; i++) total += nodes[i].size;
    double sum_excl = total - nodes[start + n - 1].size;  // exclude smallest
    if (sum_excl <= 0.0) {
        // Degenerate: emit 1x1 placeholders
        for (int64_t i = start; i < start + n; i++)
            emit(out, nodes[i].index, (BtRect){rect.x, rect.y, 1.0, 1.0});
        return;
    }
    double frac0 = nodes[start].size / sum_excl;
    // orient
    double w = rect.w, h = rect.h;
    double longd, shortd;
    int long_is_w;
    if (h <= w) { longd = w; shortd = h; long_is_w = 1; }
    else        { longd = h; shortd = w; long_is_w = 0; }
    if (shortd <= 0.0 || frac0 <= 0.0) {
        for (int64_t i = start; i < start + n; i++)
            emit(out, nodes[i].index, (BtRect){rect.x, rect.y, 1.0, 1.0});
        return;
    }
    double a1 = longd * frac0 / shortd;
    // find row count: smallest n where worst(n+1) > worst(n), or n reaches count
    int64_t row_n = 1;
    while (row_n < n) {
        double wcur  = worst_ratio((int)row_n, a1);
        double wnext = worst_ratio((int)row_n + 1, a1);
        if (wnext > wcur) break;
        row_n++;
    }
    double thickness = (double)row_n * frac0 * longd;
    // Flip detection: when the row has >=2 nodes and the thickness reaches or
    // exceeds the short side (happens exactly at a1==0.5, n==2, where
    // thickness == short), the reference swaps the item-tiling axis: items
    // tile along the LONG side instead of the SHORT side. The remaining rect
    // still shrinks along the long side by `thickness`.
    int flip = (row_n >= 2 && !long_is_w && thickness >= shortd);
    int layout_long_is_w = flip ? !long_is_w : long_is_w;
    // emit row [start, start+row_n) proportional to sizes
    double row_sum = 0.0;
    for (int64_t i = start; i < start + row_n; i++) row_sum += nodes[i].size;
    if (row_sum <= 0.0) row_sum = 1.0;
    double offset = 0.0;
    for (int64_t i = start; i < start + row_n; i++) {
        double item_len = shortd * nodes[i].size / row_sum;
        BtRect r;
        if (layout_long_is_w) {
            // thickness along w, items tile along h
            r = (BtRect){rect.x, rect.y + offset, thickness, item_len};
        } else {
            // thickness along h, items tile along w
            r = (BtRect){rect.x + offset, rect.y, item_len, thickness};
        }
        emit(out, nodes[i].index, r);
        offset += item_len;
    }
    // remaining rect: always shrinks along the long side by thickness
    BtRect rem;
    if (long_is_w) {
        rem = (BtRect){rect.x + thickness, rect.y, rect.w - thickness, rect.h};
    } else {
        rem = (BtRect){rect.x, rect.y + thickness, rect.w, rect.h - thickness};
    }
    layout_rec(nodes, start + row_n, n - row_n, rem, out);
}

__declspec(dllexport)
int bt_layout_treemap(const int64_t *sizes_ptr, int64_t sizes_len, BtRect rect, BtLayoutTreemapResult *out) {
    out->items = NULL; out->items_len = 0; out->items_cap = 0;
    if (!sizes_ptr || sizes_len <= 0) return 0;
    Node *nodes = (Node*)HeapAlloc(GetProcessHeap(), 0, sizes_len * sizeof(Node));
    if (!nodes) return 1;
    for (int64_t i = 0; i < sizes_len; i++) {
        nodes[i].index = i;
        nodes[i].size = sizes_ptr[i] > 0 ? (double)sizes_ptr[i] : 0.0;
    }
    qsort(nodes, sizes_len, sizeof(Node), cmp_node_desc);
    OutBuf out_buf = {0, 0, 0};
    layout_rec(nodes, 0, sizes_len, rect, &out_buf);
    HeapFree(GetProcessHeap(), 0, nodes);
    out->items = out_buf.arr;
    out->items_len = out_buf.len;
    out->items_cap = out_buf.len;
    return 0;
}

__declspec(dllexport)
void bt_release_layout_treemap(BtLayoutTreemapResult *buf) {
    if (!buf || buf->items_cap == 0) return;
    HeapFree(GetProcessHeap(), 0, buf->items);
    buf->items = NULL; buf->items_len = 0; buf->items_cap = 0;
}

BOOL APIENTRY DllMain(HMODULE h, DWORD r, LPVOID v) { return TRUE; }
