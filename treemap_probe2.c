// Extended probe: more cases to reverse-engineer the reference algorithm.
#include <windows.h>
#include <stdio.h>
#include <stdint.h>

typedef struct { double x, y, w, h; } BtRect;
typedef struct { int64_t index; BtRect rect; } BtTreemapItem;
typedef struct { BtTreemapItem *items; int64_t items_len; int64_t items_cap; } BtLayoutTreemapResult;
typedef int (*bt_layout_treemap_t)(const int64_t *sizes_ptr, int64_t sizes_len, BtRect rect, BtLayoutTreemapResult *out);
typedef void (*bt_release_t)(BtLayoutTreemapResult *buf);

static bt_layout_treemap_t LAYOUT;
static bt_release_t RELEASE;

static void run(const char *tag, int64_t *sizes, int64_t n, BtRect rect) {
    BtLayoutTreemapResult res = {0,0,0};
    printf("\n=== %s: n=%lld rect=(%.0f,%.0f,%.0f,%.0f) sizes=", tag, (long long)n, rect.x, rect.y, rect.w, rect.h);
    for (int64_t i=0;i<n;i++) printf("%lld%s", (long long)sizes[i], i+1<n?",":"");
    printf(" ===\n");
    int rc = LAYOUT(sizes, n, rect, &res);
    printf("rc=%d len=%lld\n", rc, (long long)res.items_len);
    if (rc == 0) {
        for (int64_t i=0;i<res.items_len;i++){
            BtTreemapItem *it=&res.items[i];
            printf("  idx=%lld x=%.4f y=%.4f w=%.4f h=%.4f area=%.4f\n",
                (long long)it->index, it->rect.x, it->rect.y, it->rect.w, it->rect.h, it->rect.w*it->rect.h);
        }
    }
    if (RELEASE) RELEASE(&res);
}

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr,"usage: %s <dll>\n", argv[0]); return 2; }
    setvbuf(stdout, NULL, _IONBF, 0);
    HMODULE h = LoadLibraryA(argv[1]);
    if (!h) { fprintf(stderr,"LoadLibrary failed: %lu\n", GetLastError()); return 3; }
    LAYOUT = (bt_layout_treemap_t)GetProcAddress(h, "bt_layout_treemap");
    RELEASE = (bt_release_t)GetProcAddress(h, "bt_release_layout_treemap");
    if (!LAYOUT) { fprintf(stderr,"no layout\n"); return 4; }
    printf("layout=%p release=%p\n",(void*)LAYOUT,(void*)RELEASE);

    BtRect r;

    // G1: 6 nodes landscape (baseline)
    { int64_t s[]={60,30,20,15,10,5}; r=(BtRect){0,0,1000,600}; run("G1 6-node landscape",s,6,r); }

    // G2: 4 nodes square — already shows binary split
    { int64_t s[]={40,30,20,10}; r=(BtRect){0,0,100,100}; run("G2 4-node square",s,4,r); }

    // G3: 3 nodes square
    { int64_t s[]={50,30,20}; r=(BtRect){0,0,100,100}; run("G3 3-node square",s,3,r); }

    // G4: 2 nodes — simple split along longer side?
    { int64_t s[]={70,30}; r=(BtRect){0,0,100,100}; run("G4 2-node square",s,2,r); }

    // G5: 2 nodes landscape (w>h) — split vertical or horizontal?
    { int64_t s[]={70,30}; r=(BtRect){0,0,200,100}; run("G5 2-node landscape",s,2,r); }

    // G6: 2 nodes portrait (h>w)
    { int64_t s[]={70,30}; r=(BtRect){0,0,100,200}; run("G6 2-node portrait",s,2,r); }

    // G7: equal sizes, 4 nodes
    { int64_t s[]={25,25,25,25}; r=(BtRect){0,0,100,100}; run("G7 4 equal",s,4,r); }

    // G8: equal sizes, 3 nodes
    { int64_t s[]={33,33,34}; r=(BtRect){0,0,100,100}; run("G8 3 ~equal",s,3,r); }

    // G9: one huge + 3 tiny
    { int64_t s[]={100,1,1,1}; r=(BtRect){0,0,100,100}; run("G9 huge+3tiny",s,4,r); }

    // G10: one huge + 2 tiny (3 nodes)
    { int64_t s[]={100,1,1}; r=(BtRect){0,0,100,100}; run("G10 huge+2tiny",s,3,r); }

    // G11: 8 nodes (powers of 2)
    { int64_t s[]={128,64,32,16,8,4,2,1}; r=(BtRect){0,0,256,256}; run("G11 8 powers-of-2",s,8,r); }

    // G12: 5 nodes
    { int64_t s[]={50,20,15,10,5}; r=(BtRect){0,0,100,100}; run("G12 5-node",s,5,r); }

    // G13: sizes not pre-sorted descending (input order 30,60,15,20,10,5)
    { int64_t s[]={30,60,15,20,10,5}; r=(BtRect){0,0,1000,600}; run("G13 unsorted input",s,6,r); }

    // G14: contains a zero
    { int64_t s[]={60,0,30,20}; r=(BtRect){0,0,100,100}; run("G14 has zero",s,4,r); }

    // G15: contains negatives
    { int64_t s[]={60,-10,30,20}; r=(BtRect){0,0,100,100}; run("G15 has negative",s,4,r); }

    // G16: all zero
    { int64_t s[]={0,0,0}; r=(BtRect){0,0,100,100}; run("G16 all zero",s,3,r); }

    // G17: single node
    { int64_t s[]={42}; r=(BtRect){0,0,100,100}; run("G17 single",s,1,r); }

    // G18: offset origin
    { int64_t s[]={40,30,20,10}; r=(BtRect){10,20,100,100}; run("G18 offset origin",s,4,r); }

    // G19: 7 nodes
    { int64_t s[]={70,40,25,15,10,5,2}; r=(BtRect){0,0,200,100}; run("G19 7-node landscape",s,7,r); }

    // G20: 10 nodes
    { int64_t s[]={100,80,60,45,35,25,20,15,10,5}; r=(BtRect){0,0,300,200}; run("G20 10-node",s,10,r); }

    printf("\nALL DONE\n");
    FreeLibrary(h);
    return 0;
}
