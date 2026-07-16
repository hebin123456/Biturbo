// Edge-case probe: stress the reference DLL to find crash boundaries and
// exact algorithm behavior on tricky inputs.
#include <windows.h>
#include <stdio.h>
#include <stdint.h>

typedef struct { double x, y, w, h; } BtRect;
typedef struct { int64_t index; BtRect rect; } BtTreemapItem;
typedef struct { BtTreemapItem *items; int64_t items_len; int64_t items_cap; } BtLayoutTreemapResult;
typedef int (*bt_layout_treemap_t)(const int64_t*, int64_t, BtRect, BtLayoutTreemapResult*);
typedef void (*bt_release_t)(BtLayoutTreemapResult*);
static bt_layout_treemap_t L; static bt_release_t R;

static void run(const char *tag, int64_t *s, int64_t n, BtRect r) {
    BtLayoutTreemapResult res={0,0,0};
    printf("\n=== %s n=%lld ===\n", tag, (long long)n);
    int rc = L(s, n, r, &res);
    printf("rc=%d len=%lld\n", rc, (long long)res.items_len);
    if (rc==0 && res.items_len>0 && res.items_len<50) {
        for (int64_t i=0;i<res.items_len;i++)
            printf("  idx=%lld x=%.4f y=%.4f w=%.4f h=%.4f\n",(long long)res.items[i].index,res.items[i].rect.x,res.items[i].rect.y,res.items[i].rect.w,res.items[i].rect.h);
    }
    if (R && res.items_cap>0) R(&res);
}

int main(int argc, char**argv){
    if(argc<2){fprintf(stderr,"usage: %s <dll>\n",argv[0]);return 2;}
    setvbuf(stdout,NULL,_IONBF,0);
    HMODULE h=LoadLibraryA(argv[1]); if(!h){fprintf(stderr,"load fail\n");return 3;}
    L=(bt_layout_treemap_t)GetProcAddress(h,"bt_layout_treemap");
    R=(bt_release_t)GetProcAddress(h,"bt_release_layout_treemap");
    if(!L){fprintf(stderr,"no layout\n");return 4;}

    // E1: very large i64 values (overflow test)
    { int64_t s[]={9223372036854775806LL,9223372036854775806LL,1}; BtRect r={0,0,100,100}; run("E1 two near-MAX +1",s,3,r); }
    // E2: i64::MAX/2+1 twice (the original crash repro)
    { int64_t s[]={4611686018427387905LL,4611686018427387905LL,0}; BtRect r={0,0,100,100}; run("E2 MAX/2+1 twice +0",s,3,r); }
    // E3: huge + many tiny
    { int64_t s[]={9223372036854775800LL,1,1,1,1}; BtRect r={0,0,100,100}; run("E3 huge+4tiny",s,5,r); }
    // E4: all same large
    { int64_t s[]={1000000000,1000000000,1000000000,1000000000}; BtRect r={0,0,100,100}; run("E4 4 same large",s,4,r); }
    // E5: negative only
    { int64_t s[]={-5,-10,-3}; BtRect r={0,0,100,100}; run("E5 all negative",s,3,r); }
    // E6: mix pos/neg/zero
    { int64_t s[]={60,-10,0,30}; BtRect r={0,0,100,100}; run("E6 pos/neg/zero",s,4,r); }
    // E7: zero rect
    { int64_t s[]={50,30,20}; BtRect r={0,0,0,0}; run("E7 zero rect",s,3,r); }
    // E8: negative rect dims
    { int64_t s[]={50,30,20}; BtRect r={0,0,-100,100}; run("E8 neg w",s,3,r); }
    // E9: huge count
    { int64_t s[200]; for(int i=0;i<200;i++) s[i]=200-i; BtRect r={0,0,1000,1000}; run("E9 200 nodes",s,200,r); }
    // E10: 1 node
    { int64_t s[]={100}; BtRect r={0,0,100,100}; run("E10 single",s,1,r); }
    // E11: 2 nodes equal
    { int64_t s[]={50,50}; BtRect r={0,0,100,100}; run("E11 two equal",s,2,r); }
    // E12: tiny canvas
    { int64_t s[]={60,30,20}; BtRect r={0,0,0.001,0.001}; run("E12 tiny canvas",s,3,r); }
    // E13: sizes_len = 0 (should return 0, no crash)
    { int64_t s[]={1}; BtRect r={0,0,100,100}; printf("\n=== E13 len=0 ===\n"); BtLayoutTreemapResult res={0,0,0}; int rc=L(s,0,r,&res); printf("rc=%d len=%lld\n",rc,(long long)res.items_len); }
    // E14: same as G2 but verify exact: 40,30,20,10
    { int64_t s[]={40,30,20,10}; BtRect r={0,0,100,100}; run("E14 G2 repeat",s,4,r); }
    // E15: 3 nodes portrait
    { int64_t s[]={50,30,20}; BtRect r={0,0,100,300}; run("E15 3 portrait",s,3,r); }

    printf("\nALL DONE\n");
    FreeLibrary(h);
    return 0;
}
