// Extensive random positive-case validation to confirm algorithm match.
#include <windows.h>
#include <stdio.h>
#include <stdint.h>
#include <math.h>

typedef struct { double x, y, w, h; } BtRect;
typedef struct { int64_t index; BtRect rect; } BtTreemapItem;
typedef struct { BtTreemapItem *items; int64_t len, cap; } Res;
typedef int  (*F)(const int64_t*, int64_t, BtRect, Res*);
typedef void (*Rf)(Res*);
static F f_ref, f_cand; static Rf r_ref, r_cand;
static int total=0, exact=0, diff=0;

static int eq(double a, double b){ double e=fabs(a-b); return e<=1e-6; }

static void run(const char *tag, int64_t *s, int n, BtRect rect) {
    Res r1={0,0,0}, r2={0,0,0};
    f_ref(s,n,rect,&r1); f_cand(s,n,rect,&r2);
    total++;
    int ok = (r1.len==r2.len);
    int64_t len = r1.len<r2.len?r1.len:r2.len;
    double maxerr=0;
    for (int64_t i=0;i<len;i++){
        if (r1.items[i].index != r2.items[i].index) ok=0;
        double errs[4]={fabs(r1.items[i].rect.x-r2.items[i].rect.x),fabs(r1.items[i].rect.y-r2.items[i].rect.y),fabs(r1.items[i].rect.w-r2.items[i].rect.w),fabs(r1.items[i].rect.h-r2.items[i].rect.h)};
        for(int j=0;j<4;j++) if(errs[j]>maxerr) maxerr=errs[j];
        if (!eq(r1.items[i].rect.x,r2.items[i].rect.x)||!eq(r1.items[i].rect.y,r2.items[i].rect.y)||!eq(r1.items[i].rect.w,r2.items[i].rect.w)||!eq(r1.items[i].rect.h,r2.items[i].rect.h)) ok=0;
    }
    if (ok) exact++; else diff++;
    if (!ok) {
        printf("DIFF [%s] maxerr=%.4f len=%lld/%lld\n",tag,maxerr,(long long)r1.len,(long long)r2.len);
        printf("  ref : "); for(int64_t i=0;i<r1.len;i++) printf("idx%lld=(%.3f,%.3f,%.3f,%.3f) ",(long long)r1.items[i].index,r1.items[i].rect.x,r1.items[i].rect.y,r1.items[i].rect.w,r1.items[i].rect.h); printf("\n");
        printf("  cand: "); for(int64_t i=0;i<r2.len;i++) printf("idx%lld=(%.3f,%.3f,%.3f,%.3f) ",(long long)r2.items[i].index,r2.items[i].rect.x,r2.items[i].rect.y,r2.items[i].rect.w,r2.items[i].rect.h); printf("\n");
    }
    r_ref(&r1); r_cand(&r2);
}

int main(int argc,char**argv){
    HMODULE hr=LoadLibraryA(argv[1]), hc=LoadLibraryA(argv[2]);
    f_ref=(F)GetProcAddress(hr,"bt_layout_treemap"); r_ref=(Rf)GetProcAddress(hr,"bt_release_layout_treemap");
    f_cand=(F)GetProcAddress(hc,"bt_layout_treemap"); r_cand=(Rf)GetProcAddress(hc,"bt_release_layout_treemap");
    setvbuf(stdout,NULL,_IONBF,0);

    BtRect rects[] = {
        {0,0,100,100},{0,0,1000,600},{0,0,600,1000},{0,0,300,200},
        {0,0,200,300},{0,0,1000,1000},{0,0,50,200},{0,0,200,50},
        {10,20,300,400},{0,0,1,1},{0,0,10000,1},{0,0,1,10000}
    };
    int nrects = sizeof(rects)/sizeof(rects[0]);

    // Systematic: various size distributions x rects
    int64_t cases[][16] = {
        {40,30,20,10}, {60,30,20,15,10,5}, {25,25,25,25},
        {100,80,60,40,30,20,15,10,5,2}, {50,40,30,25,20,15,12,10,8,5,3,1},
        {30,25,20,15,10,5,4,3,2,1}, {1000,1,1,1,1,1}, {100,99,1,1},
        {7,7,7,7,7,7,7}, {3,3,3,3,3,3,3,3,3}, {1,2,3,4,5,6,7,8,9,10},
        {10,9,8,7,6,5,4,3,2,1}, {50,50,50}, {100,100,100,100,100},
        {1000,100,10,1}, {999,998,1,1}, {40,20,10,5,2,1},
        {33,33,33,1}, {50,25,12,6,3,1}, {80,40,20,10,5},
        {1,1,1,1,1,1,1,1,1,1,1,1,1,1,1}, {100,50,25,12,6,3,1},
        {1000000,1000,10,1}, {7,3}, {1,1000000}, {99,1},
    };
    int casens[] = {4,6,4,10,12,10,6,4,7,9,10,10,3,5,4,4,6,4,6,5,15,7,4,2,2,2};

    for (int c=0;c<sizeof(casens)/sizeof(casens[0]);c++){
        for (int r=0;r<nrects;r++){
            char tag[64]; snprintf(tag,64,"case%d r%d",c,r);
            run(tag, cases[c], casens[c], rects[r]);
        }
    }
    printf("\n=== SUMMARY: %d cases, exact=%d diff=%d ===\n",total,exact,diff);
    return 0;
}
