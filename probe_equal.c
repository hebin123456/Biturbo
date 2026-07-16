#include <windows.h>
#include <stdio.h>
#include <stdint.h>
typedef struct { double x,y,w,h; } BtRect;
typedef struct { int64_t index; BtRect rect; } BtTreemapItem;
typedef struct { BtTreemapItem *items; int64_t len,cap; } Res;
typedef int (*F)(const int64_t*,int64_t,BtRect,Res*);
typedef void (*Rf)(Res*);
int main(int argc,char**argv){
    HMODULE h=LoadLibraryA(argv[1]); F f=(F)GetProcAddress(h,"bt_layout_treemap"); Rf r=(Rf)GetProcAddress(h,"bt_release_layout_treemap");
    setvbuf(stdout,NULL,_IONBF,0);
    BtRect rect={0,0,100,100};
    // 4 equal various magnitudes
    int64_t cases[][4]={{10,10,10,10},{100,100,100,100},{1,1,1,1},{3,3,3,3}};
    for(int c=0;c<4;c++){
        Res res={0,0,0}; f(cases[c],4,rect,&res);
        printf("4x%lld: ",(long long)cases[c][0]);
        for(int i=0;i<4;i++) printf("idx%lld=(%.3f,%.3f,%.3f,%.3f) ",(long long)res.items[i].index,res.items[i].rect.x,res.items[i].rect.y,res.items[i].rect.w,res.items[i].rect.h);
        printf("\n"); r(&res);
    }
    // 6 equal
    { int64_t s[]={10,10,10,10,10,10}; Res res={0,0,0}; f(s,6,rect,&res);
      printf("6x10: "); for(int i=0;i<6;i++) printf("idx%lld=(%.3f,%.3f,%.3f,%.3f) ",(long long)res.items[i].index,res.items[i].rect.x,res.items[i].rect.y,res.items[i].rect.w,res.items[i].rect.h); printf("\n"); r(&res); }
    // 8 equal
    { int64_t s[]={10,10,10,10,10,10,10,10}; Res res={0,0,0}; f(s,8,rect,&res);
      printf("8x10: "); for(int i=0;i<8;i++) printf("idx%lld=(%.3f,%.3f,%.3f,%.3f) ",(long long)res.items[i].index,res.items[i].rect.x,res.items[i].rect.y,res.items[i].rect.w,res.items[i].rect.h); printf("\n"); r(&res); }
    // 3 equal
    { int64_t s[]={10,10,10}; Res res={0,0,0}; f(s,3,rect,&res);
      printf("3x10: "); for(int i=0;i<3;i++) printf("idx%lld=(%.3f,%.3f,%.3f,%.3f) ",(long long)res.items[i].index,res.items[i].rect.x,res.items[i].rect.y,res.items[i].rect.w,res.items[i].rect.h); printf("\n"); r(&res); }
    // 5 equal
    { int64_t s[]={10,10,10,10,10}; Res res={0,0,0}; f(s,5,rect,&res);
      printf("5x10: "); for(int i=0;i<5;i++) printf("idx%lld=(%.3f,%.3f,%.3f,%.3f) ",(long long)res.items[i].index,res.items[i].rect.x,res.items[i].rect.y,res.items[i].rect.w,res.items[i].rect.h); printf("\n"); r(&res); }
    return 0;
}
