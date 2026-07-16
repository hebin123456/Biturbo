#!/usr/bin/env python3
"""Reference standard squarify (Bruls 2000) to compare against the DLL output."""
import sys

def squarify(sizes, rect):
    # sizes: list of (index, size) already sorted desc by size, size>0
    x, y, w, h = rect
    total = sum(s for _, s in sizes)
    canvas = w * h
    # fixed areas
    areas = [(idx, canvas * s / total) for idx, s in sizes]
    items = []
    remaining = [x, y, w, h]
    row = []
    i = 0
    while i < len(areas):
        cand = worst(row, areas, i, remaining)
        cur = worst(row, areas, None, remaining) if row else float('inf')
        if row and cand > cur:
            layout_row(row, areas, remaining, items)
            row = []
        row.append(i)
        i += 1
    if row:
        layout_row(row, areas, remaining, items)
    return items

def worst(row, areas, cand, rem):
    rx, ry, rw, rh = rem
    s = 0.0; mn = float('inf'); mx = 0.0
    for k in row:
        a = areas[k][1]; s += a; mn = min(mn, a); mx = max(mx, a)
    if cand is not None:
        a = areas[cand][1]; s += a; mn = min(mn, a); mx = max(mx, a)
    if s <= 0 or mn <= 0: return float('inf')
    side = min(rw, rh)
    if side <= 0: return float('inf')
    s2 = side * side
    return max(s2 * mx / (s*s), (s*s) / (s2 * mn))

def layout_row(row, areas, rem, items):
    rx, ry, rw, rh = rem
    s = sum(areas[k][1] for k in row)
    if s <= 0 or not row: return
    if rw <= rh:
        t = s / rw
        cx = rx
        for k in row:
            a = areas[k][1]
            w = a / t if t > 0 else 0
            items.append((areas[k][0], (cx, ry, w, t)))
            cx += w
        rem[1] = ry + t; rem[3] = rh - t
    else:
        t = s / rh
        cy = ry
        for k in row:
            a = areas[k][1]
            h = a / t if t > 0 else 0
            items.append((areas[k][0], (rx, cy, t, h)))
            cy += h
        rem[0] = rx + t; rem[2] = rw - t

def run(tag, sizes, rect):
    idxs = sorted(range(len(sizes)), key=lambda i: -sizes[i] if sizes[i] > 0 else 0)
    nodes = [(i, sizes[i] if sizes[i] > 0 else 0) for i in idxs]
    total = sum(s for _, s in nodes)
    print(f"\n=== {tag} ===")
    if total <= 0:
        for i, s in nodes:
            print(f"  idx={i} 1x1")
        return
    if len(nodes) < 3:
        # simple
        if len(nodes) == 1:
            print(f"  idx={nodes[0][0]} {rect}")
        else:
            d = nodes[0][1] + nodes[1][1]
            frac = nodes[0][1]/d if d>0 else 0.5
            x,y,w,h = rect
            if w >= h:
                w0 = w*frac
                print(f"  idx={nodes[0][0]} ({x},{y},{w0},{h})")
                print(f"  idx={nodes[1][0]} ({x+w0},{y},{w-w0},{h})")
            else:
                h0 = h*frac
                print(f"  idx={nodes[0][0]} ({x},{y},{w},{h0})")
                print(f"  idx={nodes[1][0]} ({x},{y+h0},{w},{h-h0})")
        return
    items = squarify(nodes, rect)
    for idx, r in items:
        print(f"  idx={idx} x={r[0]:.4f} y={r[1]:.4f} w={r[2]:.4f} h={r[3]:.4f}")

run("G2 [40,30,20,10] 100x100", [40,30,20,10], (0,0,100,100))
run("G7 [25,25,25,25] 100x100", [25,25,25,25], (0,0,100,100))
run("G1 [60,30,20,15,10,5] 1000x600", [60,30,20,15,10,5], (0,0,1000,600))
run("G3 [50,30,20] 100x100", [50,30,20], (0,0,100,100))
