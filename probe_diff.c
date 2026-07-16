// Diff probe: load reference DLL and candidate DLL, run same cases, report diffs.
#include <windows.h>
#include <stdio.h>
#include <stdint.h>
#include <math.h>

typedef struct { double x, y, w, h; } BtRect;
typedef struct { int64_t index; BtRect rect; } BtTreemapItem;
typedef struct { BtTreemapItem *items; int64_t len, cap; } Res;
typedef int  (*F)(const int64_t*, int64_t, BtRect, Res*);
typedef void (*Rf)(Res*);

static F f_ref, f_cand;
static Rf r_ref, r_cand;
static int total_cases = 0, exact_cases = 0, close_cases = 0, diff_cases = 0;

static int rect_eq(BtRect a, BtRect b, double eps, double *maxerr) {
    double errs[4] = { fabs(a.x-b.x), fabs(a.y-b.y), fabs(a.w-b.w), fabs(a.h-b.h) };
    double m = 0;
    for (int i = 0; i < 4; i++) if (errs[i] > m) m = errs[i];
    if (maxerr) *maxerr = m;
    return m <= eps;
}

static void run(const char *tag, int64_t *s, int n, BtRect rect) {
    Res r1 = {0,0,0}, r2 = {0,0,0};
    f_ref(s, n, rect, &r1);
    f_cand(s, n, rect, &r2);
    total_cases++;
    int len_match = (r1.len == r2.len);
    int all_exact = 1, all_close = 1;
    double maxerr = 0;
    int64_t len = r1.len < r2.len ? r1.len : r2.len;
    for (int64_t i = 0; i < len; i++) {
        if (r1.items[i].index != r2.items[i].index) { all_exact = 0; all_close = 0; }
        double e;
        if (!rect_eq(r1.items[i].rect, r2.items[i].rect, 1e-6, &e)) all_exact = 0;
        if (!rect_eq(r1.items[i].rect, r2.items[i].rect, 1e-3, &e)) all_close = 0;
        if (e > maxerr) maxerr = e;
    }
    if (!len_match) { all_exact = 0; all_close = 0; }
    if (all_exact) exact_cases++;
    else if (all_close) close_cases++;
    else diff_cases++;
    if (!all_exact) {
        printf("DIFF [%s] maxerr=%.4f len=%lld/%lld %s\n", tag, maxerr,
               (long long)r1.len, (long long)r2.len, len_match ? "" : "(LEN MISMATCH)");
        printf("  ref : ");
        for (int64_t i = 0; i < r1.len; i++)
            printf("idx%lld=(%.4f,%.4f,%.4f,%.4f) ", (long long)r1.items[i].index,
                   r1.items[i].rect.x, r1.items[i].rect.y, r1.items[i].rect.w, r1.items[i].rect.h);
        printf("\n  cand: ");
        for (int64_t i = 0; i < r2.len; i++)
            printf("idx%lld=(%.4f,%.4f,%.4f,%.4f) ", (long long)r2.items[i].index,
                   r2.items[i].rect.x, r2.items[i].rect.y, r2.items[i].rect.w, r2.items[i].rect.h);
        printf("\n");
    }
    r_ref(&r1); r_cand(&r2);
}

