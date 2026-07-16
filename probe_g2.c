#include <windows.h>
#include <stdio.h>
#include <stdint.h>

typedef struct { double x, y, w, h; } BtRect;
typedef struct { int64_t index; BtRect rect; } BtTreemapItem;
typedef struct { BtTreemapItem *items; int64_t len, cap; } Res;
typedef int  (*F)(const int64_t*, int64_t, BtRect, Res*);
typedef void (*Rf)(Res*);

static void run(F f, Rf r, const char *tag, int64_t *s, int n, BtRect rect) {
    Res res = {0,0,0};
    f(s, n, rect, &res);
    printf("%s : ", tag);
    for (int i = 0; i < n; i++) {
        BtTreemapItem *it = &res.items[i];
        printf("idx%lld=(%.4f,%.4f,%.4f,%.4f) ",
               (long long)it->index, it->rect.x, it->rect.y, it->rect.w, it->rect.h);
    }
    printf("\n");
    r(&res);
}

int main(int argc, char **argv) {
    HMODULE h = LoadLibraryA(argv[1]);
    if (!h) { printf("load failed\n"); return 1; }
    F f = (F)GetProcAddress(h, "bt_layout_treemap");
    Rf r = (Rf)GetProcAddress(h, "bt_release_layout_treemap");
    setvbuf(stdout, NULL, _IONBF, 0);

    BtRect sq = {0,0,100,100};
    BtRect r1000x600 = {0,0,1000,600};

    { int64_t s[] = {40,30,20,10};       run(f,r,"G2 [40,30,20,10] 100x100      ", s,4, sq); }
    { int64_t s[] = {60,30,20,15,10,5};  run(f,r,"G1 [60,30,20,15,10,5] 1000x600 ", s,6, r1000x600); }
    { int64_t s[] = {25,25,25,25};       run(f,r,"G7 [25,25,25,25] 100x100       ", s,4, sq); }
    { int64_t s[] = {15,10,5};           run(f,r,"3n [15,10,5] 100x100           ", s,3, sq); }
    { int64_t s[] = {20,10,5};           run(f,r,"3n [20,10,5] 100x100           ", s,3, sq); }
    { int64_t s[] = {50,30,15};          run(f,r,"3n [50,30,15] 100x100          ", s,3, sq); }
    { int64_t s[] = {40,30,20};          run(f,r,"3n [40,30,20] 100x100          ", s,3, sq); }
    { int64_t s[] = {40,30,20,20};       run(f,r,"4n [40,30,20,20] 100x100       ", s,4, sq); }
    { int64_t s[] = {40,30,20,5};        run(f,r,"4n [40,30,20,5] 100x100        ", s,4, sq); }
    { int64_t s[] = {50,30,20,10};       run(f,r,"4n [50,30,20,10] 100x100       ", s,4, sq); }
    { int64_t s[] = {40,40,20,10};       run(f,r,"4n [40,40,20,10] 100x100       ", s,4, sq); }
    { int64_t s[] = {60,30,20,10};       run(f,r,"4n [60,30,20,10] 100x100       ", s,4, sq); }
    return 0;
}
