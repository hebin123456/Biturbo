// C implementation of standard squarify, built as a DLL with the same ABI as
// bt_layout_treemap, so probe.exe can load it and we can diff against the
// reference DLL under wine. This is a throwaway verification harness.
#include <windows.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>

typedef struct { double x, y, w, h; } BtRect;
typedef struct { int64_t index; BtRect rect; } BtTreemapItem;
typedef struct { BtTreemapItem *items; int64_t items_len; int64_t items_cap; } BtLayoutTreemapResult;

typedef struct { int64_t index; double size; } Node;

static int cmp_node_desc(const void *a, const void *b) {
    double da = ((const Node*)a)->size, db = ((const Node*)b)->size;
    if (da < db) return 1;
    if (da > db) return -1;
    return 0;
}

static double worst_aspect(Node *nodes, int *row, int rowlen, int candidate, double canvas_area, double total, BtRect *rem) {
    double sum = 0, mn = 1e300, mx = 0;
    for (int i = 0; i < rowlen; i++) {
        double a = canvas_area * nodes[row[i]].size / total;
        sum += a; if (a < mn) mn = a; if (a > mx) mx = a;
    }
    if (candidate >= 0) {
        double a = canvas_area * nodes[candidate].size / total;
        sum += a; if (a < mn) mn = a; if (a > mx) mx = a;
    }
    if (sum <= 0 || mn <= 0) return 1e300;
    double side = rem->w < rem->h ? rem->w : rem->h;
    if (side <= 0) return 1e300;
    double s2 = side * side;
    double v1 = s2 * mx / (sum * sum);
    double v2 = (sum * sum) / (s2 * mn);
    return v1 > v2 ? v1 : v2;
}

static void layout_row(Node *nodes, int *row, int rowlen, double canvas_area, double total, BtRect *rem, BtTreemapItem **out, int *outn, int *outcap) {
    double sum = 0;
    for (int i = 0; i < rowlen; i++) sum += canvas_area * nodes[row[i]].size / total;
    if (sum <= 0 || rowlen == 0) return;
    if (*outn + rowlen > *outcap) {
        *outcap = (*outn + rowlen) * 2 + 8;
        if (*out == NULL) *out = (BtTreemapItem*)HeapAlloc(GetProcessHeap(), 0, *outcap * sizeof(BtTreemapItem));
        else *out = (BtTreemapItem*)HeapReAlloc(GetProcessHeap(), 0, *out, *outcap * sizeof(BtTreemapItem));
    }
    if (rem->w <= rem->h) {
        double t = sum / rem->w;
        double x = rem->x;
        for (int i = 0; i < rowlen; i++) {
            double a = canvas_area * nodes[row[i]].size / total;
            double w = t > 0 ? a / t : 0;
            (*out)[*outn].index = nodes[row[i]].index;
            (*out)[*outn].rect = (BtRect){x, rem->y, w, t};
            (*outn)++; x += w;
        }
        rem->y += t; rem->h -= t;
    } else {
        double t = sum / rem->h;
        double y = rem->y;
        for (int i = 0; i < rowlen; i++) {
            double a = canvas_area * nodes[row[i]].size / total;
            double h = t > 0 ? a / t : 0;
            (*out)[*outn].index = nodes[row[i]].index;
            (*out)[*outn].rect = (BtRect){rem->x, y, t, h};
            (*outn)++; y += h;
        }
        rem->x += t; rem->w -= t;
    }
}