int main(int argc, char **argv) {
    HMODULE href = LoadLibraryA(argv[1]);
    HMODULE hcand = LoadLibraryA(argv[2]);
    if (!href)  { printf("ref load fail\n"); return 1; }
    if (!hcand) { printf("cand load fail\n"); return 1; }
    f_ref = (F)GetProcAddress(href, "bt_layout_treemap");
    r_ref = (Rf)GetProcAddress(href, "bt_release_layout_treemap");
    f_cand = (F)GetProcAddress(hcand, "bt_layout_treemap");
    r_cand = (Rf)GetProcAddress(hcand, "bt_release_layout_treemap");
    setvbuf(stdout, NULL, _IONBF, 0);

    BtRect sq = {0,0,100,100};
    BtRect r1000x600 = {0,0,1000,600};
    BtRect r600x1000 = {0,0,600,1000};
    BtRect r300x200 = {0,0,300,200};

    // probe_g2 cases
    { int64_t s[]={40,30,20,10};       run("G2 [40,30,20,10] 100x100", s,4, sq); }
    { int64_t s[]={60,30,20,15,10,5};  run("G1 6n 1000x600", s,6, r1000x600); }
    { int64_t s[]={25,25,25,25};       run("G7 4eq 100x100", s,4, sq); }
    { int64_t s[]={15,10,5};           run("3n [15,10,5]", s,3, sq); }
    { int64_t s[]={20,10,5};           run("3n [20,10,5]", s,3, sq); }
    { int64_t s[]={50,30,15};          run("3n [50,30,15]", s,3, sq); }
    { int64_t s[]={40,30,20};          run("3n [40,30,20]", s,3, sq); }
    { int64_t s[]={40,30,20,20};       run("4n [40,30,20,20]", s,4, sq); }
    { int64_t s[]={40,30,20,5};        run("4n [40,30,20,5]", s,4, sq); }
    { int64_t s[]={50,30,20,10};       run("4n [50,30,20,10]", s,4, sq); }
    { int64_t s[]={40,40,20,10};       run("4n [40,40,20,10]", s,4, sq); }
    { int64_t s[]={60,30,20,10};       run("4n [60,30,20,10]", s,4, sq); }

    // equal counts
    { int64_t s[]={10,10,10,10};       run("4eq", s,4, sq); }
    { int64_t s[]={10,10,10,10,10,10}; run("6eq", s,6, sq); }
    { int64_t s[]={10,10,10,10,10,10,10,10}; run("8eq", s,8, sq); }
    { int64_t s[]={10,10,10};          run("3eq", s,3, sq); }
    { int64_t s[]={10,10,10,10,10};    run("5eq", s,5, sq); }
    { int64_t s[]={10,10,10,10,10,10,10,10,10,10}; run("10eq", s,10, sq); }
    { int64_t s[]={10,10,10,10,10,10,10,10,10,10,10,10}; run("12eq", s,12, sq); }

    // non-square rects
    { int64_t s[]={40,30,20,10};       run("G2 1000x600", s,4, r1000x600); }
    { int64_t s[]={40,30,20,10};       run("G2 600x1000", s,4, r600x1000); }
    { int64_t s[]={60,30,20,15,10,5};  run("G1 100x100", s,6, sq); }
    { int64_t s[]={40,30,20,10};       run("G2 300x200", s,4, r300x200); }

    // edge: 1, 2 nodes
    { int64_t s[]={50};                run("1n", s,1, sq); }
    { int64_t s[]={60,40};             run("2n", s,2, sq); }
    { int64_t s[]={50,50};             run("2eq", s,2, sq); }

    // edge: zeros / negatives
    { int64_t s[]={0,0,0,0};           run("4zero", s,4, sq); }
    { int64_t s[]={40,0,20,10};        run("with-zero", s,4, sq); }
    { int64_t s[]={-5,30,20,10};       run("with-neg", s,4, sq); }

    // larger random-ish
    { int64_t s[]={100,80,60,40,30,20,15,10,5,2}; run("10n 100x100", s,10, sq); }
    { int64_t s[]={100,80,60,40,30,20,15,10,5,2}; run("10n 1000x600", s,10, r1000x600); }
    { int64_t s[]={50,40,30,25,20,15,12,10,8,5,3,1}; run("12n 100x100", s,12, sq); }
    { int64_t s[]={1000,1,1,1,1,1};    run("1huge", s,6, sq); }
    { int64_t s[]={100,99,1,1};        run("near-equal-pair", s,4, sq); }
    { int64_t s[]={30,25,20,15,10};    run("5n", s,5, sq); }
    { int64_t s[]={30,25,20,15,10,5,4,3,2,1}; run("10n-stair", s,10, sq); }

    printf("\n=== SUMMARY: %d cases, exact=%d close=%d diff=%d ===\n",
           total_cases, exact_cases, close_cases, diff_cases);
    return 0;
}
