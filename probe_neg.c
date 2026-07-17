// Probe negative/zero edge cases to understand ref's special handling.
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
    printf("%-30s : ", tag);
    for (int i = 0; i < n; i++) {
        BtTreemapItem *it = &res.items[i];
        printf("idx%lld=(%.2f,%.2f,%.2f,%.2f) ",
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
    BtRect sq = {0,0,100,100};

    { int64_t s[]={-5,30,20,10};       run("[-5,30,20,10]", s,4, sq); }
    { int64_t s[]={30,-5,20,10};       run("[30,-5,20,10]", s,4, sq); }
    { int64_t s[]={30,20,10,-5};       run("[30,20,10,-5]", s,4, sq); }
    { int64_t s[]={-5,-5,20,10};       run("[-5,-5,20,10]", s,4, sq); }
    { int64_t s[]={-100,30,20,10};     run("[-100,30,20,10]", s,4, sq); }
    { int64_t s[]={-1,30,20,10};       run("[-1,30,20,10]", s,4, sq); }
    { int64_t s[]={-5,30,20};          run("[-5,30,20] 3n", s,3, sq); }
    { int64_t s[]={-5,30};             run("[-5,30] 2n", s,2, sq); }
    { int64_t s[]={-5};                run("[-5] 1n", s,1, sq); }
    { int64_t s[]={-5,-10,-20};        run("[-5,-10,-20] all-neg", s,3, sq); }
    { int64_t s[]={0,30,20,10};        run("[0,30,20,10]", s,4, sq); }
    { int64_t s[]={30,0,20,10};        run("[30,0,20,10]", s,4, sq); }
    { int64_t s[]={30,20,0,10};        run("[30,20,0,10]", s,4, sq); }
    { int64_t s[]={30,20,10,0};        run("[30,20,10,0]", s,4, sq); }
    { int64_t s[]={INT64_MIN,30,20};   run("[MIN,30,20]", s,3, sq); }
    { int64_t s[]={INT64_MAX,30,20};   run("[MAX,30,20]", s,3, sq); }
    return 0;
}