__declspec(dllexport)
int bt_layout_treemap(const int64_t *sizes_ptr, int64_t sizes_len, BtRect rect, BtLayoutTreemapResult *out) {
    out->items = NULL; out->items_len = 0; out->items_cap = 0;
    if (!sizes_ptr || sizes_len <= 0) return 0;
    Node *nodes = (Node*)HeapAlloc(GetProcessHeap(), 0, sizes_len * sizeof(Node));
    for (int64_t i = 0; i < sizes_len; i++) {
        nodes[i].index = i;
        nodes[i].size = sizes_ptr[i] > 0 ? (double)sizes_ptr[i] : 0.0;
    }
    qsort(nodes, sizes_len, sizeof(Node), cmp_node_desc);
    double total = 0;
    for (int64_t i = 0; i < sizes_len; i++) total += nodes[i].size;
    BtTreemapItem *out_arr = NULL; int outn = 0, outcap = 0;
    if (total <= 0) {
        for (int64_t i = 0; i < sizes_len; i++) {
            if (outn >= outcap) { outcap = outn + 8; out_arr = out_arr ? (BtTreemapItem*)HeapReAlloc(GetProcessHeap(), 0, out_arr, outcap*sizeof(BtTreemapItem)) : (BtTreemapItem*)HeapAlloc(GetProcessHeap(), 0, outcap*sizeof(BtTreemapItem)); }
            out_arr[outn].index = nodes[i].index;
            out_arr[outn].rect = (BtRect){rect.x, rect.y, 1.0, 1.0};
            outn++;
        }
    } else if (sizes_len < 3) {
        // simple: 1 node fills, 2 split along longer side
        if (sizes_len == 1) {
            if (outn >= outcap) { outcap = 8; out_arr = (BtTreemapItem*)HeapAlloc(GetProcessHeap(), 0, outcap*sizeof(BtTreemapItem)); }
            out_arr[0].index = nodes[0].index; out_arr[0].rect = rect; outn = 1;
        } else {
            double denom = nodes[0].size + nodes[1].size;
            double frac = denom > 0 ? nodes[0].size / denom : 0.5;
            if (outcap < 2) { outcap = 8; out_arr = (BtTreemapItem*)HeapAlloc(GetProcessHeap(), 0, outcap*sizeof(BtTreemapItem)); }
            if (rect.w >= rect.h) {
                double w0 = rect.w * frac;
                out_arr[0] = (BtTreemapItem){nodes[0].index, {rect.x, rect.y, w0, rect.h}};
                out_arr[1] = (BtTreemapItem){nodes[1].index, {rect.x+w0, rect.y, rect.w-w0, rect.h}};
            } else {
                double h0 = rect.h * frac;
                out_arr[0] = (BtTreemapItem){nodes[0].index, {rect.x, rect.y, rect.w, h0}};
                out_arr[1] = (BtTreemapItem){nodes[1].index, {rect.x, rect.y+h0, rect.w, rect.h-h0}};
            }
            outn = 2;
        }
    } else {
        double canvas_area = rect.w * rect.h;
        BtRect rem = rect;
        int row[256]; int rowlen = 0;
        int64_t i = 0;
        while (i < sizes_len) {
            double cw = worst_aspect(nodes, row, rowlen, (int)i, canvas_area, total, &rem);
            double curw = rowlen == 0 ? 1e300 : worst_aspect(nodes, row, rowlen, -1, canvas_area, total, &rem);
            if (rowlen > 0 && cw > curw) {
                layout_row(nodes, row, rowlen, canvas_area, total, &rem, &out_arr, &outn, &outcap);
                rowlen = 0;
            }
            row[rowlen++] = (int)i;
            i++;
        }
        if (rowlen > 0) layout_row(nodes, row, rowlen, canvas_area, total, &rem, &out_arr, &outn, &outcap);
    }
    HeapFree(GetProcessHeap(), 0, nodes);
    out->items = out_arr; out->items_len = outn; out->items_cap = outn;
    return 0;
}

__declspec(dllexport)
void bt_release_layout_treemap(BtLayoutTreemapResult *buf) {
    if (!buf || buf->items_cap == 0) return;
    HeapFree(GetProcessHeap(), 0, buf->items);
    buf->items = NULL; buf->items_len = 0; buf->items_cap = 0;
}

BOOL APIENTRY DllMain(HMODULE h, DWORD r, LPVOID v) { return TRUE; }
