// Precise probe: isolate the 10n n=7 subproblem to see exact ref behavior.
#include <windows.h>
#include <stdio.h>
#include <stdint.h>

typedef struct { double x, y, w, h; } BtRect;
typedef struct { int64_t index; BtRect rect; } BtTreemapItem;
typedef struct { BtTreemapItem *items; int64_t len, cap; } Res;
typedef int  (*F)(const int64_t*, int64_t, BtRect, Res*);
typedef void (*Rf)(Res*);

static F f; static Rf rf;

static void run(const char *tag, int64_t *s, int n, BtRect rect) {
    Res res = {0,0,0};
    f(s, n, rect, &res);
    printf("%-32s : ", tag);
    for (int i = 0; i < n; i++) {
        BtTreemapItem *it = &res.items[i];
        printf("idx%lld=(%.4f,%.4f,%.4f,%.4f) ",
               (long long)it->index, it->rect.x, it->rect.y, it->rect.w, it->rect.h);
    }
    printf("\n");
    rf(&res);
}

int main(int argc, char **argv) {
    HMODULE h = LoadLibraryA(argv[1]);
    f = (F)GetProcAddress(h, "bt_layout_treemap");
    rf = (Rf)GetProcAddress(h, "bt_release_layout_treemap");
    setvbuf(stdout, NULL, _IONBF, 0);

    // The exact 10n n=7 subproblem: sorted [40,30,20,15,10,5,2] on (55.5556,33.3333,44.4444,66.6667)
    // Input order doesn't matter (dll sorts), so pass sorted.
    { int64_t s[]={40,30,20,15,10,5,2};
      run("n7 sub 44.44x66.67", s,7, (BtRect){55.5556,33.3333,44.4444,66.6667}); }
    // Same sizes, origin 0, to see pure layout
    { int64_t s[]={40,30,20,15,10,5,2};
      run("n7 sub 44.44x66.67 @0", s,7, (BtRect){0,0,44.4444,66.6667}); }
    // Try varying the rect slightly around the boundary
    { int64_t s[]={40,30,20,15,10,5,2};
      run("n7 44x66", s,7, (BtRect){0,0,44,66}); }
    { int64_t s[]={40,30,20,15,10,5,2};
      run("n7 45x66", s,7, (BtRect){0,0,45,66}); }
    { int64_t s[]={40,30,20,15,10,5,2};
      run("n7 44x67", s,7, (BtRect){0,0,44,67}); }
    // Isolate: just [40,30] 2-node on various rects (base case, no recursion)
    { int64_t s[]={40,30};
      run("2n 44x66", s,2, (BtRect){0,0,44,66}); }
    { int64_t s[]={40,30};
      run("2n 66x44", s,2, (BtRect){0,0,66,44}); }
    // 3-node where a1=0.5 exactly on non-square
    // [40,40,20]: frac0=40/80=0.5. a1=long*0.5/short. For a1=0.5: long=short (square).
    // For non-square a1!=0.5. Try [20,20,10] on 60x120: long=120,short=60,a1=120*0.5/60=1.0
    { int64_t s[]={20,20,10};
      run("3n [20,20,10] 60x120", s,3, (BtRect){0,0,60,120}); }
    { int64_t s[]={20,20,10};
      run("3n [20,20,10] 120x60", s,3, (BtRect){0,0,120,60}); }
    // [40,40,20] square (a1=0.5, n=2 if worst(2)<=worst(1))
    { int64_t s[]={40,40,20};
      run("3n [40,40,20] sq", s,3, (BtRect){0,0,100,100}); }
    // 4-node [40,40,20,10]: sum_excl=100-10=90, frac0=40/90=0.444, a1=0.444 (n=2 std)
    { int64_t s[]={40,40,20,10};
      run("4n [40,40,20,10] sq", s,4, (BtRect){0,0,100,100}); }
    // The 10n top-level for reference
    { int64_t s[]={100,80,60,40,30,20,15,10,5,2};
      run("10n full 100x100", s,10, (BtRect){0,0,100,100}); }
    // n=7 on exact square 100x100 to compare
    { int64_t s[]={40,30,20,15,10,5,2};
      run("n7 100x100", s,7, (BtRect){0,0,100,100}); }
    return 0;
}
