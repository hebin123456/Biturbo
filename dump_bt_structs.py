#!/usr/bin/env python3
"""Directly dump Field + FieldLayout + ClassLayout rows relevant to the
treemap ABI, bypassing dnfile's FieldList navigation (which is finicky).

We want the C# [StructLayout] for: BtRect, BtTreemapItem, BtLayoutTreemapResult
and the P/Invoke signature for bt_layout_treemap.
"""
import dnfile

pe = dnfile.dnPE("ref_dll/ForkPlus.exe")
md = pe.net.mdtables

# 1. ClassLayout: typedef rid -> (pack, size)
cls = {}
if md.ClassLayout is not None:
    for r in md.ClassLayout.rows:
        cls[r.Parent.row_index] = (r.PackingSize, r.ClassSize)

# 2. TypeDef names
td_names = {}
if md.TypeDef is not None:
    for i, r in enumerate(md.TypeDef.rows, start=1):
        td_names[i] = (str(r.TypeNamespace), str(r.TypeName))

# 3. Field names + flags
fld = {}
if md.Field is not None:
    for i, r in enumerate(md.Field.rows, start=1):
        fld[i] = (str(r.Name), r.Flags)

# 4. FieldLayout: field rid -> offset
fl = {}
if md.FieldLayout is not None:
    for r in md.FieldLayout.rows:
        fl[r.Field.row_index] = r.Offset

# We need to know which fields belong to which typedef. Build a map by
# walking the FieldList coded index stream from the TypeDef table.
# dnfile exposes row.FieldList as a SimpleIndex; .row_index is the 1-based
# Field rid where this typedef's fields start.
typedef_field_ranges = {}
if md.TypeDef is not None:
    starts = []
    for r in md.TypeDef.rows:
        fl_obj = r.FieldList
        try:
            starts.append(fl_obj.row_index)
        except AttributeError:
            starts.append(0)
    n_fields = len(fld)
    for ti in range(len(starts)):
        s = starts[ti]
        e = starts[ti + 1] if ti + 1 < len(starts) else n_fields + 1
        typedef_field_ranges[ti + 1] = (s, e)

WANT = {"BtRect", "BtTreemapItem", "BtLayoutTreemapResult"}
print("==== Struct layouts (C# [StructLayout]) ====")
for ti, (ns, name) in td_names.items():
    if name not in WANT:
        continue
    s, e = typedef_field_ranges.get(ti, (0, 0))
    pack = cls.get(ti)
    print(f"\n#{ti} {ns}.{name}  ClassLayout(pack,size)={pack}")
    for fr in range(s, e):
        if fr == 0 or fr > len(fld):
            continue
        fname, fflags = fld[fr]
        off = fl.get(fr)
        # Field flags low bits = access; bit 0x10 = static; 0x100 = literal
        print(f"  +0x{off:02x}  {fname}  (fieldflags=0x{fflags:x})")

# 5. ImplMap: P/Invoke
print("\n==== P/Invoke (ImplMap) for treemap ====")
if md.ImplMap is not None:
    cc_names = {1: "Cdecl", 2: "Stdcall", 3: "Thiscall", 4: "Fastcall", 5: "Varargs", 0: "Winapi(?)"}
    for r in md.ImplMap.rows:
        iname = str(r.ImportName)
        if "treemap" in iname.lower():
            flags = r.MappingFlags
            print(f"  {iname}: MappingFlags=0x{flags:x} cc={cc_names.get(flags & 0x7, flags & 0x7)}  scope_row={r.ImportScope.row_index}")

# 6. ModuleRef names (which DLL is imported)
print("\n==== ModuleRef ====")
if md.ModuleRef is not None:
    for i, r in enumerate(md.ModuleRef.rows, start=1):
        print(f"  #{i}: {r.Name}")

# 7. MethodDef for bt_layout_treemap wrapper: name + signature bytes
print("\n==== MethodDef: bt_layout_treemap wrapper ====")
if md.MethodDef is not None:
    for mi, r in enumerate(md.MethodDef.rows, start=1):
        mname = str(r.Name)
        if mname == "bt_layout_treemap" or mname == "bt_release_layout_treemap":
            sig = r.Signature
            print(f"  #{mi} {mname}: Flags=0x{r.Flags:x} ImplFlags=0x{r.ImplFlags:x}")
            print(f"    sig hex: {sig.hex() if sig else None}")
            # Param table range
            ps = r.ParamList
            try:
                pstart = ps.row_index
            except AttributeError:
                pstart = 0
            print(f"    ParamList start={pstart}")

# 8. Param names for the treemap method
print("\n==== Param names for bt_layout_treemap ====")
if md.Param is not None and md.MethodDef is not None:
    # find method
    for mi, r in enumerate(md.MethodDef.rows, start=1):
        if str(r.Name) != "bt_layout_treemap":
            continue
        try:
            pstart = r.ParamList.row_index
        except AttributeError:
            continue
        pend = (
            md.MethodDef.rows[mi].ParamList.row_index
            if mi < len(md.MethodDef.rows)
            else len(md.Param.rows) + 1
        )
        for pr in range(pstart, pend):
            if pr == 0 or pr > len(md.Param.rows):
                continue
            prow = md.Param.rows[pr - 1]
            print(f"  Param #{pr}: seq={prow.Sequence} name={prow.Name} flags=0x{prow.Flags:x}")
