// Probe harness: LoadLibrary the biturbo.dll under test, call
// bt_layout_treemap with a fixed input, print each item's index + rect, then
// release. Compiled with mingw-w64, run under wine.
//
// Usage: probe.exe <path-to-biturbo.dll>
#include <windows.h>
#include <stdio.h>
#include <stdint.h>

typedef struct { double x, y, w, h; } BtRect;
typedef struct { int64_t index; BtRect rect; } BtTreemapItem;
typedef struct { BtTreemapItem *items; int64_t items_len; int64_t items_cap; } BtLayoutTreemapResult;

typedef int (*bt_layout_treemap_t)(const int64_t *sizes_ptr, int64_t sizes_len,
                                   BtRect rect, BtLayoutTreemapResult *out_result);
typedef void (*bt_release_layout_treemap_t)(BtLayoutTreemapResult *buf);

static void dump(const char *tag, BtLayoutTreemapResult *r) {
    printf("[%s] len=%lld cap=%lld\n", tag, (long long)r->items_len, (long long)r->items_cap);
    for (int64_t i = 0; i < r->items_len; i++) {
        BtTreemapItem *it = &r->items[i];
        printf("  idx=%lld x=%.4f y=%.4f w=%.4f h=%.4f area=%.4f\n",
               (long long)it->index, it->rect.x, it->rect.y, it->rect.w, it->rect.h,
               it->rect.w * it->rect.h);
    }
}

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: %s <biturbo.dll>\n", argv[0]); return 2; }
    // Force stderr/stdout unbuffered so we see output before any crash.
    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stderr, NULL, _IONBF, 0);

    HMODULE h = LoadLibraryA(argv[1]);
    if (!h) { fprintf(stderr, "LoadLibrary failed: %lu\n", GetLastError()); return 3; }
    printf("loaded %s at %p\n", argv[1], (void*)h);

    bt_layout_treemap_t layout = (bt_layout_treemap_t)GetProcAddress(h, "bt_layout_treemap");
    bt_release_layout_treemap_t release = (bt_release_layout_treemap_t)GetProcAddress(h, "bt_release_layout_treemap");
    if (!layout) { fprintf(stderr, "no bt_layout_treemap\n"); return 4; }
    if (!release) { fprintf(stderr, "no bt_release_layout_treemap (will skip release)\n"); }
    printf("layout=%p release=%p\n", (void*)layout, (void*)release);

    // Case A: 6 nodes, landscape canvas (the squarify paper-ish example).
    {
        int64_t sizes[] = {60, 30, 20, 15, 10, 5};
        int64_t n = sizeof(sizes)/sizeof(sizes[0]);
        BtRect rect = {0.0, 0.0, 1000.0, 600.0};
        BtLayoutTreemapResult res = {0,0,0};
        printf("\n=== Case A: sizes={60,30,20,15,10,5} rect=1000x600 ===\n");
        int rc = layout(sizes, n, rect, &res);
        printf("rc=%d\n", rc);
        if (rc == 0) dump("A", &res);
        if (release) release(&res);
    }

    // Case B: 4 nodes, square canvas.
    {
        int64_t sizes[] = {40, 30, 20, 10};
        int64_t n = 4;
        BtRect rect = {0.0, 0.0, 100.0, 100.0};
        BtLayoutTreemapResult res = {0,0,0};
        printf("\n=== Case B: sizes={40,30,20,10} rect=100x100 ===\n");
        int rc = layout(sizes, n, rect, &res);
        printf("rc=%d\n", rc);
        if (rc == 0) dump("B", &res);
        if (release) release(&res);
    }

    // Case C: 3 nodes (squarify threshold).
    {
        int64_t sizes[] = {50, 30, 20};
        int64_t n = 3;
        BtRect rect = {0.0, 0.0, 100.0, 100.0};
        BtLayoutTreemapResult res = {0,0,0};
        printf("\n=== Case C: sizes={50,30,20} rect=100x100 ===\n");
        int rc = layout(sizes, n, rect, &res);
        printf("rc=%d\n", rc);
        if (rc == 0) dump("C", &res);
        if (release) release(&res);
    }

    // Case D: 2 nodes (simple split path).
    {
        int64_t sizes[] = {70, 30};
        int64_t n = 2;
        BtRect rect = {0.0, 0.0, 100.0, 100.0};
        BtLayoutTreemapResult res = {0,0,0};
        printf("\n=== Case D: sizes={70,30} rect=100x100 ===\n");
        int rc = layout(sizes, n, rect, &res);
        printf("rc=%d\n", rc);
        if (rc == 0) dump("D", &res);
        if (release) release(&res);
    }

    // Case E: 1 node.
    {
        int64_t sizes[] = {42};
        int64_t n = 1;
        BtRect rect = {5.0, 6.0, 100.0, 100.0};
        BtLayoutTreemapResult res = {0,0,0};
        printf("\n=== Case E: sizes={42} rect=(5,6,100,100) ===\n");
        int rc = layout(sizes, n, rect, &res);
        printf("rc=%d\n", rc);
        if (rc == 0) dump("E", &res);
        if (release) release(&res);
    }

    // Case F: empty.
    {
        int64_t sizes[] = {0};
        BtRect rect = {0.0, 0.0, 100.0, 100.0};
        BtLayoutTreemapResult res = {0,0,0};
        printf("\n=== Case F: sizes={0} ===\n");
        int rc = layout(sizes, 1, rect, &res);
        printf("rc=%d len=%lld\n", rc, (long long)res.items_len);
        if (release && res.items) release(&res);
    }

    printf("\nALL DONE\n");
    FreeLibrary(h);
    return 0;
}
